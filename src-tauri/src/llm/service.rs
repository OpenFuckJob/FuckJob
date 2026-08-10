use crate::config::{AppRuntimeConfig, LlmProviderPreset};
use crate::credential::ResolvedCredential;
use crate::error::AppError;
use crate::llm::template;
use crate::llm::types::{ConnectionReport, LlmResponse};
use futures::StreamExt;
use rig::client::CompletionClient;
use rig::completion::{CompletionError, CompletionModel};
use rig::streaming::StreamedAssistantContent;
use serde_json::Value;
use std::time::Duration;
use tokio::time::timeout;

const LLM_REQUEST_TIMEOUT_SECONDS: u64 = 120;

#[derive(Clone, Debug)]
pub struct LlmService {
    backend: LlmBackend,
    model: String,
    provider: LlmProviderPreset,
}

#[derive(Clone, Debug)]
enum LlmBackend {
    Anthropic(rig::providers::anthropic::Client),
    DeepSeek(rig::providers::deepseek::Client),
    OpenAiCompatible(rig::providers::openai::CompletionsClient),
    OpenAiResponses(rig::providers::openai::Client),
    MiniMax(rig::providers::minimax::Client),
    Moonshot(rig::providers::moonshot::Client),
    Ollama(rig::providers::ollama::Client),
    OpenRouter(rig::providers::openrouter::Client),
    XiaomiMimo(rig::providers::xiaomimimo::Client),
    ZAi(rig::providers::zai::Client),
}

impl LlmService {
    pub fn from_runtime(
        config: &AppRuntimeConfig,
        credential: &ResolvedCredential,
    ) -> Result<Self, AppError> {
        let llm = config
            .llm_config
            .as_ref()
            .ok_or_else(|| AppError::configuration("请先配置大模型服务"))?;
        let provider = llm.provider.clone();
        let base_url = normalize_provider_base_url(&provider, &llm.base_url);
        let model = llm.model.trim().to_string();
        if base_url.is_empty() || model.is_empty() {
            return Err(AppError::configuration("大模型地址和模型名称不能为空"));
        }

        let secret = credential.secret();
        if provider_requires_key(&provider) && secret.is_none() {
            return Err(AppError::credential("请先配置该大模型服务的 API Key"));
        }
        let api_key = secret.unwrap_or(if matches!(provider, LlmProviderPreset::Ollama) {
            ""
        } else {
            "noop"
        });

        let backend = match provider {
            LlmProviderPreset::Anthropic => LlmBackend::Anthropic(
                rig::providers::anthropic::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::DeepSeek => LlmBackend::DeepSeek(
                rig::providers::deepseek::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::OpenAi => {
                // Use Rig's OpenAI-compatible Chat Completions client so user supplied
                // OpenAI-compatible BaseURL values keep working with /chat/completions.
                LlmBackend::OpenAiCompatible(
                    rig::providers::openai::CompletionsClient::builder()
                        .api_key(api_key)
                        .base_url(&base_url)
                        .build()
                        .map_err(|e| {
                            AppError::configuration("无法创建大模型客户端")
                                .with_detail(e.to_string())
                        })?,
                )
            }
            LlmProviderPreset::OpenAiResponses => LlmBackend::OpenAiResponses(
                rig::providers::openai::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::MiniMax => LlmBackend::MiniMax(
                rig::providers::minimax::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::Moonshot => LlmBackend::Moonshot(
                rig::providers::moonshot::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::Ollama => LlmBackend::Ollama(
                rig::providers::ollama::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::OpenRouter => LlmBackend::OpenRouter(
                rig::providers::openrouter::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::XiaomiMimo => LlmBackend::XiaomiMimo(
                rig::providers::xiaomimimo::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
            LlmProviderPreset::ZAi => LlmBackend::ZAi(
                rig::providers::zai::Client::builder()
                    .api_key(api_key)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| {
                        AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
                    })?,
            ),
        };

        Ok(Self {
            backend,
            model,
            provider,
        })
    }

    pub async fn generate(&self, prompt: String) -> Result<LlmResponse, AppError> {
        match &self.backend {
            LlmBackend::Anthropic(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                )
                .await
            }
            LlmBackend::DeepSeek(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                )
                .await
            }
            LlmBackend::OpenAiCompatible(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                )
                .await
            }
            LlmBackend::OpenAiResponses(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                )
                .await
            }
            LlmBackend::MiniMax(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                )
                .await
            }
            LlmBackend::Moonshot(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                )
                .await
            }
            LlmBackend::Ollama(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                )
                .await
            }
            LlmBackend::OpenRouter(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                )
                .await
            }
            LlmBackend::XiaomiMimo(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                )
                .await
            }
            LlmBackend::ZAi(client) => {
                complete_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                )
                .await
            }
        }
    }

    pub async fn generate_template(
        &self,
        prompt_template: &str,
        params: &Value,
    ) -> Result<LlmResponse, AppError> {
        self.generate(template::render(prompt_template, params)?)
            .await
    }

    pub async fn stream<F>(&self, prompt: String, mut on_delta: F) -> Result<LlmResponse, AppError>
    where
        F: FnMut(String) -> Result<(), AppError>,
    {
        match &self.backend {
            LlmBackend::Anthropic(client) => {
                stream_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &mut on_delta,
                )
                .await
            }
            LlmBackend::DeepSeek(client) => {
                stream_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &mut on_delta,
                )
                .await
            }
            LlmBackend::OpenAiCompatible(client) => {
                stream_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &mut on_delta,
                )
                .await
            }
            LlmBackend::OpenAiResponses(client) => {
                stream_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &mut on_delta,
                )
                .await
            }
            LlmBackend::MiniMax(client) => {
                stream_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &mut on_delta,
                )
                .await
            }
            LlmBackend::Moonshot(client) => {
                stream_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &mut on_delta,
                )
                .await
            }
            LlmBackend::Ollama(client) => {
                stream_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &mut on_delta,
                )
                .await
            }
            LlmBackend::OpenRouter(client) => {
                stream_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &mut on_delta,
                )
                .await
            }
            LlmBackend::XiaomiMimo(client) => {
                stream_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &mut on_delta,
                )
                .await
            }
            LlmBackend::ZAi(client) => {
                stream_once(
                    client.completion_model(&self.model),
                    &self.model,
                    &prompt,
                    &self.provider,
                    &mut on_delta,
                )
                .await
            }
        }
    }

    pub async fn test_connection(&self) -> Result<ConnectionReport, AppError> {
        let response = self.generate("Reply with OK only.".to_string()).await?;
        Ok(ConnectionReport {
            model: response.model.unwrap_or_else(|| self.model.clone()),
            response: response.content,
        })
    }
}

