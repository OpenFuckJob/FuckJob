use crate::command::base::CommandResult;
use crate::credential::{self, CredentialStatus};
use crate::error::AppError;
use crate::llm::service::LlmService;
use crate::llm::types::ConnectionReport;
use serde::Deserialize;
use std::time::Duration;

const MODEL_LIST_TIMEOUT_SECONDS: u64 = 15;

#[derive(Debug, Deserialize)]
struct ModelListResponse {
    data: Vec<ModelListItem>,
}

#[derive(Debug, Deserialize)]
struct ModelListItem {
    id: String,
}

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

async fn fetch_model_list(base_url: &str) -> Result<Vec<String>, AppError> {
    let base_url = base_url.trim().trim_end_matches('/');
    let parsed = reqwest::Url::parse(base_url).map_err(|error| {
        AppError::validation("大模型服务地址无效").with_detail(error.to_string())
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::validation("大模型服务地址仅支持 HTTP 或 HTTPS"));
    }

    let mut client_builder =
        reqwest::Client::builder().timeout(Duration::from_secs(MODEL_LIST_TIMEOUT_SECONDS));
    if parsed
        .host_str()
        .is_some_and(|host| host == "127.0.0.1" || host == "localhost")
    {
        client_builder = client_builder.no_proxy();
    }
    let client = client_builder.build().map_err(|error| {
        AppError::configuration("无法创建大模型客户端").with_detail(error.to_string())
    })?;

    let credential = credential::resolve()?;
    let mut request = client.get(format!("{base_url}/models"));
    if let Some(api_key) = credential.secret() {
        request = request.bearer_auth(api_key);
    }
    let response = request.send().await.map_err(|error| {
        AppError::network("获取模型列表失败，请检查服务地址和网络").with_detail(error.to_string())
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(
            AppError::provider(format!("获取模型列表失败（HTTP {status}）"))
                .with_detail(format!("GET {base_url}/models returned {status}")),
        );
    }

    let payload = response
        .json::<ModelListResponse>()
        .await
        .map_err(|error| {
            AppError::provider("模型列表响应格式不受支持").with_detail(error.to_string())
        })?;
    let mut models: Vec<String> = payload
        .data
        .into_iter()
        .map(|item| item.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    models.sort();
    models.dedup();
    if models.is_empty() {
        return Err(AppError::provider("服务未返回可用模型"));
    }
    Ok(models)
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
