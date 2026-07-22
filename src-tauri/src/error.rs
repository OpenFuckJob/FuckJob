use serde::{Deserialize, Serialize};
use std::fmt::{self, Display, Formatter};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AppErrorCode {
    Configuration,
    Credential,
    Network,
    Provider,
    Storage,
    Browser,
    Validation,
    Cancelled,
    Internal,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    pub code: AppErrorCode,
    pub message: String,
    #[serde(skip_serializing)]
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: AppErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Configuration, message)
    }

    pub fn credential(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Credential, message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Network, message)
    }

    pub fn provider(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Provider, message)
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Storage, message)
    }

    pub fn browser(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Browser, message)
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Validation, message)
    }

    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Cancelled, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(AppErrorCode::Internal, message)
    }
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

impl From<String> for AppError {
    fn from(message: String) -> Self {
        Self::internal(message)
    }
}

impl From<&str> for AppError {
    fn from(message: &str) -> Self {
        Self::internal(message)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        Self::internal("操作失败").with_detail(error.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::storage("存储操作失败").with_detail(error.to_string())
    }
}

impl From<keyring::Error> for AppError {
    fn from(error: keyring::Error) -> Self {
        Self::credential("凭证操作失败").with_detail(error.to_string())
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for AppError {
    fn from(error: tokio::sync::oneshot::error::RecvError) -> Self {
        Self::internal("后台任务通信失败").with_detail(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{AppError, AppErrorCode};
    use serde_json::json;

    #[test]
    fn error_codes_have_stable_lowercase_serialization() {
        let cases = [
            (AppErrorCode::Configuration, "configuration"),
            (AppErrorCode::Credential, "credential"),
            (AppErrorCode::Network, "network"),
            (AppErrorCode::Provider, "provider"),
            (AppErrorCode::Storage, "storage"),
            (AppErrorCode::Browser, "browser"),
            (AppErrorCode::Validation, "validation"),
            (AppErrorCode::Cancelled, "cancelled"),
            (AppErrorCode::Internal, "internal"),
        ];

        for (code, expected) in cases {
            assert_eq!(serde_json::to_value(code).unwrap(), json!(expected));
        }
    }

    #[test]
    fn display_only_exposes_the_user_message() {
        let error =
            AppError::new(AppErrorCode::Internal, "操作失败").with_detail("debug-only context");

        assert_eq!(error.to_string(), "操作失败");
    }

    #[test]
    fn serialization_never_exposes_internal_detail() {
        let error = AppError::internal("操作失败").with_detail("token=never-serialize-me");
        let serialized = serde_json::to_value(error).unwrap();

        assert_eq!(
            serialized,
            json!({"code": "internal", "message": "操作失败"})
        );
        assert!(!serialized.to_string().contains("never-serialize-me"));
    }

    #[test]
    fn io_errors_become_storage_errors_with_safe_messages() {
        let source = std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "permission denied for /secret/path",
        );
        let diagnostic = source.to_string();

        let error = AppError::from(source);

        assert_eq!(error.code, AppErrorCode::Storage);
        assert_eq!(error.message, "存储操作失败");
        assert_eq!(error.detail.as_deref(), Some(diagnostic.as_str()));
    }

    #[test]
    fn keyring_errors_become_credential_errors_with_safe_messages() {
        let source = keyring::Error::NoEntry;
        let diagnostic = source.to_string();

        let error = AppError::from(source);

        assert_eq!(error.code, AppErrorCode::Credential);
        assert_eq!(error.message, "凭证操作失败");
        assert_eq!(error.detail.as_deref(), Some(diagnostic.as_str()));
    }

    #[test]
    fn anyhow_errors_hide_diagnostics_from_the_user_message() {
        let error = AppError::from(anyhow::anyhow!(
            "database failed at /secret/path with token=abc"
        ));

        assert_eq!(error.code, AppErrorCode::Internal);
        assert_eq!(error.message, "操作失败");
        assert_eq!(
            error.detail.as_deref(),
            Some("database failed at /secret/path with token=abc")
        );
    }

    #[test]
    fn task_receive_errors_hide_diagnostics_from_the_user_message() {
        let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
        drop(sender);
        let source = receiver.blocking_recv().unwrap_err();
        let diagnostic = source.to_string();

        let error = AppError::from(source);

        assert_eq!(error.code, AppErrorCode::Internal);
        assert_eq!(error.message, "后台任务通信失败");
        assert_eq!(error.detail.as_deref(), Some(diagnostic.as_str()));
    }
}
