use crate::{
    command::user_resumes::UserResumes,
    config::{
        default_app_config, parse_config_content, validate_and_normalize, CURRENT_SCHEMA_VERSION,
    },
    dao::model::{ChatMessageRecord, InterviewJobAnalysis, JobDetail},
    error::AppError,
    storage::{atomic::atomic_write, write_lock},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

pub const BACKUP_FORMAT_VERSION: u32 = 1;
const MANIFEST: &str = "manifest.json";
const CONFIG: &str = "config/app_config.yaml";
const DATA_FILES: &[(&str, &str)] = &[
    ("data/job_details.json", "job_details.json"),
    ("data/chat_messages.json", "chat_messages.json"),
    ("data/interview_analyses.json", "interview_analyses.json"),
    ("data/user_resumes.json", "user_resumes.json"),
];

#[derive(Debug, Clone)]
pub struct BackupPaths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format_version: u32,
    pub config_schema_version: u32,
    pub app_version: String,
    pub created_at: String,
    pub sha256: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct RestoreResult {
    pub restart_required: bool,
    pub message: String,
    pub recovery_backup_path: String,
}

fn allowed(name: &str) -> bool {
    name == MANIFEST || name == CONFIG || DATA_FILES.iter().any(|(entry, _)| *entry == name)
}

fn defaults(name: &str) -> Result<Vec<u8>, AppError> {
    if name == CONFIG {
        return serde_yaml::to_string(&default_app_config())
            .map(String::into_bytes)
            .map_err(|e| AppError::configuration("无法生成默认配置").with_detail(e.to_string()));
    }
    Ok(if name.ends_with("user_resumes.json") {
        b"{}".to_vec()
    } else {
        b"[]".to_vec()
    })
}

fn source_path(paths: &BackupPaths, name: &str) -> PathBuf {
    if name == CONFIG {
        paths.config_file.clone()
    } else {
        let file = DATA_FILES
            .iter()
            .find(|(entry, _)| *entry == name)
            .unwrap()
            .1;
        paths.data_dir.join("data").join(file)
    }
}

fn sanitized_config(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut value: serde_yaml::Value = serde_yaml::from_slice(bytes)
        .map_err(|e| AppError::configuration("应用配置格式无效").with_detail(e.to_string()))?;
    fn scrub(value: &mut serde_yaml::Value) {
        match value {
            serde_yaml::Value::Mapping(map) => {
                map.retain(|key, child| {
                    let sensitive = key.as_str().is_some_and(|key| {
                        matches!(
                            key.to_ascii_lowercase().as_str(),
                            "api_key" | "apikey" | "authorization" | "cookie" | "token"
                        )
                    });
                    if !sensitive {
                        scrub(child);
                    }
                    !sensitive
                });
            }
            serde_yaml::Value::Sequence(values) => values.iter_mut().for_each(scrub),
            _ => {}
        }
    }
    scrub(&mut value);
    serde_yaml::to_string(&value)
        .map(String::into_bytes)
        .map_err(|e| AppError::configuration("无法清理应用配置").with_detail(e.to_string()))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn export_backup(path: &Path, paths: &BackupPaths) -> Result<(), AppError> {
    let _exclusive = write_lock();
    export_backup_unlocked(path, paths)
}

fn export_backup_unlocked(path: &Path, paths: &BackupPaths) -> Result<(), AppError> {
    let mut payload = BTreeMap::new();
    for name in std::iter::once(CONFIG).chain(DATA_FILES.iter().map(|(name, _)| *name)) {
        let source = source_path(paths, name);
        let bytes = if source.exists() {
            fs::read(&source)
                .map_err(|e| AppError::storage("无法读取备份数据").with_detail(e.to_string()))?
        } else {
            defaults(name)?
        };
        payload.insert(
            name.to_string(),
            if name == CONFIG {
                sanitized_config(&bytes)?
            } else {
                bytes
            },
        );
    }
    let manifest = BackupManifest {
        format_version: BACKUP_FORMAT_VERSION,
        config_schema_version: CURRENT_SCHEMA_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: Utc::now().to_rfc3339(),
        sha256: payload
            .iter()
            .map(|(name, bytes)| (name.clone(), digest(bytes)))
            .collect(),
    };
    let manifest = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| AppError::storage("无法生成备份清单").with_detail(e.to_string()))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut zip = ZipWriter::new(temp.reopen()?);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file(MANIFEST, options).map_err(zip_error)?;
        zip.write_all(&manifest)?;
        for (name, bytes) in payload {
            zip.start_file(name, options).map_err(zip_error)?;
            zip.write_all(&bytes)?;
        }
        zip.finish().map_err(zip_error)?;
    }
    let bytes = fs::read(temp.path())?;
    atomic_write(path, &bytes)
}