async fn complete_once<M>(
    model: M,
    model_name: &str,
    prompt: &str,
    provider: &LlmProviderPreset,
) -> Result<LlmResponse, AppError>
where
    M: CompletionModel + Send,
{
    let response = timeout(Duration::from_secs(LLM_REQUEST_TIMEOUT_SECONDS), async {
        model.completion_request(prompt).send().await
    })
    .await
    .map_err(|_| AppError::network("大模型请求超时"))?
    .map_err(|error| map_completion_error(error, provider))?;

    let mut content = String::new();
    for item in response.choice {
        if let rig::completion::AssistantContent::Text(text) = item {
            content.push_str(&text.text);
        }
    }

    Ok(LlmResponse {
        content,
        model: Some(model_name.to_string()),
        finish_reason: None,
        usage: None,
    })
}

async fn stream_once<M, F>(
    model: M,
    model_name: &str,
    prompt: &str,
    provider: &LlmProviderPreset,
    on_delta: &mut F,
) -> Result<LlmResponse, AppError>
where
    M: CompletionModel + Send,
    F: FnMut(String) -> Result<(), AppError>,
{
    timeout(Duration::from_secs(LLM_REQUEST_TIMEOUT_SECONDS), async {
        let request = model.completion_request(prompt).build();
        let mut stream = model
            .stream(request)
            .await
            .map_err(|error| map_stream_completion_error(error, provider))?;

        let mut content = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| map_stream_completion_error(error, provider))?;
            match chunk {
                StreamedAssistantContent::Text(text) => {
                    content.push_str(&text.text);
                    on_delta(text.text)?;
                }
                _ => {}
            }
        }

        Ok(LlmResponse {
            content,
            model: Some(model_name.to_string()),
            finish_reason: None,
            usage: None,
        })
    })
    .await
    .map_err(|_| AppError::network("大模型流式请求超时"))?
}

pub(crate) fn normalize_provider_base_url(provider: &LlmProviderPreset, base_url: &str) -> String {
    let base_url = base_url.trim().trim_end_matches('/');
    if matches!(provider, LlmProviderPreset::Ollama) {
        base_url.strip_suffix("/v1").unwrap_or(base_url).to_string()
    } else {
        base_url.to_string()
    }
}

