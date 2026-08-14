use crate::command::base::CommandResult;
use crate::config::{LlmChainLink, LlmProviderPreset, PRIMARY_LLM_ENTRY_ID};
use crate::credential::{self, CredentialStatus, EffectiveCredentialSource, ResolvedCredential};
use crate::error::AppError;
use crate::llm::service::{normalize_provider_base_url, provider_requires_key, LlmService};
use crate::llm::types::ConnectionReport;
use rig::client::ModelListingClient;
use rig::model::ModelListingError;
use serde::Serialize;
use std::time::Duration;
use tokio::time::timeout;

/// 批量查询降级链密钥状态时的单条结果。
/// 比 `CredentialStatus` 多带一个 `entry_id`，让配置页可以直接按服务对号入座。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmEntryCredentialStatus {
    pub entry_id: String,
    pub configured: bool,
    pub source: EffectiveCredentialSource,
}

/// 把内部 `Result` 收敛成命令层返回值，避免每个命令都重复 match
fn to_command_result<T>(result: Result<T, AppError>) -> CommandResult<T> {
    match result {
        Ok(value) => CommandResult::ok(value),
        Err(error) => CommandResult::err(error),
    }
}

// ================================
// 主用服务（沿用旧调用点）
// ================================

#[tauri::command]
pub fn get_llm_credential_status() -> CommandResult<CredentialStatus> {
    to_command_result(credential::status_for_entry(PRIMARY_LLM_ENTRY_ID))
}

#[tauri::command]
pub fn set_llm_api_key(api_key: String) -> CommandResult<CredentialStatus> {
    to_command_result(set_entry_api_key(PRIMARY_LLM_ENTRY_ID, &api_key))
}

#[tauri::command]
pub fn clear_llm_api_key() -> CommandResult<CredentialStatus> {
    to_command_result(clear_entry_api_key(PRIMARY_LLM_ENTRY_ID))
}

// ================================
// 按服务标识操作降级链
// ================================

fn set_entry_api_key(entry_id: &str, api_key: &str) -> Result<CredentialStatus, AppError> {
    credential::set_for_entry(entry_id, api_key)?;
    credential::status_for_entry(entry_id)
}

fn clear_entry_api_key(entry_id: &str) -> Result<CredentialStatus, AppError> {
    credential::delete_for_entry(entry_id)?;
    credential::status_for_entry(entry_id)
}

/// 查询单个服务的密钥状态
#[tauri::command]
pub fn get_llm_credential_status_for(entry_id: String) -> CommandResult<CredentialStatus> {
    to_command_result(credential::status_for_entry(&entry_id))
}

/// 保存单个服务的密钥，返回保存后的状态
#[tauri::command]
pub fn set_llm_api_key_for(entry_id: String, api_key: String) -> CommandResult<CredentialStatus> {
    to_command_result(set_entry_api_key(&entry_id, &api_key))
}

/// 清除单个服务的密钥，返回清除后的状态
#[tauri::command]
pub fn clear_llm_api_key_for(entry_id: String) -> CommandResult<CredentialStatus> {
    to_command_result(clear_entry_api_key(&entry_id))
}

/// 互换两个服务已保存的密钥，用于降级链上调整主用/备用顺序。
///
/// 密钥明文不允许离开 Rust，因此搬运必须在后端整体完成，
/// 前端只负责发起并接收交换后的状态。
#[tauri::command]
pub fn swap_llm_credentials(
    entry_a: String,
    entry_b: String,
) -> CommandResult<Vec<LlmEntryCredentialStatus>> {
    let result = credential::swap_entries(&entry_a, &entry_b).and_then(|()| {
        collect_entry_credential_status(
            &[entry_a.clone(), entry_b.clone()],
            credential::status_for_entry,
        )
    });
    to_command_result(result)
}

/// 批量查询多个服务的密钥状态，供配置页列表一次性渲染。
/// 返回结果与入参等长且顺序一致，前端可以直接按下标对齐。
#[tauri::command]
pub fn list_llm_credential_status(
    entry_ids: Vec<String>,
) -> CommandResult<Vec<LlmEntryCredentialStatus>> {
    to_command_result(collect_entry_credential_status(
        &entry_ids,
        credential::status_for_entry,
    ))
}

