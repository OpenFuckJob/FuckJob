use crate::error::{AppError, AppErrorCode};
use crate::llm::sse::SseDecoder;
use crate::llm::types::{LlmRequest, LlmResponse, LlmUsage};
use reqwest::{Client, StatusCode};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone)]
pub struct OpenAiCompatibleProvider {
    client: Client,
    base_url: String,
    api_key: Option<String>,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        base_url: &str,
        api_key: Option<&str>,
        timeout_seconds: u64,
    ) -> Result<Self, AppError> {
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err(AppError::configuration("大模型地址不能为空"));
        }
        let mut builder = Client::builder();
        if base_url.starts_with("http://127.0.0.1") || base_url.starts_with("http://localhost") {
            builder = builder.no_proxy();
        }
        let client = builder
            .timeout(Duration::from_secs(timeout_seconds.max(1)))
            .build()
            .map_err(|e| {
                AppError::configuration("无法创建大模型客户端").with_detail(e.to_string())
            })?;
        Ok(Self {
            client,
            base_url,
            api_key: api_key.map(str::to_owned),
        })
    }

    pub(crate) fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let request = self.client.request(method, self.endpoint(path));
        match &self.api_key {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    }

    pub async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse, AppError> {
        let response = self
            .request(reqwest::Method::POST, "chat/completions")
            .json(request)
            .send()
            .await
            .map_err(map_transport)?;
        let response = ensure_success(response).await?;
        let body: CompletionBody = parse_json_response(response, "无法解析模型响应").await?;
        let choice = body
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| AppError::provider("模型响应缺少候选结果"))?;
        let content = choice
            .message
            .content
            .ok_or_else(|| AppError::provider("模型响应缺少文本内容"))?;
        Ok(LlmResponse {
            content,
            model: body.model,
            finish_reason: choice.finish_reason,
            usage: body.usage,
        })
    }

    pub async fn stream<F>(
        &self,
        request: &LlmRequest,
        mut on_delta: F,
    ) -> Result<LlmResponse, AppError>
    where
        F: FnMut(String) -> Result<(), AppError>,
    {
        let response = self
            .request(reqwest::Method::POST, "chat/completions")
            .json(request)
            .send()
            .await
            .map_err(map_transport)?;
        let mut response = ensure_success(response).await?;
        let mut decoder = SseDecoder::default();
        let mut content = String::new();
        let mut completed = false;
        while let Some(chunk) = response.chunk().await.map_err(map_transport)? {
            for event in decoder.push(&chunk)? {
                completed |= event.done;
                if let Some(delta) = event.content {
                    content.push_str(&delta);
                    on_delta(delta)?;
                }
            }
        }
        for event in decoder.finish()? {
            completed |= event.done;
            if let Some(delta) = event.content {
                content.push_str(&delta);
                on_delta(delta)?;
            }
        }
        if !completed {
            return Err(AppError::network("大模型流式响应意外中断"));
        }
        Ok(LlmResponse {
            content,
            model: None,
            finish_reason: None,
            usage: None,
        })
    }
}

#[derive(Deserialize)]
struct CompletionBody {
    choices: Vec<Choice>,
    model: Option<String>,
    usage: Option<LlmUsage>,
}
#[derive(Deserialize)]
struct Choice {
    message: CompletionMessage,
    finish_reason: Option<String>,
}
#[derive(Deserialize)]
struct CompletionMessage {
    content: Option<String>,
}

fn map_transport(error: reqwest::Error) -> AppError {
    let message = if error.is_timeout() {
        "大模型请求超时"
    } else {
        "无法连接大模型服务"
    };
    AppError::network(message).with_detail(error.to_string())
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, AppError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.ok();
    let error = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            AppError::credential("大模型密钥无效或无权访问")
        }
        StatusCode::NOT_FOUND => AppError::provider("大模型地址或模型不存在"),
        StatusCode::TOO_MANY_REQUESTS => AppError::provider("大模型服务请求过于频繁"),
        _ => AppError::new(AppErrorCode::Provider, "大模型服务请求失败"),
    };
    Err(error.with_detail(provider_http_detail(status, body.as_deref())))
}

async fn parse_json_response<T: DeserializeOwned>(
    response: reqwest::Response,
    message: &'static str,
) -> Result<T, AppError> {
    let bytes = response.bytes().await.map_err(map_transport)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| AppError::provider(message).with_detail(json_parse_detail(&error)))
}

fn json_parse_detail(error: &serde_json::Error) -> String {
    format!(
        "JSON response parse error: category={:?}, line={}, column={}",
        error.classify(),
        error.line(),
        error.column()
    )
}