pub(crate) fn provider_requires_key(provider: &LlmProviderPreset) -> bool {
    !matches!(provider, LlmProviderPreset::Ollama)
}

fn map_completion_error(error: CompletionError, provider: &LlmProviderPreset) -> AppError {
    map_rig_error(error, false, provider)
}

fn map_stream_completion_error(error: CompletionError, provider: &LlmProviderPreset) -> AppError {
    map_rig_error(error, true, provider)
}

fn map_rig_error(
    error: CompletionError,
    streaming: bool,
    provider: &LlmProviderPreset,
) -> AppError {
    let status_code = error
        .provider_response_status()
        .map(|status| status.as_u16());
    let metadata = error
        .provider_response_json()
        .ok()
        .flatten()
        .and_then(|value| value.get("error").cloned())
        .and_then(|value| provider_error_metadata(&value));

    // Build a safe user-facing diagnostic (HTTP status + provider error code/type).
    // The `provider_error_metadata` output only includes alphanumeric-safe fields
    // (code, type), never raw message bodies, so it is safe to surface to users.
    let diagnostic = match (status_code, metadata.as_ref()) {
        (Some(s), Some(m)) => Some(format!("（HTTP {s}，{m}）")),
        (Some(s), None) => Some(format!("（HTTP {s}）")),
        (None, _) => None,
    };

    let base = match status_code {
        Some(401 | 403) => "大模型密钥无效或无权访问",
        Some(404) if matches!(provider, LlmProviderPreset::Ollama) => {
            "Ollama 模型不存在，请先在 Ollama 中 pull 该模型"
        }
        Some(404) => "大模型地址或模型不存在",
        Some(429) => "大模型服务返回 HTTP 429：请求受限或账户额度不足",
        Some(_) => "大模型服务请求失败",
        None => match &error {
            CompletionError::HttpError(_) => {
                if matches!(provider, LlmProviderPreset::Ollama) {
                    "无法连接 Ollama，请确认 Ollama 已启动"
                } else if streaming {
                    "大模型流式请求失败"
                } else {
                    "无法连接大模型服务"
                }
            }
            _ => {
                if streaming {
                    "大模型流式生成失败"
                } else {
                    "大模型生成失败"
                }
            }
        },
    };

    let message = match diagnostic.as_deref() {
        Some(d) => format!("{base}{d}"),
        None => base.to_string(),
    };

    let mut mapped = match status_code {
        Some(401 | 403) => AppError::credential(message),
        Some(404 | 429) => AppError::provider(message),
        Some(_) => AppError::provider(message),
        None => match &error {
            CompletionError::HttpError(_) => AppError::network(message),
            _ => AppError::provider(message),
        },
    };

    // Attach full diagnostics as internal detail for logging (never serialized).
    let detail = match (status_code, metadata) {
        (Some(s), Some(m)) => Some(format!("HTTP {s}; {m}")),
        (Some(s), None) => Some(format!("HTTP {s}")),
        (None, _) => safe_completion_detail(&error),
    };
    if let Some(detail) = detail {
        mapped = mapped.with_detail(detail);
    }
    mapped
}

fn safe_completion_detail(error: &CompletionError) -> Option<String> {
    match error {
        CompletionError::HttpError(rig::http_client::Error::Instance(source)) => {
            let message = source.to_string().to_ascii_lowercase();
            if message.contains("timeout") || message.contains("timed out") {
                Some("request timed out".to_string())
            } else {
                Some("HTTP transport failed".to_string())
            }
        }
        CompletionError::HttpError(_) => Some("HTTP transport failed".to_string()),
        CompletionError::JsonError(error) => Some(format!(
            "JSON response parse error: category={:?}, line={}, column={}",
            error.classify(),
            error.line(),
            error.column()
        )),
        CompletionError::UrlError(_) => Some("invalid provider URL".to_string()),
        CompletionError::ResponseError(_) => Some("provider response parse failed".to_string()),
        CompletionError::ProviderResponse(_) => {
            Some("provider returned an error response".to_string())
        }
        CompletionError::ProviderError(_) => Some("provider request failed".to_string()),
        CompletionError::RequestError(_) => Some("completion request build failed".to_string()),
        _ => None,
    }
}