fn collect_entry_credential_status<F>(
    entry_ids: &[String],
    mut status_of: F,
) -> Result<Vec<LlmEntryCredentialStatus>, AppError>
where
    F: FnMut(&str) -> Result<CredentialStatus, AppError>,
{
    entry_ids
        .iter()
        .map(|entry_id| {
            let status = status_of(entry_id)?;
            Ok(LlmEntryCredentialStatus {
                entry_id: entry_id.clone(),
                configured: status.configured,
                source: status.source,
            })
        })
        .collect()
}

/// 在降级链中按标识定位一个服务。
/// 链由已保存的配置推导而来，所以「找不到」既可能是标识写错，
/// 也可能是用户刚在界面上填完还没保存，错误文案需要同时覆盖这两种情况。
fn find_chain_link<'a>(
    chain: &'a [LlmChainLink],
    entry_id: &str,
) -> Result<&'a LlmChainLink, AppError> {
    chain
        .iter()
        .find(|link| link.id == entry_id)
        .ok_or_else(|| {
            AppError::configuration(
                "未找到该大模型服务，请先保存配置后再测试；未保存或已停用的服务无法测试",
            )
        })
}

/// 用已保存的配置构建降级链中某个服务的实例
/// 连接测试是全项目**唯一**不走 [`crate::agent`] 的模型调用，这是刻意的：
/// 它要验证的是「这一个 provider 通不通」，而 Agent 循环带降级链——
/// 主用服务连不上时会顶上备用服务并返回成功，等于把测试本身废掉。
/// 探活也不是「任务」，没有提示词、没有结构化输出、不需要返工。
fn entry_service(app_handle: tauri::AppHandle, entry_id: &str) -> Result<LlmService, AppError> {
    let (link, credential) = resolve_chain_entry(app_handle, entry_id)?;
    LlmService::from_chain_link(&link, &credential)
}

/// 读取已保存的配置，定位目标服务并取出它自己的密钥
fn resolve_chain_entry(
    app_handle: tauri::AppHandle,
    entry_id: &str,
) -> Result<(LlmChainLink, ResolvedCredential), AppError> {
    let config = crate::config::load_app_config_inner(app_handle)?;
    let chain = config.llm_chain();
    let link = find_chain_link(&chain, entry_id)?.clone();
    let credential = credential::resolve_for_entry(&link.id)?;
    Ok((link, credential))
}

/// 用当前已保存的配置，测试降级链中某个服务的连通性
#[tauri::command]
pub async fn test_llm_entry_connection(
    app_handle: tauri::AppHandle,
    entry_id: String,
) -> CommandResult<ConnectionReport> {
    match entry_service(app_handle, &entry_id) {
        Ok(service) => to_command_result(service.test_connection().await),
        Err(error) => CommandResult::err(error),
    }
}

/// 列出降级链中某个服务可用的模型
#[tauri::command]
pub async fn list_llm_models_for(
    app_handle: tauri::AppHandle,
    entry_id: String,
) -> CommandResult<Vec<String>> {
    let (link, credential) = match resolve_chain_entry(app_handle, &entry_id) {
        Ok(resolved) => resolved,
        Err(error) => return CommandResult::err(error),
    };
    to_command_result(
        fetch_model_list_with_credential(link.provider, &link.base_url, &credential).await,
    )
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
    let credential = credential::resolve_for_entry(PRIMARY_LLM_ENTRY_ID)?;
    fetch_model_list_with_credential(provider, base_url, &credential).await
}

