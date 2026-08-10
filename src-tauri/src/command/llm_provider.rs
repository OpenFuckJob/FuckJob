use crate::command::base::CommandResult;
use crate::config::LlmProviderPreset;
use crate::credential::{self, CredentialStatus};
use crate::error::AppError;
use crate::llm::service::{normalize_provider_base_url, provider_requires_key, LlmService};
use crate::llm::types::ConnectionReport;
use rig::client::ModelListingClient;
use rig::model::ModelListingError;
use std::time::Duration;
use tokio::time::timeout;

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

const MODEL_LIST_TIMEOUT_SECONDS: u64 = 30;

async fn fetch_model_list(
    provider: LlmProviderPreset,
    base_url: &str,
) -> Result<Vec<String>, AppError> {
    validate_llm_base_url(base_url)?;
    let credential = credential::resolve()?;
    if provider_requires_key(&provider) && credential.secret().is_none() {
        return Err(AppError::credential("请先配置该大模型服务的 API Key"));
    }
    let api_key = credential
        .secret()
        .unwrap_or(if matches!(provider, LlmProviderPreset::Ollama) {
            ""
        } else {
            "noop"
        });
    let base_url = normalize_provider_base_url(&provider, base_url);

    let models = match provider {
        LlmProviderPreset::Anthropic => {
            let client = rig::providers::anthropic::Client::builder()
                .api_key(api_key)
                .base_url(&base_url)
                .build()
                .map_err(|error| {
                    AppError::configuration("无法创建大模型客户端").with_detail(error.to_string())
                })?;
            timeout(
                Duration::from_secs(MODEL_LIST_TIMEOUT_SECONDS),
                client.list_models(),
            )
            .await
            .map_err(|_| AppError::network("获取模型列表超时"))?
            .map_err(map_model_listing_error)?
        }
        LlmProviderPreset::DeepSeek => {
            let client = rig::providers::deepseek::Client::builder()
                .api_key(api_key)
                .base_url(&base_url)
                .build()
                .map_err(|error| {
                    AppError::configuration("无法创建大模型客户端").with_detail(error.to_string())
                })?;
            timeout(
                Duration::from_secs(MODEL_LIST_TIMEOUT_SECONDS),
                client.list_models(),
            )
            .await
            .map_err(|_| AppError::network("获取模型列表超时"))?
            .map_err(map_model_listing_error)?
        }
        LlmProviderPreset::OpenAi | LlmProviderPreset::OpenAiResponses => {
            let client = rig::providers::openai::Client::builder()
                .api_key(api_key)
                .base_url(&base_url)
                .build()
                .map_err(|error| {
                    AppError::configuration("无法创建大模型客户端").with_detail(error.to_string())
                })?;
            timeout(
                Duration::from_secs(MODEL_LIST_TIMEOUT_SECONDS),
                client.list_models(),
            )
            .await
            .map_err(|_| AppError::network("获取模型列表超时"))?
            .map_err(map_model_listing_error)?
        }
        LlmProviderPreset::MiniMax | LlmProviderPreset::Moonshot | LlmProviderPreset::ZAi => {
            return Err(AppError::provider(
                "该 provider 暂未提供模型列表接口，请手动填写模型名称",
            ));
        }
        LlmProviderPreset::Ollama => {
            let client = rig::providers::ollama::Client::builder()
                .api_key(api_key)
                .base_url(&base_url)
                .build()
                .map_err(|error| {
                    AppError::configuration("无法创建大模型客户端").with_detail(error.to_string())
                })?;
            timeout(
                Duration::from_secs(MODEL_LIST_TIMEOUT_SECONDS),
                client.list_models(),
            )
            .await
            .map_err(|_| AppError::network("获取模型列表超时"))?
            .map_err(map_model_listing_error)?
        }
        LlmProviderPreset::OpenRouter => {
            let client = rig::providers::openrouter::Client::builder()
                .api_key(api_key)
                .base_url(&base_url)
                .build()
                .map_err(|error| {
                    AppError::configuration("无法创建大模型客户端").with_detail(error.to_string())
                })?;
            timeout(
                Duration::from_secs(MODEL_LIST_TIMEOUT_SECONDS),
                client.list_models(),
            )
            .await
            .map_err(|_| AppError::network("获取模型列表超时"))?
            .map_err(map_model_listing_error)?
        }
        LlmProviderPreset::XiaomiMimo => {
            let client = rig::providers::xiaomimimo::Client::builder()
                .api_key(api_key)
                .base_url(&base_url)
                .build()
                .map_err(|error| {
                    AppError::configuration("无法创建大模型客户端").with_detail(error.to_string())
                })?;
            timeout(
                Duration::from_secs(MODEL_LIST_TIMEOUT_SECONDS),
                client.list_models(),
            )
            .await
            .map_err(|_| AppError::network("获取模型列表超时"))?
            .map_err(map_model_listing_error)?
        }
    };

    let mut names = models
        .into_iter()
        .map(|model| model.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    Ok(names)
}

fn map_model_listing_error(error: ModelListingError) -> AppError {
    match error {
        ModelListingError::ApiError {
            status_code,
            message,
        } => {
            let mut mapped = match status_code {
                401 | 403 => AppError::credential("大模型密钥无效或无权获取模型列表"),
                404 => AppError::provider("大模型服务未提供模型列表接口，请手动填写模型名称"),
                429 => AppError::provider("获取模型列表受限或账户额度不足"),
                _ => AppError::provider(format!("获取模型列表失败（HTTP {status_code}）")),
            };
            mapped = mapped.with_detail(format!("HTTP {status_code}; {message}"));
            mapped
        }
        ModelListingError::RequestError { message } => {
            AppError::network("无法连接大模型服务获取模型列表").with_detail(message)
        }
        ModelListingError::ParseError { message } => {
            AppError::provider("模型列表响应解析失败，请手动填写模型名称").with_detail(message)
        }
        ModelListingError::AuthError { message } => {
            AppError::credential("大模型密钥无效或无权获取模型列表").with_detail(message)
        }
        ModelListingError::RateLimitError { message } => {
            AppError::provider("获取模型列表受限或账户额度不足").with_detail(message)
        }
        ModelListingError::ServiceUnavailable { message } => {
            AppError::network("大模型服务暂不可用，无法获取模型列表").with_detail(message)
        }
        ModelListingError::UnknownError { message } => {
            AppError::provider("获取模型列表失败").with_detail(message)
        }
    }
}

#[tauri::command]
pub async fn list_llm_models(
    provider: LlmProviderPreset,
    base_url: String,
) -> CommandResult<Vec<String>> {
    match fetch_model_list(provider, &base_url).await {
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
