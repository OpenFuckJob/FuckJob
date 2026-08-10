use crate::command::base::CommandResult;
use crate::credential::{self, CredentialStatus};
use crate::error::AppError;
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

fn validate_llm_base_url(base_url: &str) -> Result<(), AppError> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        return Err(AppError::validation("大模型服务地址不能为空"));
    }
    let scheme_end = base_url
        .find("://")
        .ok_or_else(|| AppError::validation("大模型服务地址无效"))?;
    let scheme = &base_url[..scheme_end];
    if !matches!(scheme, "http" | "https") {
        return Err(AppError::validation("大模型服务地址仅支持 HTTP 或 HTTPS"));
    }
    Ok(())
}

async fn fetch_model_list(base_url: &str) -> Result<Vec<String>, AppError> {
    validate_llm_base_url(base_url)?;
    Err(AppError::provider(
        "当前 Rig 调用方式不支持自动获取模型列表，请手动填写模型名称后使用连接测试验证",
    ))
}

#[tauri::command]
pub async fn list_llm_models(base_url: String) -> CommandResult<Vec<String>> {
    match fetch_model_list(&base_url).await {
        Ok(models) => CommandResult::ok(models),
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
