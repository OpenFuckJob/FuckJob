use crate::command::base::CommandResult;
use crate::dao::{
    analysis_dao, chat_message_dao, job_detail_dao, model::*, profile_snapshot_dao, sync_state_dao,
};
use crate::storage::atomic::atomic_write;
use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tauri::Manager;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStats {
    pub job_details_count: usize,
    pub chat_messages_count: usize,
    pub interview_analyses_count: usize,
    pub user_resumes_count: usize,
    #[serde(default)]
    pub job_profile_snapshots_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportManifest {
    pub version: String,
    pub app_version: String,
    pub exported_at: String,
    pub stats: BackupStats,
    pub includes_config: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResultStats {
    pub job_details_added: usize,
    pub job_details_updated: usize,
    pub chat_messages_added: usize,
    pub interview_analyses_added: usize,
    pub interview_analyses_updated: usize,
    pub user_resumes_added: usize,
    pub config_imported: bool,
    #[serde(default)]
    pub job_profile_snapshots_added: usize,
    #[serde(default)]
    pub job_profile_snapshots_updated: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportStrategy {
    Merge,
    Overwrite,
}

fn user_resumes_path(data_dir: &Path) -> PathBuf {
    data_dir.join("data").join("user_resumes.json")
}

#[tauri::command]
pub fn export_data_bundle(
    app_handle: tauri::AppHandle,
    path: String,
    include_config: bool,
) -> CommandResult<ExportManifest> {
    match export_data_bundle_inner(&app_handle, &path, include_config) {
        Ok(manifest) => CommandResult::ok(manifest),
        Err(err) => CommandResult::err(err),
    }
}

fn export_data_bundle_inner(
    app_handle: &tauri::AppHandle,
    target_path: &str,
    include_config: bool,
) -> Result<ExportManifest> {
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("无法获取 app_data_dir: {}", e))?;

    let jobs = job_detail_dao::list().unwrap_or_default();
    let chats = chat_message_dao::list().unwrap_or_default();
    let analyses = analysis_dao::list().unwrap_or_default();
    let profile_snapshots = profile_snapshot_dao::list().unwrap_or_default();
    let sync_state = sync_state_dao::export_state();

    let user_resumes_file = user_resumes_path(&data_dir);
    let user_resumes_content = if user_resumes_file.exists() {
        fs::read_to_string(&user_resumes_file).unwrap_or_else(|_| "{}".to_string())
    } else {
        "{}".to_string()
    };
    let user_resumes_count = serde_json::from_str::<serde_json::Value>(&user_resumes_content)
        .ok()
        .and_then(|v| v.as_object().map(|o| o.len()))
        .unwrap_or(0);

    let config_content = if include_config {
        match crate::config::config_path(app_handle) {
            Ok(cfg_path) if cfg_path.exists() => Some(
                fs::read_to_string(&cfg_path)
                    .with_context(|| format!("读取应用配置失败：{}", cfg_path.display()))?,
            ),
            _ => None,
        }
    } else {
        None
    };

    let stats = BackupStats {
        job_details_count: jobs.len(),
        chat_messages_count: chats.len(),
        interview_analyses_count: analyses.len(),
        user_resumes_count,
        job_profile_snapshots_count: profile_snapshots.len(),
    };

    let manifest = ExportManifest {
        version: "1.0".to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        stats,
        includes_config: config_content.is_some(),
    };

    let file = File::create(target_path).context("无法创建导出文件")?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("manifest.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;

    zip.start_file("data/job_details.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&jobs)?.as_bytes())?;

    zip.start_file("data/chat_messages.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&chats)?.as_bytes())?;

    zip.start_file("data/interview_analyses.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&analyses)?.as_bytes())?;

    zip.start_file("data/job_profile_snapshots.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&profile_snapshots)?.as_bytes())?;

    zip.start_file("data/sync_state.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&sync_state)?.as_bytes())?;

    zip.start_file("data/user_resumes.json", options)?;
    zip.write_all(user_resumes_content.as_bytes())?;

    if let Some(cfg_content) = config_content {
        zip.start_file("config.yaml", options)?;
        zip.write_all(cfg_content.as_bytes())?;
    }

    zip.finish()?;
    Ok(manifest)
}

#[tauri::command]
pub fn inspect_data_bundle(path: String) -> CommandResult<ExportManifest> {
    match inspect_data_bundle_inner(&path) {
        Ok(manifest) => CommandResult::ok(manifest),
        Err(err) => CommandResult::err(err),
    }
}

fn inspect_data_bundle_inner(archive_path: &str) -> Result<ExportManifest> {
    let file = File::open(archive_path).context("无法打开备份归档文件")?;
    let mut zip = ZipArchive::new(file).context("无效的备份压缩包格式")?;

    let mut manifest_file = zip
        .by_name("manifest.json")
        .context("备份包缺少 manifest.json")?;
    let mut content = String::new();
    manifest_file.read_to_string(&mut content)?;

    let manifest: ExportManifest = serde_json::from_str(&content).context("解析备份元数据失败")?;
    Ok(manifest)
}

#[tauri::command]
pub fn import_data_bundle(
    app_handle: tauri::AppHandle,
    path: String,
    strategy: ImportStrategy,
    import_config: bool,
) -> CommandResult<ImportResultStats> {
    match import_data_bundle_inner(&app_handle, &path, strategy, import_config) {
        Ok(stats) => CommandResult::ok(stats),
        Err(err) => CommandResult::err(err),
    }
}

fn read_optional_zip_entry(zip: &mut ZipArchive<File>, name: &str) -> Result<Option<String>> {
    match zip.by_name(name) {
        Ok(mut file) => {
            let mut content = String::new();
            file.read_to_string(&mut content)
                .with_context(|| format!("读取备份条目 {name} 失败"))?;
            Ok(Some(content))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(error) => Err(error).with_context(|| format!("打开备份条目 {name} 失败")),
    }
}

fn import_data_bundle_inner(
    app_handle: &tauri::AppHandle,
    archive_path: &str,
    strategy: ImportStrategy,
    import_config: bool,
) -> Result<ImportResultStats> {
    let file = File::open(archive_path).context("无法打开备份归档文件")?;
    let mut zip = ZipArchive::new(file).context("无效的备份压缩包格式")?;

    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("无法获取 app_data_dir: {}", e))?;

    // 先完成所有条目的解析与校验，任一文件损坏即立刻中止，避免半覆盖本地数据
    let incoming_jobs = read_optional_zip_entry(&mut zip, "data/job_details.json")?
        .map(|content| {
            serde_json::from_str::<Vec<JobDetail>>(&content)
                .context("解析备份中的岗位数据 (job_details.json) 失败")
        })
        .transpose()?;
    let incoming_chats = read_optional_zip_entry(&mut zip, "data/chat_messages.json")?
        .map(|content| {
            serde_json::from_str::<Vec<ChatMessageRecord>>(&content)
                .context("解析备份中的聊天记录 (chat_messages.json) 失败")
        })
        .transpose()?;
    let incoming_analyses = read_optional_zip_entry(&mut zip, "data/interview_analyses.json")?
        .map(|content| {
            serde_json::from_str::<Vec<InterviewJobAnalysis>>(&content)
                .context("解析备份中的岗位分析 (interview_analyses.json) 失败")
        })
        .transpose()?;
    let incoming_snapshots = read_optional_zip_entry(&mut zip, "data/job_profile_snapshots.json")?
        .map(|content| {
            serde_json::from_str::<Vec<JobProfileSnapshot>>(&content)
                .context("解析备份中的求职方案快照 (job_profile_snapshots.json) 失败")
        })
        .transpose()?;
    let incoming_sync_state = read_optional_zip_entry(&mut zip, "data/sync_state.json")?
        .map(|content| {
            serde_json::from_str::<sync_state_dao::SyncStateFile>(&content)
                .context("解析备份中的同步状态 (sync_state.json) 失败")
        })
        .transpose()?;
    let incoming_resumes = read_optional_zip_entry(&mut zip, "data/user_resumes.json")?
        .map(|content| {
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&content)
                .context("解析备份中的简历数据 (user_resumes.json) 失败")
        })
        .transpose()?;
    let incoming_config = if import_config {
        read_optional_zip_entry(&mut zip, "config.yaml")?
            .map(|content| {
                // 统一走应用自己的兼容解析器。直接反序列化 v0-v2 备份会得到空的
                // job_profiles，随后被 v3 校验拒绝，也无法把旧五块策略迁移为默认方案。
                crate::config::parse_config_content(&content)
                    .map_err(|error| anyhow::anyhow!("解析备份中的应用配置 (config.yaml) 失败：{error}"))
            })
            .transpose()?
    } else {
        None
    };

    let mut result_stats = ImportResultStats {
        job_details_added: 0,
        job_details_updated: 0,
        chat_messages_added: 0,
        interview_analyses_added: 0,
        interview_analyses_updated: 0,
        user_resumes_added: 0,
        config_imported: false,
        job_profile_snapshots_added: 0,
        job_profile_snapshots_updated: 0,
    };

    // 1. 导入/合并 JobDetails — 批量操作，只读写各一次
    if let Some(incoming_jobs) = incoming_jobs {
        match strategy {
            ImportStrategy::Overwrite => {
                let count = incoming_jobs.len();
                job_detail_dao::replace_all(incoming_jobs)?;
                result_stats.job_details_added = count;
            }
            ImportStrategy::Merge => {
                let batch = job_detail_dao::batch_upsert(incoming_jobs, |existing, incoming| {
                    incoming.updated_at >= existing.updated_at
                })?;
                result_stats.job_details_added = batch.added;
                result_stats.job_details_updated = batch.updated;
            }
        }
    }

    // 2. 导入/合并 ChatMessages — 批量插入不存在的记录
    if let Some(incoming_chats) = incoming_chats {
        match strategy {
            ImportStrategy::Overwrite => {
                let count = incoming_chats.len();
                chat_message_dao::replace_all(incoming_chats)?;
                result_stats.chat_messages_added = count;
            }
            ImportStrategy::Merge => {
                let added = chat_message_dao::batch_insert_new(incoming_chats)?;
                result_stats.chat_messages_added = added;
            }
        }
    }

    // 3. 导入/合并 InterviewAnalyses — 批量操作
    if let Some(incoming_analyses) = incoming_analyses {
        match strategy {
            ImportStrategy::Overwrite => {
                let count = incoming_analyses.len();
                analysis_dao::replace_all(incoming_analyses)?;
                result_stats.interview_analyses_added = count;
            }
            ImportStrategy::Merge => {
                let batch =
                    analysis_dao::batch_upsert(incoming_analyses, |existing, incoming| {
                        incoming.analyzed_at >= existing.analyzed_at
                    })?;
                result_stats.interview_analyses_added = batch.added;
                result_stats.interview_analyses_updated = batch.updated;
            }
        }
    }

    // 4. 导入不可变求职方案快照。老备份不含此文件，保持向后兼容。
    if let Some(incoming) = incoming_snapshots {
        match strategy {
            ImportStrategy::Overwrite => {
                result_stats.job_profile_snapshots_added = incoming.len();
                profile_snapshot_dao::replace_all(incoming)?;
            }
            ImportStrategy::Merge => {
                let batch = profile_snapshot_dao::batch_upsert(incoming)?;
                result_stats.job_profile_snapshots_added = batch.added;
                result_stats.job_profile_snapshots_updated = batch.updated;
            }
        }
    }

    // 5. 导入/合并同步状态（老版本备份不含 sync_state.json，覆盖恢复时需清空本地陈旧信号）
    match incoming_sync_state {
        Some(incoming_state) => match strategy {
            ImportStrategy::Overwrite => {
                sync_state_dao::replace_state(incoming_state)?;
            }
            ImportStrategy::Merge => {
                sync_state_dao::merge_state(incoming_state)?;
            }
        },
        None if matches!(strategy, ImportStrategy::Overwrite) => {
            sync_state_dao::replace_state(sync_state_dao::SyncStateFile::default())?;
        }
        None => {}
    }

    // 6. 导入/合并 UserResumes
    if let Some(incoming_resumes) = incoming_resumes {
        let target_file = user_resumes_path(&data_dir);
        let mut local_resumes: serde_json::Map<String, serde_json::Value> =
            if matches!(strategy, ImportStrategy::Overwrite) {
                serde_json::Map::new()
            } else if target_file.exists() {
                let text = fs::read_to_string(&target_file).unwrap_or_else(|_| "{}".to_string());
                serde_json::from_str(&text).unwrap_or_default()
            } else {
                serde_json::Map::new()
            };

        for (key, val) in incoming_resumes {
            if !local_resumes.contains_key(&key)
                || matches!(strategy, ImportStrategy::Overwrite)
            {
                local_resumes.insert(key, val);
                result_stats.user_resumes_added += 1;
            }
        }
        let serialized = serde_json::to_string_pretty(&local_resumes)?;
        atomic_write(&target_file, serialized.as_bytes()).context("写入简历数据失败")?;
    }

    // 7. 可选导入应用配置（跨设备恢复时自动修复缺失或异机浏览器路径）
    if let Some(mut cfg) = incoming_config {
        crate::config::ensure_browser_user_data_dir(app_handle, &mut cfg)
            .context("修复浏览器数据目录失败")?;
        crate::config::ensure_browser_exe_path(&mut cfg);
        crate::config::save_app_config_inner(app_handle.clone(), cfg)
            .context("导入应用配置失败")?;
        result_stats.config_imported = true;
    }

    Ok(result_stats)
}