fn zip_error(error: zip::result::ZipError) -> AppError {
    AppError::storage("备份 ZIP 操作失败").with_detail(error.to_string())
}

fn validate_entry(name: &str, unix_mode: Option<u32>) -> Result<(), AppError> {
    let path = Path::new(name);
    if name.contains('\\')
        || path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || !allowed(name)
        || unix_mode.is_some_and(|mode| mode & 0o170000 == 0o120000)
    {
        return Err(
            AppError::validation("备份包含不安全或不支持的文件").with_detail(name.to_string())
        );
    }
    Ok(())
}

fn read_and_validate(path: &Path) -> Result<HashMap<String, Vec<u8>>, AppError> {
    let file = File::open(path)
        .map_err(|e| AppError::storage("无法打开备份文件").with_detail(e.to_string()))?;
    let mut archive = ZipArchive::new(file).map_err(zip_error)?;
    let mut entries = HashMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(zip_error)?;
        let name = entry.name().to_string();
        validate_entry(&name, entry.unix_mode())?;
        if entries.contains_key(&name) {
            return Err(AppError::validation("备份包含重复文件").with_detail(name));
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        entries.insert(name, bytes);
    }
    let manifest_bytes = entries
        .get(MANIFEST)
        .ok_or_else(|| AppError::validation("备份缺少清单"))?;
    let manifest: BackupManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|e| AppError::validation("备份清单无效").with_detail(e.to_string()))?;
    if manifest.format_version > BACKUP_FORMAT_VERSION {
        return Err(AppError::validation("备份版本高于当前应用支持版本"));
    }
    if manifest.format_version == 0 {
        return Err(AppError::validation("备份版本不受支持"));
    }
    for name in std::iter::once(CONFIG).chain(DATA_FILES.iter().map(|(name, _)| *name)) {
        let bytes = entries
            .get(name)
            .ok_or_else(|| AppError::validation("备份缺少必需文件").with_detail(name))?;
        let expected = manifest
            .sha256
            .get(name)
            .ok_or_else(|| AppError::validation("备份清单缺少校验值").with_detail(name))?;
        if &digest(bytes) != expected {
            return Err(AppError::validation("备份文件校验失败").with_detail(name));
        }
        if name == CONFIG {
            validate_staged_config(bytes)?;
        } else {
            validate_typed_data(name, bytes)?;
        }
    }
    let clean_config = validate_staged_config(&entries[CONFIG])?;
    entries.insert(CONFIG.to_string(), clean_config);
    Ok(entries)
}

fn validate_typed_data(name: &str, bytes: &[u8]) -> Result<(), AppError> {
    let result = match name {
        "data/job_details.json" => serde_json::from_slice::<Vec<JobDetail>>(bytes).map(|_| ()),
        "data/chat_messages.json" => {
            serde_json::from_slice::<Vec<ChatMessageRecord>>(bytes).map(|_| ())
        }
        "data/interview_analyses.json" => {
            serde_json::from_slice::<Vec<InterviewJobAnalysis>>(bytes).map(|_| ())
        }
        "data/user_resumes.json" => serde_json::from_slice::<UserResumes>(bytes).map(|_| ()),
        _ => unreachable!("validated allowlist contains an unknown data file"),
    };
    result.map_err(|error| {
        AppError::validation("备份数据结构无效").with_detail(format!("{name}: {error}"))
    })
}

fn validate_staged_config(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| AppError::validation("配置编码无效").with_detail(error.to_string()))?;
    let raw: serde_yaml::Value = serde_yaml::from_str(text)
        .map_err(|error| AppError::validation("备份配置无效").with_detail(error.to_string()))?;
    if !raw.is_mapping() {
        return Err(AppError::validation("备份配置无效").with_detail("配置根节点必须是对象"));
    }
    let source_version = raw
        .get("schema_version")
        .map(|version| serde_yaml::from_value::<u32>(version.clone()))
        .transpose()
        .map_err(|error| AppError::validation("备份配置无效").with_detail(error.to_string()))?
        .unwrap_or(0);
    if source_version > CURRENT_SCHEMA_VERSION {
        return Err(AppError::validation("配置版本高于当前应用支持版本"));
    }

    let mut config = parse_config_content(text)
        .map_err(|error| AppError::validation("备份配置无效").with_detail(error))?;
    config.schema_version = CURRENT_SCHEMA_VERSION;
    validate_and_normalize(&mut config)
        .map_err(|error| AppError::validation("备份配置无效").with_detail(error))?;
    let normalized = serde_yaml::to_string(&config)
        .map_err(|error| AppError::validation("备份配置无效").with_detail(error.to_string()))?;
    sanitized_config(normalized.as_bytes()).map_err(|error| {
        AppError::validation("备份配置无效").with_detail(error.detail.unwrap_or(error.message))
    })
}

