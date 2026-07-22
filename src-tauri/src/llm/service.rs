use crate::config::AppRuntimeConfig;
use crate::credential::ResolvedCredential;
use crate::error::AppError;
use crate::llm::openai_compatible::provider_error_metadata;
use crate::llm::template;
use crate::llm::types::{ConnectionReport, LlmResponse};
use futures::StreamExt;
use rig::client::CompletionClient;
use rig::completion::{CompletionError, CompletionModel};
use rig::streaming::StreamedAssistantContent;
use rig_core as rig;
use serde_json::Value;
use std::time::Duration;

const LLM_REQUEST_TIMEOUT_SECONDS: u64 = 120;

#[derive(Clone)]
pub struct LlmService {
    client: rig::providers::openai::CompletionsClient,
    model: String,
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
        let base_url = llm.base_url.trim().trim_end_matches('/').to_string();
        let model = llm.model.trim().to_string();
        if base_url.is_empty() || model.is_empty() {
            return Err(AppError::configuration("大模型地址和模型名称不能为空"));
        }

        let api_key = credential.secret().unwrap_or("noop");

        let mut http_builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(LLM_REQUEST_TIMEOUT_SECONDS));
        if base_url.starts_with("http://127.0.0.1")
            || base_url.starts_with("http://localhost")
        {
            http_builder = http_builder.no_proxy();
        }
        let http_client = http_builder.build().map_err(|e| {
            AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
        })?;

        // OpenAI-compatible providers broadly implement Chat Completions, while
        // Rig's default OpenAI client targets the newer Responses API.
        let client = rig::providers::openai::CompletionsClient::builder()
            .api_key(api_key)
            .base_url(&base_url)
            .http_client(http_client)
            .build()
            .map_err(|e| {
                AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
            })?;

        Ok(Self { client, model })
    }

    pub async fn generate(&self, prompt: String) -> Result<LlmResponse, AppError> {
        let model = self.client.completion_model(&self.model);
        let response = model
            .completion_request(&prompt)
            .send()
            .await
            .map_err(map_completion_error)?;

        let mut content = String::new();
        for item in response.choice {
            if let rig::completion::AssistantContent::Text(text) = item {
                content.push_str(&text.text);
            }
        }

        Ok(LlmResponse {
            content,
            model: Some(self.model.clone()),
            finish_reason: None,
            usage: None,
        })
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
        let model = self.client.completion_model(&self.model);
        let request = model.completion_request(&prompt).build();
        let mut stream = model
            .stream(request)
            .await
            .map_err(map_stream_completion_error)?;

        let mut content = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_stream_completion_error)?;
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
            model: Some(self.model.clone()),
            finish_reason: None,
            usage: None,
        })
    }

    pub async fn test_connection(&self) -> Result<ConnectionReport, AppError> {
        let response = self.generate("Reply with OK only.".to_string()).await?;
        Ok(ConnectionReport {
            model: response.model.unwrap_or_else(|| self.model.clone()),
            response: response.content,
        })
    }
}

fn map_completion_error(error: CompletionError) -> AppError {
    map_rig_error(error, false)
}

fn map_stream_completion_error(error: CompletionError) -> AppError {
    map_rig_error(error, true)
}

fn map_rig_error(error: CompletionError, streaming: bool) -> AppError {
    let status = error.provider_response_status();
    let metadata = error
        .provider_response_json()
        .ok()
        .flatten()
        .and_then(|value| value.get("error").cloned())
        .and_then(|value| provider_error_metadata(&value));

    // Build a safe user-facing diagnostic (HTTP status + provider error code/type).
    // The `provider_error_metadata` output only includes alphanumeric-safe fields
    // (code, type), never raw message bodies, so it is safe to surface to users.
    let diagnostic = match (&status, &metadata) {
        (Some(s), Some(m)) => Some(format!("（HTTP {s}，{m}）")),
        (Some(s), None) => Some(format!("（HTTP {s}）")),
        (None, _) => None,
    };

    let base = match status {
        Some(reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN) => {
            "大模型密钥无效或无权访问"
        }
        Some(reqwest::StatusCode::NOT_FOUND) => "大模型地址或模型不存在",
        Some(reqwest::StatusCode::TOO_MANY_REQUESTS) => {
            "大模型服务返回 HTTP 429：请求受限或账户额度不足"
        }
        Some(_) => "大模型服务请求失败",
        None => match &error {
            CompletionError::HttpError(_) => {
                if streaming {
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

    let mut mapped = match status {
        Some(reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN) => {
            AppError::credential(message)
        }
        Some(reqwest::StatusCode::NOT_FOUND) => AppError::provider(message),
        Some(reqwest::StatusCode::TOO_MANY_REQUESTS) => AppError::provider(message),
        Some(_) => AppError::provider(message),
        None => match &error {
            CompletionError::HttpError(_) => AppError::network(message),
            _ => AppError::provider(message),
        },
    };

    // Attach full diagnostics as internal detail for logging (never serialized).
    let detail = match (status, metadata) {
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
        CompletionError::ProviderResponse(_) => Some("provider returned an error response".to_string()),
        CompletionError::ProviderError(_) => Some("provider request failed".to_string()),
        CompletionError::RequestError(_) => Some("completion request build failed".to_string()),
        _ => None,
    }
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
        let mut config = default_app_config();
        config.llm_config = Some(LlmConfig {
            provider: LlmProviderPreset::Custom,
            base_url,
            model: "local-model".to_string(),
        });
        let credential =
            resolve_with_environment(&EmptyCredentialBackend, Some("secret")).unwrap();
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
                if let Some(header_end) = bytes.windows(4).position(|value| value == b"\r\n\r\n")
                {
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
            error.message.contains("大模型服务返回 HTTP 429：请求受限或账户额度不足"),
            "message should include the base error: {}",
            error.message
        );
        assert!(
            error.message.contains("（HTTP 429") && error.message.contains("code=rate_limit"),
            "message should include safe provider diagnostics: {}",
            error.message
        );
        assert!(!error.message.contains(secret), "message must not echo the provider body");
        let detail = error.detail.unwrap_or_default();
        assert!(detail.contains("HTTP 429"));
        assert!(detail.contains("code=rate_limit"));
        assert!(!detail.contains(secret));
    }
}
