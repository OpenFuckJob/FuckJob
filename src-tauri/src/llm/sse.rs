use crate::{error::AppError, llm::openai_compatible::provider_error_metadata};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    pub content: Option<String>,
    pub done: bool,
}

#[derive(Default)]
pub struct SseDecoder {
    buffer: Vec<u8>,
}

impl SseDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, AppError> {
        self.buffer.extend_from_slice(chunk);
        self.drain(false)
    }

    pub fn finish(&mut self) -> Result<Vec<SseEvent>, AppError> {
        self.drain(true)
    }

    fn drain(&mut self, finish: bool) -> Result<Vec<SseEvent>, AppError> {
        let text = String::from_utf8(self.buffer.clone())
            .map_err(|error| {
                AppError::provider("流式响应不是有效 UTF-8").with_detail(format!(
                    "SSE UTF-8 error: valid_up_to={}, error_len={:?}",
                    error.utf8_error().valid_up_to(),
                    error.utf8_error().error_len()
                ))
            })?
            .replace("\r\n", "\n");
        let mut parts: Vec<&str> = text.split("\n\n").collect();
        let remainder = if !finish {
            parts.pop().unwrap_or_default().to_string()
        } else {
            String::new()
        };
        if finish && !parts.last().is_some_and(|part| part.is_empty()) && !text.ends_with("\n\n") {
            // split already contains the final partial frame
        } else if finish && parts.last() == Some(&"") {
            parts.pop();
        }
        let mut events = Vec::new();
        for frame in parts {
            if frame.trim().is_empty() {
                continue;
            }
            let data = frame
                .lines()
                .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() {
                continue;
            }
            if data.trim() == "[DONE]" {
                events.push(SseEvent {
                    content: None,
                    done: true,
                });
                continue;
            }
            let value: Value = serde_json::from_str(&data).map_err(|e| {
                AppError::provider("无法解析流式模型响应").with_detail(format!(
                    "SSE JSON parse error: category={:?}, line={}, column={}",
                    e.classify(),
                    e.line(),
                    e.column()
                ))
            })?;
            if let Some(error) = value.get("error") {
                let detail = provider_error_metadata(error)
                    .unwrap_or_else(|| "SSE provider error without safe metadata".to_string());
                return Err(AppError::provider("模型服务返回错误").with_detail(detail));
            }
            let content = value
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
                .map(str::to_owned);
            events.push(SseEvent {
                content,
                done: false,
            });
        }
        self.buffer = remainder.into_bytes();
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::SseDecoder;

    #[test]
    fn decodes_split_crlf_frames_role_delta_and_done() {
        let mut decoder = SseDecoder::default();
        assert!(decoder
            .push(b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\r\n\r")
            .unwrap()
            .is_empty());
        let events = decoder
            .push(
                "\ndata: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}\n\ndata: [DONE]\n\n"
                    .as_bytes(),
            )
            .unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[1].content.as_deref(), Some("你"));
        assert!(events[2].done);
    }

    #[test]
    fn reports_provider_error_and_malformed_json() {
        let mut decoder = SseDecoder::default();
        assert_eq!(
            decoder
                .push(b"data: {\"error\":{\"message\":\"bad key\"}}\n\n")
                .unwrap_err()
                .code,
            crate::error::AppErrorCode::Provider
        );
        let mut decoder = SseDecoder::default();
        assert_eq!(
            decoder.push(b"data: {oops}\n\n").unwrap_err().code,
            crate::error::AppErrorCode::Provider
        );
    }

    #[test]
    fn provider_sse_error_does_not_retain_echoed_secret() {
        let mut decoder = SseDecoder::default();
        let error = decoder
            .push(b"data: {\"error\":{\"message\":\"secret prompt echo\",\"code\":\"bad_request\",\"type\":\"invalid_request\"}}\n\n")
            .unwrap_err();

        assert!(!error
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("secret prompt echo"));
        let payload =
            serde_json::to_string(&crate::command::base::CommandResult::<()>::err(error)).unwrap();
        assert!(!payload.contains("secret prompt echo"));
        assert!(!payload.contains("detail"));
    }

    #[test]
    fn finish_decodes_final_partial_frame() {
        let mut decoder = SseDecoder::default();
        decoder
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"end\"}}]}")
            .unwrap();
        assert_eq!(decoder.finish().unwrap()[0].content.as_deref(), Some("end"));
    }
}
