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

#[tauri::command]
pub fn save_app_config(
    app_handle: tauri::AppHandle,
    config: AppRuntimeConfig,
) -> CommandResult<()> {
    match config::save_app_config_inner(app_handle, config) {
        Ok(()) => CommandResult::ok(()),
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