fn safe_provider_identifier(value: &str) -> Option<&str> {
    (!value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
    .then_some(value)
}

pub(crate) fn provider_error_metadata(error: &Value) -> Option<String> {
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

fn provider_http_detail(status: StatusCode, body: Option<&str>) -> String {
    let metadata = body
        .and_then(|body| serde_json::from_str::<Value>(body).ok())
        .and_then(|value| value.get("error").cloned())
        .and_then(|error| provider_error_metadata(&error));
    match metadata {
        Some(metadata) => format!("HTTP {status}; {metadata}"),
        None => format!("HTTP {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::OpenAiCompatibleProvider;
    use crate::error::AppErrorCode;
    use crate::llm::types::{LlmMessage, LlmRequest, LlmRole};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    fn request(stream: bool) -> LlmRequest {
        LlmRequest {
            model: "local-model".to_string(),
            messages: vec![LlmMessage {
                role: LlmRole::User,
                content: "hello".to_string(),
            }],
            stream,
        }
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
                if let Some(header_end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..header_end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
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
    #[test]
    fn endpoint_has_exactly_one_separator() {
        let provider = OpenAiCompatibleProvider::new("http://localhost:1234/v1/", None, 1).unwrap();
        assert_eq!(
            provider.endpoint("/chat/completions"),
            "http://localhost:1234/v1/chat/completions"
        );
    }

    #[test]
    fn completion_sends_bearer_auth_and_standard_body() {
        let body = r#"{"choices":[{"message":{"content":"OK"},"finish_reason":"stop"}],"model":"local-model","usage":{"total_tokens":2}}"#;
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        let leaked: &'static [u8] = Box::leak(response.into_bytes().into_boxed_slice());
        let (url, received) = mock_server(vec![leaked]);
        let provider = OpenAiCompatibleProvider::new(&url, Some("secret"), 2).unwrap();
        let result = tauri::async_runtime::block_on(provider.complete(&request(false))).unwrap();
        assert_eq!(result.content, "OK");
        let raw = received.recv().unwrap();
        assert!(raw.starts_with("POST /v1/chat/completions "));
        assert!(raw
            .to_ascii_lowercase()
            .contains("authorization: bearer secret"));
        assert!(raw.contains("\"model\":\"local-model\""));
        assert!(raw.contains("\"stream\":false"));
    }

    #[test]
    fn unauthorized_status_maps_to_credential_error() {
        let (url, _) = mock_server(vec![
            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 3\r\nConnection: close\r\n\r\nbad",
        ]);
        let provider = OpenAiCompatibleProvider::new(&url, None, 2).unwrap();
        let error = tauri::async_runtime::block_on(provider.complete(&request(false))).unwrap_err();
        assert_eq!(error.code, AppErrorCode::Credential);
    }

    #[test]
    fn provider_http_error_never_retains_or_serializes_echoed_body() {
        let secret = "prompt-and-token-never-retain";
        let body = format!(
            r#"{{"error":{{"message":"{secret}","code":"rate_limit","type":"requests"}}}}"#
        );
        let response = format!(
            "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(), body
        );
        let leaked: &'static [u8] = Box::leak(response.into_bytes().into_boxed_slice());
        let (url, _) = mock_server(vec![leaked]);
        let provider = OpenAiCompatibleProvider::new(&url, None, 2).unwrap();

        let error = tauri::async_runtime::block_on(provider.complete(&request(false))).unwrap_err();
        assert!(!error.detail.as_deref().unwrap_or_default().contains(secret));
        let payload =
            serde_json::to_string(&crate::command::base::CommandResult::<()>::err(error)).unwrap();
        assert!(!payload.contains(secret));
        assert!(!payload.contains("detail"));
    }

    #[test]
    fn split_sse_is_ordered_and_disconnect_is_not_success() {
        let (url, _) = mock_server(vec![
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"A\"}}]}\n",
            b"\ndata: {\"choices\":[{\"delta\":{\"content\":\"B\"}}]}\n\ndata: [DONE]\n\n",
        ]);
        let provider = OpenAiCompatibleProvider::new(&url, None, 2).unwrap();
        let mut deltas = Vec::new();
        let result = tauri::async_runtime::block_on(provider.stream(&request(true), |d| {
            deltas.push(d);
            Ok(())
        }))
        .unwrap();
        assert_eq!(result.content, "AB");
        assert_eq!(deltas, ["A", "B"]);

        let (url, _) = mock_server(vec![b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n"]);
        let provider = OpenAiCompatibleProvider::new(&url, None, 2).unwrap();
        let error = tauri::async_runtime::block_on(provider.stream(&request(true), |_| Ok(())))
            .unwrap_err();
        assert_eq!(error.code, AppErrorCode::Network);
    }
}
