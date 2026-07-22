use crate::command::base::CommandResult;
use crate::credential::{self, CredentialStatus};
use crate::llm::service::LlmService;
use crate::llm::types::ConnectionReport;

#[tauri::command]
pub fn get_llm_credential_status() -> CommandResult<CredentialStatus> {
    match credential::status() {
        Ok(status) => CommandResult::ok(status),
        Err(error) => CommandResult::err(error),
    }
}

#[tauri::command]
pub fn set_llm_api_key(api_key: String) -> CommandResult<CredentialStatus> {
    match credential::set(&api_key).and_then(|_| credential::status()) {
        Ok(status) => CommandResult::ok(status),
        Err(error) => CommandResult::err(error),
    }
}

#[tauri::command]
pub fn clear_llm_api_key() -> CommandResult<CredentialStatus> {
    match credential::delete().and_then(|_| credential::status()) {
        Ok(status) => CommandResult::ok(status),
        Err(error) => CommandResult::err(error),
    }
}

fn service(app_handle: tauri::AppHandle) -> Result<LlmService, crate::error::AppError> {
    let config = crate::config::load_app_config_inner(app_handle)?;
    let credential = credential::resolve()?;
    LlmService::from_runtime(&config, &credential)
}

#[tauri::command]
pub async fn test_llm_connection(app_handle: tauri::AppHandle) -> CommandResult<ConnectionReport> {
    match service(app_handle) {
        Ok(service) => match service.test_connection().await {
            Ok(v) => CommandResult::ok(v),
            Err(e) => CommandResult::err(e),
        },
        Err(e) => CommandResult::err(e),
    }
}
