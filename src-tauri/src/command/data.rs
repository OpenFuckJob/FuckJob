use crate::{
    command::base::CommandResult,
    config::{self, AppRuntimeConfig},
    error::AppError,
    logger,
    storage::{
        backup::{self, BackupPaths, RestoreResult},
        write_lock,
    },
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Debug, Serialize)]
pub struct ClearLogsResult {
    pub path: String,
    pub cleared: bool,
}

fn paths(app: &tauri::AppHandle) -> Result<BackupPaths, AppError> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::storage("无法定位应用数据目录").with_detail(e.to_string()))?;
    Ok(BackupPaths {
        config_file: config::config_path(app)?,
        data_dir,
    })
}

fn data_directory(app: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::storage("无法定位应用数据目录").with_detail(e.to_string()))
}

#[tauri::command]
pub fn get_data_directory(app_handle: tauri::AppHandle) -> CommandResult<String> {
    match data_directory(&app_handle) {
        Ok(path) => CommandResult::ok(path.display().to_string()),
        Err(e) => CommandResult::err(e),
    }
}

#[tauri::command]
pub fn export_data_backup(app_handle: tauri::AppHandle, path: String) -> CommandResult<()> {
    match paths(&app_handle).and_then(|paths| backup::export_backup(Path::new(&path), &paths)) {
        Ok(()) => CommandResult::ok(()),
        Err(e) => CommandResult::err(e),
    }
}

#[tauri::command]
pub fn restore_data_backup(
    app_handle: tauri::AppHandle,
    path: String,
) -> CommandResult<RestoreResult> {
    match paths(&app_handle).and_then(|paths| backup::restore_backup(Path::new(&path), &paths)) {
        Ok(result) => CommandResult::ok(result),
        Err(e) => CommandResult::err(e),
    }
}

#[tauri::command]
pub fn clear_logs() -> CommandResult<ClearLogsResult> {
    let _exclusive = write_lock();
    match logger::path().and_then(|path| logger::clear().map(|_| path)) {
        Ok(path) => CommandResult::ok(ClearLogsResult {
            path: path.display().to_string(),
            cleared: true,
        }),
        Err(e) => CommandResult::err(AppError::storage("无法清理日志").with_detail(e.to_string())),
    }
}

#[tauri::command]
pub fn reset_app_config(app_handle: tauri::AppHandle) -> CommandResult<AppRuntimeConfig> {
    let _exclusive = write_lock();
    let config = config::default_app_config();
    match config::save_app_config_unlocked(app_handle, config.clone()) {
        Ok(()) => CommandResult::ok(config),
        Err(e) => CommandResult::err(e),
    }
}