fn safe_provider_identifier(value: &str) -> Option<&str> {
    (!value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
    .then_some(value)
}

fn provider_error_metadata(error: &Value) -> Option<String> {
    let mut fields = Vec::new();
    for name in ["code", "type"] {
        if let Some(value) = error
            .get(name)
            .and_then(Value::as_str)
            .and_then(safe_provider_identifier)
        {
            fields.push(format!("{name}={value}"));
        }
    }
    (!fields.is_empty()).then(|| fields.join(", "))
}

#[cfg(test)]
mod tests {
    use super::LlmService;
    use crate::config::{default_app_config, LlmConfig, LlmProviderPreset};
    use crate::credential::{resolve_with_environment, CredentialBackend};
    use crate::error::AppError;
    use serde_json::Value;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    #[derive(Default)]
    struct EmptyCredentialBackend;

    impl CredentialBackend for EmptyCredentialBackend {
        fn get(&self) -> Result<Option<String>, AppError> {
            Ok(None)
        }

        fn set(&self, _secret: &str) -> Result<(), AppError> {
            Ok(())
        }

        fn delete(&self) -> Result<(), AppError> {
            Ok(())
        }
    }

    fn service(base_url: String) -> LlmService {
        service_with_provider(LlmProviderPreset::OpenAi, base_url)
    }

    fn service_with_provider(provider: LlmProviderPreset, base_url: String) -> LlmService {
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider,
            base_url,
            model: "local-model".to_string(),
        });
        let credential = resolve_with_environment(&EmptyCredentialBackend, Some("secret")).unwrap();
        LlmService::from_runtime(&config, &credential).unwrap()
    }

    fn mock_server(response_parts: Vec<&'static [u8]>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(header_end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..header_end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&bytes).to_string());
            for part in response_parts {
                stream.write_all(part).unwrap();
                stream.flush().unwrap();
            }
        });
        (format!("http://{address}/v1"), receiver)
    }

    fn http_response(content_type: &str, body: &str) -> &'static [u8] {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        Box::leak(response.into_bytes().into_boxed_slice())
    }

    #[test]
    fn rig_completion_uses_chat_completions_with_auth_and_model() {
        let body = r#"{"id":"chatcmpl-1","object":"chat.completion","created":1,"model":"local-model","system_fingerprint":null,"choices":[{"index":0,"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2,"prompt_tokens_details":null,"completion_tokens_details":null}}"#;
        let (url, received) = mock_server(vec![http_response("application/json", body)]);
        let result = tauri::async_runtime::block_on(service(url).test_connection()).unwrap();

        assert_eq!(result.response, "OK");
        let raw = received.recv().unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /v1/chat/completions "));
        assert!(headers
            .to_ascii_lowercase()
            .contains("authorization: bearer secret"));
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["model"], "local-model");
        assert!(payload.get("temperature").is_none());
        assert!(payload.get("max_tokens").is_none());
        assert!(payload.get("stream").is_none());
    }

    #[test]
    fn rig_responses_provider_uses_responses_endpoint_with_auth_and_model() {
        let body = r#"{"id":"resp_1","object":"response","created_at":1,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"local-model","usage":null,"output":[{"type":"message","id":"msg_1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"OK","annotations":[]}]}],"tools":[]}"#;
        let (url, received) = mock_server(vec![http_response("application/json", body)]);
        let result = tauri::async_runtime::block_on(
            service_with_provider(LlmProviderPreset::OpenAiResponses, url).test_connection(),
        )
        .unwrap();

        assert_eq!(result.response, "OK");
        let raw = received.recv().unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /v1/responses "));
        assert!(headers
            .to_ascii_lowercase()
            .contains("authorization: bearer secret"));
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["model"], "local-model");
        assert!(payload.get("temperature").is_none());
        assert!(payload.get("max_output_tokens").is_none());
        assert!(payload.get("stream").is_none());
    }

    #[test]
    fn rig_responses_stream_uses_responses_endpoint_and_preserves_delta_order() {
        let completed = r#"{"type":"response.completed","sequence_number":3,"response":{"id":"resp_1","object":"response","created_at":1,"status":"completed","error":null,"incomplete_details":null,"instructions":null,"max_output_tokens":null,"model":"local-model","usage":null,"output":[{"type":"message","id":"msg_1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"OK","annotations":[]}]}],"tools":[]}}"#;
        let (url, received) = mock_server(vec![
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"sequence_number\":1,\"delta\":\"O\"}\n\n",
            b"data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"sequence_number\":2,\"delta\":\"K\"}\n\n",
            Box::leak(format!("data: {completed}\n\ndata: [DONE]\n\n").into_bytes().into_boxed_slice()),
        ]);
        let mut deltas = Vec::new();
        let report = tauri::async_runtime::block_on(
            service_with_provider(LlmProviderPreset::OpenAiResponses, url).stream(
                "stream test".to_string(),
                |delta| {
                    deltas.push(delta);
                    Ok(())
                },
            ),
        )
        .unwrap();

        assert_eq!(report.content, "OK");
        assert_eq!(deltas, ["O", "K"]);
        let raw = received.recv().unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /v1/responses "));
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["stream"], true);
    }

    #[test]
    fn rig_stream_uses_chat_completions_and_preserves_delta_order() {
        let (url, received) = mock_server(vec![
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"id\":\"chatcmpl-1\",\"model\":\"local-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"O\"},\"finish_reason\":null}]}\n\n",
            b"data: {\"id\":\"chatcmpl-1\",\"model\":\"local-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"K\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ]);
        let mut deltas = Vec::new();
        let report = tauri::async_runtime::block_on(service(url).stream(
            "stream test".to_string(),
            |delta| {
                deltas.push(delta);
                Ok(())
            },
        ))
        .unwrap();

        assert_eq!(report.content, "OK");
        assert_eq!(deltas, ["O", "K"]);
        let raw = received.recv().unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /v1/chat/completions "));
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["stream"], true);
    }

    #[test]
    fn rig_provider_error_maps_status_without_retaining_echoed_body() {
        let secret = "prompt-and-token-never-retain";
        let body = format!(
            r#"{{"error":{{"message":"{secret}","code":"rate_limit","type":"requests"}}}}"#
        );
        let response = format!(
            "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let leaked = Box::leak(response.into_bytes().into_boxed_slice());
        let (url, _) = mock_server(vec![leaked]);

        let error = tauri::async_runtime::block_on(service(url).test_connection()).unwrap_err();
        assert!(
            error
                .message
                .contains("大模型服务返回 HTTP 429：请求受限或账户额度不足"),
            "message should include the base error: {}",
            error.message
        );
        assert!(
            error.message.contains("（HTTP 429") && error.message.contains("code=rate_limit"),
            "message should include safe provider diagnostics: {}",
            error.message
        );
        assert!(
            !error.message.contains(secret),
            "message must not echo the provider body"
        );
        let detail = error.detail.unwrap_or_default();
        assert!(detail.contains("HTTP 429"));
        assert!(detail.contains("code=rate_limit"));
        assert!(!detail.contains(secret));
    }

    #[test]
    fn ollama_provider_uses_native_chat_endpoint_and_strips_legacy_v1_suffix() {
        let body = r#"{"model":"llama3.2","created_at":"2026-08-10T00:00:00Z","message":{"role":"assistant","content":"OK"},"done":true,"prompt_eval_count":1,"eval_count":1}"#;
        let (url, received) = mock_server(vec![http_response("application/json", body)]);
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderPreset::Ollama,
            base_url: url,
            model: "llama3.2".to_string(),
        });
        let credential = resolve_with_environment(&EmptyCredentialBackend, None).unwrap();
        let result = tauri::async_runtime::block_on(
            LlmService::from_runtime(&config, &credential)
                .unwrap()
                .test_connection(),
        )
        .unwrap();

        assert_eq!(result.response, "OK");
        let raw = received.recv().unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(headers.starts_with("POST /api/chat "));
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        let payload: Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["model"], "llama3.2");
        assert_eq!(payload["stream"], false);
    }

    #[test]
    fn key_required_providers_fail_before_client_creation_without_secret() {
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderPreset::DeepSeek,
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-chat".to_string(),
        });
        let credential = resolve_with_environment(&EmptyCredentialBackend, None).unwrap();
        let error = LlmService::from_runtime(&config, &credential).unwrap_err();

        assert_eq!(error.message, "请先配置该大模型服务的 API Key");
    }

    #[test]
    fn normalizes_ollama_base_url_only_at_runtime() {
        assert_eq!(
            super::normalize_provider_base_url(
                &LlmProviderPreset::Ollama,
                " http://localhost:11434/v1/ "
            ),
            "http://localhost:11434"
        );
        assert_eq!(
            super::normalize_provider_base_url(
                &LlmProviderPreset::OpenAi,
                " http://localhost:1234/v1/ "
            ),
            "http://localhost:1234/v1"
        );
    }
}
