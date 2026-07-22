use crate::{
    command::user_resumes::UserResumes,
    config::{self, AppRuntimeConfig, CURRENT_SCHEMA_VERSION},
    credential::{set_with_backend, CredentialBackend},
    error::AppError,
    storage::atomic::atomic_write,
};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct MigrationPaths {
    pub config_path: PathBuf,
    pub app_data_dir: PathBuf,
}

impl MigrationPaths {
    pub fn new(config_path: PathBuf, app_data_dir: PathBuf) -> Self {
        Self {
            config_path,
            app_data_dir,
        }
    }

    pub fn legacy_user_resumes_path(&self) -> PathBuf {
        self.app_data_dir.join("user_resumes.json")
    }

    pub fn user_resumes_path(&self) -> PathBuf {
        self.app_data_dir.join("data").join("user_resumes.json")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.app_data_dir.join("backups")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeMigrationAction {
    None,
    OldCopied,
    TargetKept,
    DuplicateRemoved,
    Merged,
    TargetRestored,
    CorruptLegacyIgnored,
}

#[derive(Debug, Clone)]
pub struct MigrationReport {
    pub migrated: bool,
    pub resume_action: ResumeMigrationAction,
    pub backups: Vec<PathBuf>,
    pub conflict_report: Option<PathBuf>,
}

impl MigrationReport {
    fn not_needed() -> Self {
        Self {
            migrated: false,
            resume_action: ResumeMigrationAction::None,
            backups: Vec::new(),
            conflict_report: None,
        }
    }

    fn started() -> Self {
        Self {
            migrated: true,
            ..Self::not_needed()
        }
    }
}

pub fn migrate_v0_to_v1<B: CredentialBackend + ?Sized>(
    paths: &MigrationPaths,
    credential_backend: &B,
) -> Result<MigrationReport, AppError> {
    let original_config = read_optional(&paths.config_path)?;
    let raw_config = original_config
        .as_deref()
        .map(|bytes| parse_raw_config(bytes, &paths.config_path))
        .transpose()?;

    let source_schema_version = raw_config
        .as_ref()
        .and_then(|value| value.get("schema_version"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if source_schema_version > u64::from(CURRENT_SCHEMA_VERSION) {
        return Err(AppError::configuration(format!(
            "应用配置版本 {source_schema_version} 高于当前支持的版本 {CURRENT_SCHEMA_VERSION}"
        )));
    }
    if source_schema_version == u64::from(CURRENT_SCHEMA_VERSION) {
        return Ok(MigrationReport::not_needed());
    }

    let mut config = match original_config.as_deref() {
        Some(bytes) => {
            let content = std::str::from_utf8(bytes).map_err(|error| {
                AppError::configuration(format!(
                    "应用配置无法解析：{}",
                    paths.config_path.display()
                ))
                .with_detail(error.to_string())
            })?;
            config::parse_config_content(content).map_err(|error| {
                AppError::configuration(format!(
                    "应用配置无法解析：{}",
                    paths.config_path.display()
                ))
                .with_detail(error)
            })?
        }
        None => config::default_app_config(),
    };

    if let Some(secret) = raw_config.as_ref().and_then(legacy_plaintext_key) {
        let keychain_has_secret = credential_backend
            .get()?
            .is_some_and(|value| !value.trim().is_empty());
        if !keychain_has_secret {
            set_with_backend(credential_backend, secret)?;
        }
    }

    let mut report = MigrationReport::started();
    migrate_user_resumes(paths, &mut report)?;

    config.browser_config.user_data_dir =
        resolve_browser_profile(&config.browser_config.user_data_dir, &paths.app_data_dir)
            .to_string_lossy()
            .into_owned();
    config.schema_version = CURRENT_SCHEMA_VERSION;
    config::validate_and_normalize(&mut config).map_err(AppError::validation)?;

    if let Some(original) = original_config.as_deref() {
        let backup_bytes = match raw_config.as_ref() {
            Some(raw) if legacy_plaintext_key(raw).is_some() => sanitized_config_backup(raw)?,
            _ => original.to_vec(),
        };
        report
            .backups
            .push(write_backup(paths, "app-config.yaml", &backup_bytes)?);
    }

    write_migrated_config(&paths.config_path, &config)?;
    Ok(report)
}

pub fn resolve_browser_profile(configured: &str, app_data_dir: &Path) -> PathBuf {
    let trimmed = configured.trim();
    if !trimmed.is_empty() && trimmed != "null" && trimmed != "None" {
        return PathBuf::from(trimmed);
    }

    let legacy_default = app_data_dir.join("default");
    if legacy_default.exists() {
        legacy_default
    } else {
        app_data_dir.join("browser-profile")
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    if !path.exists() {
        return Ok(None);
    }
    fs::read(path).map(Some).map_err(|error| {
        AppError::storage(format!("无法读取数据文件：{}", path.display()))
            .with_detail(error.to_string())
    })
}

fn parse_raw_config(bytes: &[u8], path: &Path) -> Result<Value, AppError> {
    serde_yaml::from_slice(bytes).map_err(|error| {
        AppError::configuration(format!("应用配置无法解析：{}", path.display()))
            .with_detail(error.to_string())
    })
}

fn legacy_plaintext_key(raw: &Value) -> Option<&str> {
    raw.get("llm_config")
        .and_then(|llm| llm.get("api_key"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|secret| !secret.is_empty())
}

fn write_migrated_config(path: &Path, config: &AppRuntimeConfig) -> Result<(), AppError> {
    let content = serde_yaml::to_string(config).map_err(|error| {
        AppError::configuration("无法序列化应用配置").with_detail(error.to_string())
    })?;
    atomic_write(path, content.as_bytes())
}

type ResumeMap = Map<String, Value>;

fn migrate_user_resumes(
    paths: &MigrationPaths,
    report: &mut MigrationReport,
) -> Result<(), AppError> {
    let legacy_path = paths.legacy_user_resumes_path();
    let target_path = paths.user_resumes_path();
    let legacy_bytes = read_optional(&legacy_path)?;
    let target_bytes = read_optional(&target_path)?;

    match (legacy_bytes, target_bytes) {
        (None, None) => Ok(()),
        (Some(legacy_bytes), None) => {
            parse_resume_map(&legacy_bytes)
                .map_err(|error| corrupt_resume_error(&legacy_path, error))?;
            report.backups.push(write_backup(
                paths,
                "legacy-user-resumes.json",
                &legacy_bytes,
            )?);
            atomic_write(&target_path, &legacy_bytes)?;
            remove_legacy(&legacy_path)?;
            report.resume_action = ResumeMigrationAction::OldCopied;
            Ok(())
        }
        (None, Some(target_bytes)) => {
            parse_resume_map(&target_bytes)
                .map_err(|error| corrupt_resume_error(&target_path, error))?;
            report.resume_action = ResumeMigrationAction::TargetKept;
            Ok(())
        }
        (Some(legacy_bytes), Some(target_bytes)) => migrate_resume_pair(
            paths,
            report,
            &legacy_path,
            &target_path,
            legacy_bytes,
            target_bytes,
        ),
    }
}

fn migrate_resume_pair(
    paths: &MigrationPaths,
    report: &mut MigrationReport,
    legacy_path: &Path,
    target_path: &Path,
    legacy_bytes: Vec<u8>,
    target_bytes: Vec<u8>,
) -> Result<(), AppError> {
    let legacy = parse_resume_map(&legacy_bytes);
    let target = parse_resume_map(&target_bytes);
    match (legacy, target) {
        (Ok(legacy), Ok(target)) if legacy == target => {
            remove_legacy(legacy_path)?;
            report.resume_action = ResumeMigrationAction::DuplicateRemoved;
            Ok(())
        }
        (Ok(legacy), Ok(target)) => {
            let legacy_backup = write_backup(paths, "legacy-user-resumes.json", &legacy_bytes)?;
            let target_backup = write_backup(paths, "target-user-resumes.json", &target_bytes)?;
            report
                .backups
                .extend([legacy_backup.clone(), target_backup.clone()]);

            let mut collisions: Vec<String> = legacy
                .keys()
                .filter(|name| target.contains_key(*name))
                .cloned()
                .collect();
            collisions.sort();
            let mut merged = legacy;
            merged.extend(target);
            report.conflict_report = Some(write_resume_report(
                paths,
                "merged_target_wins",
                &collisions,
                &[legacy_backup, target_backup],
            )?);
            atomic_write(target_path, &serialize_resume_map(&merged)?)?;
            remove_legacy(legacy_path)?;
            report.resume_action = ResumeMigrationAction::Merged;
            Ok(())
        }
        (Ok(_legacy), Err(target_error)) => {
            let legacy_backup = write_backup(paths, "legacy-user-resumes.json", &legacy_bytes)?;
            let target_backup = write_backup(paths, "target-user-resumes.json", &target_bytes)?;
            report
                .backups
                .extend([legacy_backup.clone(), target_backup.clone()]);
            report.conflict_report = Some(write_resume_report(
                paths,
                "restored_target_from_legacy",
                &[],
                &[legacy_backup, target_backup],
            )?);
            atomic_write(target_path, &legacy_bytes)?;
            remove_legacy(legacy_path)?;
            report.resume_action = ResumeMigrationAction::TargetRestored;
            let _ = target_error;
            Ok(())
        }
        (Err(legacy_error), Ok(_target)) => {
            let legacy_backup =
                write_backup(paths, "corrupt-legacy-user-resumes.json", &legacy_bytes)?;
            report.backups.push(legacy_backup.clone());
            report.conflict_report = Some(write_resume_report(
                paths,
                "ignored_corrupt_legacy",
                &[],
                &[legacy_backup],
            )?);
            report.resume_action = ResumeMigrationAction::CorruptLegacyIgnored;
            let _ = legacy_error;
            Ok(())
        }
        (Err(legacy_error), Err(target_error)) => Err(AppError::storage(format!(
            "简历数据损坏，未修改文件：{}；{}",
            legacy_path.display(),
            target_path.display()
        ))
        .with_detail(format!("legacy: {legacy_error}; target: {target_error}"))),
    }
}

fn parse_resume_map(bytes: &[u8]) -> Result<ResumeMap, String> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(ResumeMap::new());
    }
    serde_json::from_slice::<UserResumes>(bytes).map_err(|error| error.to_string())?;
    serde_json::from_slice::<ResumeMap>(bytes).map_err(|error| error.to_string())
}

fn serialize_resume_map(map: &ResumeMap) -> Result<Vec<u8>, AppError> {
    serde_json::to_vec_pretty(map)
        .map_err(|error| AppError::storage("无法序列化简历数据").with_detail(error.to_string()))
}

fn corrupt_resume_error(path: &Path, detail: String) -> AppError {
    AppError::storage(format!("简历数据损坏，未修改文件：{}", path.display())).with_detail(detail)
}

fn remove_legacy(path: &Path) -> Result<(), AppError> {
    fs::remove_file(path).map_err(|error| {
        AppError::storage(format!("无法清理旧简历文件：{}", path.display()))
            .with_detail(error.to_string())
    })
}

fn write_backup(paths: &MigrationPaths, label: &str, bytes: &[u8]) -> Result<PathBuf, AppError> {
    let path = paths
        .backups_dir()
        .join(unique_timestamped_name(label, "bak"));
    atomic_write(&path, bytes)?;
    Ok(path)
}

fn sanitized_config_backup(raw: &Value) -> Result<Vec<u8>, AppError> {
    let mut sanitized = raw.clone();
    if let Some(llm_config) = sanitized
        .get_mut("llm_config")
        .and_then(Value::as_object_mut)
    {
        llm_config.remove("api_key");
    }
    serde_yaml::to_string(&sanitized)
        .map(String::into_bytes)
        .map_err(|error| {
            AppError::configuration("无法创建脱敏配置备份").with_detail(error.to_string())
        })
}

#[derive(Serialize)]
struct ResumeConflictMetadata<'a> {
    action: &'a str,
    collisions: &'a [String],
    backups: Vec<String>,
}

fn write_resume_report(
    paths: &MigrationPaths,
    action: &str,
    collisions: &[String],
    backups: &[PathBuf],
) -> Result<PathBuf, AppError> {
    let metadata = ResumeConflictMetadata {
        action,
        collisions,
        backups: backups
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
    };
    let bytes = serde_json::to_vec_pretty(&metadata).map_err(|error| {
        AppError::storage("无法生成简历迁移报告").with_detail(error.to_string())
    })?;
    let path = paths
        .backups_dir()
        .join(unique_timestamped_name("user-resumes-conflict", "json"));
    atomic_write(&path, &bytes)?;
    Ok(path)
}

fn unique_timestamped_name(label: &str, extension: &str) -> String {
    format!(
        "{}-{}-{label}.{extension}",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        Uuid::new_v4()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        credential::CredentialBackend,
        error::{AppError, AppErrorCode},
    };
    use serde_json::{json, Value};
    use std::{cell::RefCell, fs, path::Path};

    #[derive(Default)]
    struct FakeCredentialBackend {
        value: RefCell<Option<String>>,
        fail_set: bool,
    }

    impl CredentialBackend for FakeCredentialBackend {
        fn get(&self) -> Result<Option<String>, AppError> {
            Ok(self.value.borrow().clone())
        }

        fn set(&self, secret: &str) -> Result<(), AppError> {
            if self.fail_set {
                return Err(AppError::credential("无法保存大模型密钥"));
            }
            *self.value.borrow_mut() = Some(secret.to_string());
            Ok(())
        }

        fn delete(&self) -> Result<(), AppError> {
            *self.value.borrow_mut() = None;
            Ok(())
        }
    }

    fn paths(root: &Path) -> MigrationPaths {
        MigrationPaths::new(
            root.join("config").join("app_config.yaml"),
            root.join("app-data"),
        )
    }

    fn legacy_config(user_data_dir: &str) -> Vec<u8> {
        format!(
            r#"onboarding_completed: false
llm_config: null
browser_config:
  user_data_dir: "{user_data_dir}"
  chrome_exe_path: null
"#
        )
        .into_bytes()
    }

    fn write_config(paths: &MigrationPaths, content: &[u8]) {
        fs::create_dir_all(paths.config_path.parent().unwrap()).unwrap();
        fs::write(&paths.config_path, content).unwrap();
    }

    fn resume_document(entries: &[(&str, &str)]) -> Value {
        Value::Object(
            entries
                .iter()
                .map(|(name, content)| {
                    (
                        (*name).to_string(),
                        json!({"content": content, "thumbnail": null}),
                    )
                })
                .collect(),
        )
    }

    fn write_json(path: &Path, value: &Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }

    fn resume_backups(report: &MigrationReport) -> Vec<&std::path::PathBuf> {
        report
            .backups
            .iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains("user-resumes"))
            })
            .collect()
    }

