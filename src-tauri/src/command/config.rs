use crate::browser::{self, BrowserEnvStatus};
use crate::command::base::CommandResult;
use crate::config::{self, AppRuntimeConfig};

#[tauri::command]
pub fn load_app_config(app_handle: tauri::AppHandle) -> CommandResult<AppRuntimeConfig> {
    match config::load_app_config_inner(app_handle) {
        Ok(cfg) => CommandResult::ok(cfg),
        Err(err) => CommandResult::err(err),
    }
}

/// 保存配置，并把**落盘后**的那份交回前端。
///
/// 保存路径上会做迁移、夹取、补生成人格种子，落盘内容与提交内容并不相同。
/// 前端拿这个返回值刷新自己手里的配置，才不会带着旧值继续编辑
#[tauri::command]
pub fn save_app_config(
    app_handle: tauri::AppHandle,
    config: AppRuntimeConfig,
) -> CommandResult<AppRuntimeConfig> {
    match config::save_app_config_inner(app_handle, config) {
        Ok(saved) => CommandResult::ok(saved),
        Err(err) => CommandResult::err(err),
    }
}

#[tauri::command]
pub fn import_app_config(
    app_handle: tauri::AppHandle,
    path: String,
) -> CommandResult<AppRuntimeConfig> {
    match config::import_app_config_inner(app_handle, &path) {
        Ok(cfg) => CommandResult::ok(cfg),
        Err(err) => CommandResult::err(err),
    }
}

#[tauri::command]
pub fn export_app_config(path: String, config: AppRuntimeConfig) -> CommandResult<()> {
    match config::export_app_config_inner(&path, config) {
        Ok(()) => CommandResult::ok(()),
        Err(err) => CommandResult::err(err),
    }
}

#[tauri::command]
pub fn parse_resume_pdf(path: String) -> CommandResult<String> {
    match config::parse_resume_pdf_inner(&path) {
        Ok(content) => CommandResult::ok(content),
        Err(err) => CommandResult::err(err),
    }
}

#[tauri::command]
pub fn check_browser_env(app_handle: tauri::AppHandle) -> CommandResult<BrowserEnvStatus> {
    match config::load_app_config_inner(app_handle) {
        Ok(cfg) => CommandResult::ok(browser::check_browser_env_status(&cfg.browser_config)),
        Err(err) => CommandResult::err(err),
    }
}