pub fn restore_backup(path: &Path, paths: &BackupPaths) -> Result<RestoreResult, AppError> {
    restore_backup_with(path, paths, |_| Ok(()))
}

fn restore_backup_with<F>(
    path: &Path,
    paths: &BackupPaths,
    before_replace: F,
) -> Result<RestoreResult, AppError>
where
    F: Fn(usize) -> Result<(), AppError>,
{
    let entries = read_and_validate(path)?;
    let _exclusive = write_lock();
    let recovery_dir = paths.data_dir.join("recovery");
    fs::create_dir_all(&recovery_dir)?;
    let recovery = recovery_dir.join(format!(
        "before-restore-{}.zip",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    ));
    export_backup_unlocked(&recovery, paths)?;

    let names: Vec<&str> = std::iter::once(CONFIG)
        .chain(DATA_FILES.iter().map(|(name, _)| *name))
        .collect();
    let originals: Vec<(PathBuf, Option<Vec<u8>>)> = names
        .iter()
        .map(|name| {
            let target = source_path(paths, name);
            let old = fs::read(&target).ok();
            (target, old)
        })
        .collect();
    for (index, name) in names.iter().enumerate() {
        if let Err(error) = before_replace(index)
            .and_then(|_| atomic_write(&source_path(paths, name), &entries[*name]))
        {
            for (target, old) in &originals {
                match old {
                    Some(bytes) => {
                        let _ = atomic_write(target, bytes);
                    }
                    None => {
                        let _ = fs::remove_file(target);
                    }
                }
            }
            return Err(AppError::storage("恢复失败，原数据已回滚").with_detail(error.to_string()));
        }
    }
    Ok(RestoreResult {
        restart_required: true,
        message: "数据已恢复，请手动重启应用".to_string(),
        recovery_backup_path: recovery.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn setup() -> (tempfile::TempDir, BackupPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = BackupPaths {
            config_file: dir.path().join("config/app_config.yaml"),
            data_dir: dir.path().join("app-data"),
        };
        atomic_write(
            &paths.config_file,
            serde_yaml::to_string(&default_app_config())
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        for (name, _) in DATA_FILES {
            atomic_write(
                &source_path(&paths, name),
                defaults(name).unwrap().as_slice(),
            )
            .unwrap();
        }
        (dir, paths)
    }

    fn rewrite_backup_entry(source: &Path, destination: &Path, name: &str, replacement: Vec<u8>) {
        let mut entries = read_and_validate(source).unwrap();
        entries.insert(name.to_string(), replacement);
        let mut manifest: BackupManifest = serde_json::from_slice(&entries[MANIFEST]).unwrap();
        manifest.sha256 = std::iter::once(CONFIG)
            .chain(DATA_FILES.iter().map(|(entry, _)| *entry))
            .map(|entry| (entry.to_string(), digest(&entries[entry])))
            .collect();
        entries.insert(MANIFEST.to_string(), serde_json::to_vec(&manifest).unwrap());

        let mut zip = ZipWriter::new(File::create(destination).unwrap());
        for (entry, bytes) in entries {
            zip.start_file(entry, SimpleFileOptions::default()).unwrap();
            zip.write_all(&bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    fn live_contents(paths: &BackupPaths) -> Vec<Vec<u8>> {
        std::iter::once(CONFIG)
            .chain(DATA_FILES.iter().map(|(name, _)| *name))
            .map(|name| fs::read(source_path(paths, name)).unwrap())
            .collect()
    }

    #[test]
    fn export_has_exact_allowlist_and_valid_checksums() {
        let (dir, paths) = setup();
        let backup = dir.path().join("backup.zip");
        export_backup(&backup, &paths).unwrap();
        let entries = read_and_validate(&backup).unwrap();
        assert_eq!(entries.len(), 6);
        assert!(entries.contains_key(MANIFEST));
        assert!(!entries
            .keys()
            .any(|name| name.contains("cookie") || name.contains("log")));
    }

    #[test]
    fn rejects_traversal_and_unlisted_entries() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["../escape", "logs/rpa.log"] {
            let path = dir.path().join(name.replace('/', "_"));
            let file = File::create(&path).unwrap();
            let mut zip = ZipWriter::new(file);
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
            assert_eq!(
                read_and_validate(&path).unwrap_err().code,
                crate::error::AppErrorCode::Validation
            );
        }
    }

    #[test]
    fn corrupt_checksum_is_rejected_before_mutation() {
        let (dir, paths) = setup();
        let backup = dir.path().join("backup.zip");
        export_backup(&backup, &paths).unwrap();
        let mut entries = read_and_validate(&backup).unwrap();
        entries.get_mut(CONFIG).unwrap().push(b' ');
        // rebuilding with the original manifest intentionally makes the checksum invalid
        let bad = dir.path().join("bad.zip");
        let mut zip = ZipWriter::new(File::create(&bad).unwrap());
        for (name, bytes) in entries {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(&bytes).unwrap();
        }
        zip.finish().unwrap();
        assert!(restore_backup(&bad, &paths).is_err());
    }

    #[test]
    fn future_version_is_rejected() {
        let (dir, paths) = setup();
        let backup = dir.path().join("backup.zip");
        export_backup(&backup, &paths).unwrap();
        let mut entries = read_and_validate(&backup).unwrap();
        let mut manifest: BackupManifest = serde_json::from_slice(&entries[MANIFEST]).unwrap();
        manifest.format_version += 1;
        entries.insert(MANIFEST.into(), serde_json::to_vec(&manifest).unwrap());
        let bad = dir.path().join("future.zip");
        let mut zip = ZipWriter::new(File::create(&bad).unwrap());
        for (name, bytes) in entries {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(&bytes).unwrap();
        }
        zip.finish().unwrap();
        assert!(restore_backup(&bad, &paths)
            .unwrap_err()
            .message
            .contains("版本"));
    }

    #[test]
    fn typed_data_validation_rejects_wrong_shapes_and_malformed_records_before_mutation() {
        for (name, invalid) in [
            ("data/job_details.json", br#"{}"#.as_slice()),
            ("data/user_resumes.json", br#"[]"#.as_slice()),
            ("data/chat_messages.json", br#"[{}]"#.as_slice()),
            (
                "data/interview_analyses.json",
                br#"[{"job_id":1}]"#.as_slice(),
            ),
        ] {
            let (dir, paths) = setup();
            let backup = dir.path().join("backup.zip");
            export_backup(&backup, &paths).unwrap();
            let bad = dir.path().join("bad.zip");
            rewrite_backup_entry(&backup, &bad, name, invalid.to_vec());
            let before = live_contents(&paths);

            let error = restore_backup(&bad, &paths).unwrap_err();

            assert_eq!(error.code, crate::error::AppErrorCode::Validation);
            assert_eq!(live_contents(&paths), before);
            assert!(!paths.data_dir.join("recovery").exists());
        }
    }

    #[test]
    fn future_config_is_rejected_before_mutation() {
        let (dir, paths) = setup();
        let backup = dir.path().join("backup.zip");
        export_backup(&backup, &paths).unwrap();
        let bad = dir.path().join("future-config.zip");
        rewrite_backup_entry(
            &backup,
            &bad,
            CONFIG,
            b"schema_version: 999\nllm_config: null\n".to_vec(),
        );
        let before = live_contents(&paths);

        let error = restore_backup(&bad, &paths).unwrap_err();

        assert_eq!(error.code, crate::error::AppErrorCode::Validation);
        assert_eq!(live_contents(&paths), before);
        assert!(!paths.data_dir.join("recovery").exists());
    }

    #[test]
    fn legacy_config_is_normalized_and_sanitized_in_staging() {
        let (dir, paths) = setup();
        let backup = dir.path().join("backup.zip");
        export_backup(&backup, &paths).unwrap();
        let legacy = br#"
llm_config:
  base_url: http://localhost:11434/v1
  model: llama3
  api_key: never-restore-me
"#;
        let staged = dir.path().join("legacy.zip");
        rewrite_backup_entry(&backup, &staged, CONFIG, legacy.to_vec());

        restore_backup(&staged, &paths).unwrap();

        let restored = fs::read_to_string(&paths.config_file).unwrap();
        let config = parse_config_content(&restored).unwrap();
        assert_eq!(config.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(!restored.contains("api_key"));
        assert!(!restored.contains("never-restore-me"));
    }

    #[test]
    fn restore_replaces_all_files_and_rolls_back_every_file_on_failure() {
        let (dir, paths) = setup();
        let backup = dir.path().join("backup.zip");
        export_backup(&backup, &paths).unwrap();
        for (name, _) in DATA_FILES {
            atomic_write(&source_path(&paths, name), b"[1]").unwrap();
        }
        restore_backup(&backup, &paths).unwrap();
        for (name, _) in DATA_FILES {
            assert_eq!(
                fs::read(source_path(&paths, name)).unwrap(),
                defaults(name).unwrap()
            );
        }
        for (name, _) in DATA_FILES {
            atomic_write(&source_path(&paths, name), b"[2]").unwrap();
        }
        let calls = AtomicUsize::new(0);
        assert!(restore_backup_with(&backup, &paths, |_| {
            if calls.fetch_add(1, Ordering::SeqCst) == 2 {
                Err(AppError::storage("injected"))
            } else {
                Ok(())
            }
        })
        .is_err());
        for (name, _) in DATA_FILES {
            assert_eq!(fs::read(source_path(&paths, name)).unwrap(), b"[2]");
        }
    }
}