    #[test]
    fn old_only_valid_is_atomically_migrated_and_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write_config(&paths, &legacy_config(""));
        let expected = resume_document(&[("old", "old content")]);
        write_json(&paths.legacy_user_resumes_path(), &expected);

        let report = migrate_v0_to_v1(&paths, &FakeCredentialBackend::default()).unwrap();

        assert_eq!(report.resume_action, ResumeMigrationAction::OldCopied);
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(paths.user_resumes_path()).unwrap()).unwrap(),
            expected
        );
        assert!(!paths.legacy_user_resumes_path().exists());
        assert_eq!(resume_backups(&report).len(), 1);
        assert!(report
            .backups
            .iter()
            .all(|path| path.starts_with(paths.backups_dir())));
    }

    #[test]
    fn target_only_valid_is_used_without_resume_backup_or_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write_config(&paths, &legacy_config(""));
        let expected = resume_document(&[("target", "target content")]);
        write_json(&paths.user_resumes_path(), &expected);
        let original = fs::read(paths.user_resumes_path()).unwrap();

        let report = migrate_v0_to_v1(&paths, &FakeCredentialBackend::default()).unwrap();

        assert_eq!(report.resume_action, ResumeMigrationAction::TargetKept);
        assert_eq!(fs::read(paths.user_resumes_path()).unwrap(), original);
        assert!(resume_backups(&report).is_empty());
    }

    #[test]
    fn semantically_equal_files_use_target_without_duplicate_backup() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write_config(&paths, &legacy_config(""));
        let expected = resume_document(&[("same", "same content")]);
        write_json(&paths.legacy_user_resumes_path(), &expected);
        fs::create_dir_all(paths.user_resumes_path().parent().unwrap()).unwrap();
        fs::write(
            paths.user_resumes_path(),
            serde_json::to_string(&expected).unwrap(),
        )
        .unwrap();

        let report = migrate_v0_to_v1(&paths, &FakeCredentialBackend::default()).unwrap();

        assert_eq!(
            report.resume_action,
            ResumeMigrationAction::DuplicateRemoved
        );
        assert!(!paths.legacy_user_resumes_path().exists());
        assert!(resume_backups(&report).is_empty());
    }

    #[test]
    fn different_valid_files_merge_by_name_with_target_winning_and_report() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write_config(&paths, &legacy_config(""));
        write_json(
            &paths.legacy_user_resumes_path(),
            &resume_document(&[("shared", "old"), ("old-only", "old-only")]),
        );
        write_json(
            &paths.user_resumes_path(),
            &resume_document(&[("shared", "target"), ("target-only", "target-only")]),
        );

        let report = migrate_v0_to_v1(&paths, &FakeCredentialBackend::default()).unwrap();

        assert_eq!(report.resume_action, ResumeMigrationAction::Merged);
        let merged: Value =
            serde_json::from_slice(&fs::read(paths.user_resumes_path()).unwrap()).unwrap();
        assert_eq!(merged["shared"]["content"], "target");
        assert_eq!(merged["old-only"]["content"], "old-only");
        assert_eq!(merged["target-only"]["content"], "target-only");
        assert_eq!(resume_backups(&report).len(), 2);

        let report_path = report.conflict_report.unwrap();
        let metadata: Value = serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
        assert_eq!(metadata["action"], "merged_target_wins");
        assert_eq!(metadata["collisions"], json!(["shared"]));
    }

    #[test]
    fn corrupt_target_and_valid_old_restores_target_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write_config(&paths, &legacy_config(""));
        let old = resume_document(&[("restored", "safe")]);
        write_json(&paths.legacy_user_resumes_path(), &old);
        fs::create_dir_all(paths.user_resumes_path().parent().unwrap()).unwrap();
        fs::write(paths.user_resumes_path(), b"{ corrupt").unwrap();

        let report = migrate_v0_to_v1(&paths, &FakeCredentialBackend::default()).unwrap();

        assert_eq!(report.resume_action, ResumeMigrationAction::TargetRestored);
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(paths.user_resumes_path()).unwrap()).unwrap(),
            old
        );
        assert_eq!(resume_backups(&report).len(), 2);
        let metadata: Value =
            serde_json::from_slice(&fs::read(report.conflict_report.unwrap()).unwrap()).unwrap();
        assert_eq!(metadata["action"], "restored_target_from_legacy");
    }

    #[test]
    fn both_corrupt_aborts_without_writing_either_or_bumping_schema() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let original_config = legacy_config("");
        write_config(&paths, &original_config);
        fs::create_dir_all(paths.app_data_dir.clone()).unwrap();
        fs::write(paths.legacy_user_resumes_path(), b"old corrupt").unwrap();
        fs::create_dir_all(paths.user_resumes_path().parent().unwrap()).unwrap();
        fs::write(paths.user_resumes_path(), br#"{"broken": 42}"#).unwrap();

        let error = migrate_v0_to_v1(&paths, &FakeCredentialBackend::default()).unwrap_err();

        assert_eq!(error.code, AppErrorCode::Storage);
        assert!(error
            .message
            .contains(&paths.legacy_user_resumes_path().display().to_string()));
        assert!(error
            .message
            .contains(&paths.user_resumes_path().display().to_string()));
        assert_eq!(fs::read(&paths.config_path).unwrap(), original_config);
        assert_eq!(
            fs::read(paths.legacy_user_resumes_path()).unwrap(),
            b"old corrupt"
        );
        assert_eq!(
            fs::read(paths.user_resumes_path()).unwrap(),
            br#"{"broken": 42}"#
        );
    }

    #[test]
    fn rerunning_completed_migration_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write_config(&paths, &legacy_config(""));
        write_json(
            &paths.legacy_user_resumes_path(),
            &resume_document(&[("resume", "content")]),
        );
        let backend = FakeCredentialBackend::default();

        let first = migrate_v0_to_v1(&paths, &backend).unwrap();
        let config_after_first = fs::read(&paths.config_path).unwrap();
        let target_after_first = fs::read(paths.user_resumes_path()).unwrap();
        let backups_after_first = fs::read_dir(paths.backups_dir()).unwrap().count();
        let second = migrate_v0_to_v1(&paths, &backend).unwrap();

        assert!(first.migrated);
        assert!(!second.migrated);
        assert_eq!(second.resume_action, ResumeMigrationAction::None);
        assert_eq!(fs::read(&paths.config_path).unwrap(), config_after_first);
        assert_eq!(
            fs::read(paths.user_resumes_path()).unwrap(),
            target_after_first
        );
        assert_eq!(
            fs::read_dir(paths.backups_dir()).unwrap().count(),
            backups_after_first
        );
    }

    #[test]
    fn legacy_plaintext_key_is_saved_and_removed_from_migrated_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let content = br#"llm_config:
  use_custom: false
  base_url: https://api.openai.com/v1
  model: gpt-compatible
  api_key: plaintext-secret
