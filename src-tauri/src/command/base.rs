use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CommandResult<T> {
    pub data: Option<T>,
    pub success: bool,
    pub error: Option<AppError>,
}

impl<T> CommandResult<T> {
    pub fn ok(data: T) -> Self {
        Self {
            data: Some(data),
            success: true,
            error: None,
        }
    }

    pub fn err(error: impl Into<AppError>) -> Self {
        Self {
            data: None,
            success: false,
            error: Some(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CommandResult;
    use crate::error::{AppError, AppErrorCode};
    use serde_json::json;

    #[test]
    fn legacy_errors_are_serialized_as_structured_internal_errors() {
        let result = CommandResult::<()>::err("连接失败");

        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({
                "data": null,
                "success": false,
                "error": {
                    "code": "internal",
                    "message": "连接失败",
                },
            })
        );
    }

    #[test]
    fn command_payload_omits_internal_error_detail() {
        let result = CommandResult::<()>::err(
            AppError::provider("模型服务请求失败").with_detail("echoed-secret-body"),
        );
        let serialized = serde_json::to_string(&result).unwrap();

        assert!(!serialized.contains("detail"));
        assert!(!serialized.contains("echoed-secret-body"));
    }

    #[test]
    fn structured_errors_keep_their_code() {
        let result =
            CommandResult::<()>::err(AppError::new(AppErrorCode::Configuration, "请配置模型"));

        assert_eq!(result.error.unwrap().code, AppErrorCode::Configuration);
    }
}