/// 用指定服务自己的密钥拉取模型列表。
/// 降级链上每个服务的密钥独立存储，所以这里由调用方传入已解析的凭证。
async fn fetch_model_list_with_credential(
    provider: LlmProviderPreset,
    base_url: &str,
    credential: &ResolvedCredential,
) -> Result<Vec<String>, AppError> {
    validate_llm_base_url(base_url)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppErrorCode;

    fn link(id: &str, model: &str) -> LlmChainLink {
        LlmChainLink {
            id: id.to_string(),
            label: None,
            provider: LlmProviderPreset::OpenAi,
            base_url: "https://llm.example.test/v1".to_string(),
            model: model.to_string(),
        }
    }

    fn chain() -> Vec<LlmChainLink> {
        vec![
            link(PRIMARY_LLM_ENTRY_ID, "gpt-4o"),
            link("backup-a", "qwen-max"),
            link("backup-b", "gpt-4o-mini"),
        ]
    }

    #[test]
    fn find_chain_link_hits_primary_service() {
        let chain = chain();

        let found = find_chain_link(&chain, PRIMARY_LLM_ENTRY_ID).unwrap();

        assert_eq!(found.id, PRIMARY_LLM_ENTRY_ID);
        assert_eq!(found.model, "gpt-4o");
        assert!(found.is_primary());
    }

    #[test]
    fn find_chain_link_hits_fallback_service() {
        let chain = chain();

        let found = find_chain_link(&chain, "backup-b").unwrap();

        assert_eq!(found.id, "backup-b");
        assert_eq!(found.model, "gpt-4o-mini");
        assert!(!found.is_primary());
    }

    #[test]
    fn find_chain_link_reports_unsaved_service_clearly() {
        let chain = chain();

        // 界面上刚填完还没保存就点测试，是最常见的误用，错误文案必须点明这一点
        let error = find_chain_link(&chain, "backup-unsaved").unwrap_err();

        assert_eq!(error.code, AppErrorCode::Configuration);
        assert!(error.message.contains("未找到该大模型服务"));
        assert!(error.message.contains("保存"));
    }

    #[test]
    fn find_chain_link_on_empty_chain_reports_the_same_error() {
        let error = find_chain_link(&[], PRIMARY_LLM_ENTRY_ID).unwrap_err();

        assert_eq!(error.code, AppErrorCode::Configuration);
        assert!(error.message.contains("未找到该大模型服务"));
    }

    #[test]
    fn batch_status_keeps_input_length_and_order() {
        let entry_ids = vec![
            PRIMARY_LLM_ENTRY_ID.to_string(),
            "backup-b".to_string(),
            "backup-a".to_string(),
        ];

        let statuses = collect_entry_credential_status(&entry_ids, |entry_id| {
            Ok(CredentialStatus {
                configured: entry_id != "backup-a",
                source: if entry_id == "backup-a" {
                    EffectiveCredentialSource::None
                } else {
                    EffectiveCredentialSource::Keychain
                },
            })
        })
        .unwrap();

        assert_eq!(statuses.len(), entry_ids.len());
        assert_eq!(
            statuses
                .iter()
                .map(|s| s.entry_id.as_str())
                .collect::<Vec<_>>(),
            vec![PRIMARY_LLM_ENTRY_ID, "backup-b", "backup-a"]
        );
        assert_eq!(
            statuses[2],
            LlmEntryCredentialStatus {
                entry_id: "backup-a".to_string(),
                configured: false,
                source: EffectiveCredentialSource::None,
            }
        );
        assert!(statuses[0].configured);
        assert!(statuses[1].configured);
    }

    #[test]
    fn batch_status_on_empty_input_returns_empty_list() {
        let statuses = collect_entry_credential_status(&[], |_| {
            panic!("空入参不应触发任何凭证读取");
        })
        .unwrap();

        assert!(statuses.is_empty());
    }

    #[test]
    fn batch_status_propagates_the_first_failure() {
        let entry_ids = vec![PRIMARY_LLM_ENTRY_ID.to_string(), "backup-a".to_string()];

        let error = collect_entry_credential_status(&entry_ids, |entry_id| {
            if entry_id == PRIMARY_LLM_ENTRY_ID {
                Ok(CredentialStatus {
                    configured: true,
                    source: EffectiveCredentialSource::Keychain,
                })
            } else {
                Err(AppError::credential("凭证读取失败"))
            }
        })
        .unwrap_err();

        assert_eq!(error.code, AppErrorCode::Credential);
    }

    #[test]
    fn batch_status_never_serializes_a_secret_field() {
        let statuses = collect_entry_credential_status(&["backup-a".to_string()], |_| {
            Ok(CredentialStatus {
                configured: true,
                source: EffectiveCredentialSource::Keychain,
            })
        })
        .unwrap();

        let serialized = serde_json::to_string(&statuses).unwrap();

        assert_eq!(
            serialized,
            r#"[{"entry_id":"backup-a","configured":true,"source":"keychain"}]"#
        );
    }
}