browser_config:
  user_data_dir: ""
"#;
        write_config(&paths, content);
        let backend = FakeCredentialBackend::default();

        let report = migrate_v0_to_v1(&paths, &backend).unwrap();

        assert_eq!(backend.value.borrow().as_deref(), Some("plaintext-secret"));
        let migrated = fs::read_to_string(&paths.config_path).unwrap();
        assert!(!migrated.contains("plaintext-secret"));
        for backup in report.backups {
            assert!(!fs::read_to_string(backup)
                .unwrap()
                .contains("plaintext-secret"));
        }
        let raw: Value = serde_yaml::from_str(&migrated).unwrap();
        assert!(raw["llm_config"].get("api_key").is_none());
        assert!(migrated.contains("schema_version: 1"));
    }

    #[test]
    fn keyring_failure_preserves_original_config_and_does_not_start_file_migration() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let original = br#"llm_config:
  use_custom: true
  base_url: https://example.test/v1
  model: private
  api_key: plaintext-secret
"#;
        write_config(&paths, original);
        write_json(
            &paths.legacy_user_resumes_path(),
            &resume_document(&[("resume", "content")]),
        );
        let backend = FakeCredentialBackend {
            value: RefCell::new(None),
            fail_set: true,
        };

        let error = migrate_v0_to_v1(&paths, &backend).unwrap_err();

        assert_eq!(error.code, AppErrorCode::Credential);
        assert_eq!(fs::read(&paths.config_path).unwrap(), original);
        assert!(!paths.user_resumes_path().exists());
        assert!(paths.legacy_user_resumes_path().exists());
    }

    #[test]
    fn existing_keychain_secret_is_not_overwritten_by_stale_legacy_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write_config(
            &paths,
            br#"llm_config:
  use_custom: true
  base_url: https://example.test/v1
  model: private
  api_key: stale-secret
"#,
        );
        let backend = FakeCredentialBackend {
            value: RefCell::new(Some("newer-secret".to_string())),
            fail_set: false,
        };

        migrate_v0_to_v1(&paths, &backend).unwrap();

        assert_eq!(backend.value.borrow().as_deref(), Some("newer-secret"));
    }

    #[test]
    fn corrupt_legacy_is_backed_up_but_not_deleted_when_target_is_valid() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        write_config(&paths, &legacy_config(""));
        fs::create_dir_all(paths.app_data_dir.clone()).unwrap();
        fs::write(paths.legacy_user_resumes_path(), b"corrupt legacy").unwrap();
        write_json(
            &paths.user_resumes_path(),
            &resume_document(&[("target", "valid")]),
        );

        let report = migrate_v0_to_v1(&paths, &FakeCredentialBackend::default()).unwrap();

        assert_eq!(
            report.resume_action,
            ResumeMigrationAction::CorruptLegacyIgnored
        );
        assert_eq!(
            fs::read(paths.legacy_user_resumes_path()).unwrap(),
            b"corrupt legacy"
        );
        assert_eq!(resume_backups(&report).len(), 1);
    }

    #[test]
    fn future_schema_version_aborts_without_touching_config_or_data() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        let future = b"schema_version: 2\nllm_config: null\n";
        write_config(&paths, future);
        write_json(
            &paths.legacy_user_resumes_path(),
            &resume_document(&[("legacy", "content")]),
        );

        let error = migrate_v0_to_v1(&paths, &FakeCredentialBackend::default()).unwrap_err();

        assert_eq!(error.code, AppErrorCode::Configuration);
        assert_eq!(fs::read(&paths.config_path).unwrap(), future);
        assert!(!paths.user_resumes_path().exists());
    }

    #[test]
    fn browser_profile_resolution_prefers_explicit_then_legacy_then_new_default() {
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path();
        fs::create_dir(app_data.join("default")).unwrap();

        assert_eq!(
            resolve_browser_profile(" /explicit/profile ", app_data),
            std::path::PathBuf::from("/explicit/profile")
        );
        assert_eq!(
            resolve_browser_profile("", app_data),
            app_data.join("default")
        );

        fs::remove_dir(app_data.join("default")).unwrap();
        assert_eq!(
            resolve_browser_profile("", app_data),
            app_data.join("browser-profile")
        );
    }

    #[test]
    fn new_install_starts_directly_at_version_one() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());

        let report = migrate_v0_to_v1(&paths, &FakeCredentialBackend::default()).unwrap();

        assert!(report.migrated);
        let config =
            crate::config::parse_config_content(&fs::read_to_string(&paths.config_path).unwrap())
                .unwrap();
        assert_eq!(config.schema_version, 1);
        assert!(config.llm_config.is_none());
        assert_eq!(
            config.browser_config.user_data_dir,
            paths.app_data_dir.join("browser-profile").to_string_lossy()
        );
    }
}
