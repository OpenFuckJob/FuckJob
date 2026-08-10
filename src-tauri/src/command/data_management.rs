use crate::command::base::CommandResult;
use crate::dao::{analysis_dao, chat_message_dao, job_detail_dao, model::*};
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

    let stats = BackupStats {
        job_details_count: jobs.len(),
        chat_messages_count: chats.len(),
        interview_analyses_count: analyses.len(),
        user_resumes_count,
    };

    let manifest = ExportManifest {
        version: "1.0".to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        stats,
        includes_config: include_config,
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

    zip.start_file("data/user_resumes.json", options)?;
    zip.write_all(user_resumes_content.as_bytes())?;

    if include_config {
        if let Ok(cfg_path) = crate::config::config_path(app_handle) {
            if cfg_path.exists() {
                if let Ok(cfg_content) = fs::read_to_string(&cfg_path) {
                    zip.start_file("config.yaml", options)?;
                    zip.write_all(cfg_content.as_bytes())?;
                }
            }
        }
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

    let mut result_stats = ImportResultStats {
        job_details_added: 0,
        job_details_updated: 0,
        chat_messages_added: 0,
        interview_analyses_added: 0,
        interview_analyses_updated: 0,
        user_resumes_added: 0,
        config_imported: false,
    };

    // 1. 导入/合并 JobDetails — 批量操作，只读写各一次
    if let Ok(mut file) = zip.by_name("data/job_details.json") {
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        if let Ok(incoming_jobs) = serde_json::from_str::<Vec<JobDetail>>(&content) {
            match strategy {
                ImportStrategy::Overwrite => {
                    let count = incoming_jobs.len();
                    job_detail_dao::replace_all(incoming_jobs)?;
                    result_stats.job_details_added = count;
                }
                ImportStrategy::Merge => {
                    let batch =
                        job_detail_dao::batch_upsert(incoming_jobs, |existing, incoming| {
                            incoming.updated_at >= existing.updated_at
                        })?;
                    result_stats.job_details_added = batch.added;
                    result_stats.job_details_updated = batch.updated;
                }
            }
        }
    }

    // 2. 导入/合并 ChatMessages — 批量插入不存在的记录
    if let Ok(mut file) = zip.by_name("data/chat_messages.json") {
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        if let Ok(incoming_chats) = serde_json::from_str::<Vec<ChatMessageRecord>>(&content) {
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
    }

    // 3. 导入/合并 InterviewAnalyses — 批量操作
    if let Ok(mut file) = zip.by_name("data/interview_analyses.json") {
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        if let Ok(incoming_analyses) = serde_json::from_str::<Vec<InterviewJobAnalysis>>(&content) {
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
    }

    // 4. 导入/合并 UserResumes
    if let Ok(mut file) = zip.by_name("data/user_resumes.json") {
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        if let Ok(incoming_resumes) =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&content)
        {
            let target_file = user_resumes_path(&data_dir);
            let mut local_resumes: serde_json::Map<String, serde_json::Value> = if target_file
                .exists()
            {
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
            let _ = fs::create_dir_all(target_file.parent().unwrap());
            let _ = fs::write(target_file, serde_json::to_string_pretty(&local_resumes)?);
        }
    }

    // 5. 可选导入应用配置
    if import_config {
        if let Ok(mut file) = zip.by_name("config.yaml") {
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            if let Ok(cfg) = serde_yaml::from_str::<crate::config::AppRuntimeConfig>(&content) {
                if crate::config::save_app_config_inner(app_handle.clone(), cfg).is_ok() {
                    result_stats.config_imported = true;
                }
            }
        }
    }

    Ok(result_stats)
}
