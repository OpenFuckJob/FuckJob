use crate::error::AppError;
use serde::{Deserialize, Serialize};

pub const KEYRING_SERVICE: &str = "fuck_job";
pub const KEYRING_USER: &str = "llm_api_key";
pub const ENVIRONMENT_VARIABLE: &str = "FUCKJOB_LLM_API_KEY";

pub trait CredentialBackend {
    fn get(&self) -> Result<Option<String>, AppError>;
    fn set(&self, secret: &str) -> Result<(), AppError>;
    fn delete(&self) -> Result<(), AppError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectiveCredentialSource {
    Keychain,
    Environment,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialStatus {
    pub configured: bool,
    pub source: EffectiveCredentialSource,
}

/// This type intentionally does not implement `Serialize`: credential values
/// are for trusted Rust call sites only and can never be returned by Tauri.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCredential {
    source: EffectiveCredentialSource,
    secret: Option<String>,
}

impl ResolvedCredential {
    pub fn source(&self) -> EffectiveCredentialSource {
        self.source
    }

    pub fn secret(&self) -> Option<&str> {
        self.secret.as_deref()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringCredentialBackend;

impl KeyringCredentialBackend {
    fn entry(&self) -> Result<keyring::Entry, AppError> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(AppError::from)
    }
}

impl CredentialBackend for KeyringCredentialBackend {
    fn get(&self) -> Result<Option<String>, AppError> {
        match self.entry()?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(AppError::from(error)),
        }
    }

    fn set(&self, secret: &str) -> Result<(), AppError> {
        self.entry()?.set_password(secret).map_err(AppError::from)
    }

    fn delete(&self) -> Result<(), AppError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(AppError::from(error)),
        }
    }
}

pub fn resolve() -> Result<ResolvedCredential, AppError> {
    let environment = std::env::var(ENVIRONMENT_VARIABLE).ok();
    resolve_with_environment(&KeyringCredentialBackend, environment.as_deref())
}

pub fn status() -> Result<CredentialStatus, AppError> {
    let environment = std::env::var(ENVIRONMENT_VARIABLE).ok();
    status_with_environment(&KeyringCredentialBackend, environment.as_deref())
}

pub fn set(secret: &str) -> Result<(), AppError> {
    set_with_backend(&KeyringCredentialBackend, secret)
}

pub fn delete() -> Result<(), AppError> {
    delete_with_backend(&KeyringCredentialBackend)
}

pub fn resolve_with_environment<B: CredentialBackend + ?Sized>(
    backend: &B,
    environment: Option<&str>,
) -> Result<ResolvedCredential, AppError> {
    let environment = environment.and_then(|value| normalized_secret(value.to_string()));
    match backend.get() {
        Ok(Some(value)) => {
            if let Some(secret) = normalized_secret(value) {
                return Ok(ResolvedCredential {
                    source: EffectiveCredentialSource::Keychain,
                    secret: Some(secret),
                });
            }
        }
        Ok(None) => {}
        Err(error) => {
            if let Some(secret) = environment {
                return Ok(ResolvedCredential {
                    source: EffectiveCredentialSource::Environment,
                    secret: Some(secret),
                });
            }
            return Err(AppError::credential(format!(
                "无法读取系统凭证，请配置环境变量 {ENVIRONMENT_VARIABLE} 后重试"
            ))
            .with_detail(error.detail.unwrap_or(error.message)));
        }
    }

    if let Some(secret) = environment {
        return Ok(ResolvedCredential {
            source: EffectiveCredentialSource::Environment,
            secret: Some(secret),
        });
    }

    Ok(ResolvedCredential {
        source: EffectiveCredentialSource::None,
        secret: None,
    })
}

pub fn status_with_environment<B: CredentialBackend + ?Sized>(
    backend: &B,
    environment: Option<&str>,
) -> Result<CredentialStatus, AppError> {
    let resolved = resolve_with_environment(backend, environment)?;
    Ok(CredentialStatus {
        configured: resolved.secret.is_some(),
        source: resolved.source,
    })
}

pub fn set_with_backend<B: CredentialBackend + ?Sized>(
    backend: &B,
    secret: &str,
) -> Result<(), AppError> {
    let secret = normalized_secret(secret.to_string())
        .ok_or_else(|| AppError::validation("大模型密钥不能为空"))?;
    backend.set(&secret)
}

pub fn delete_with_backend<B: CredentialBackend + ?Sized>(backend: &B) -> Result<(), AppError> {
    backend.delete()
}

fn normalized_secret(secret: String) -> Option<String> {
    let trimmed = secret.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeBackend {
        value: RefCell<Option<String>>,
        get_error: Option<AppError>,
    }

    impl FakeBackend {
        fn with_value(value: Option<&str>) -> Self {
            Self {
                value: RefCell::new(value.map(str::to_string)),
                get_error: None,
            }
        }

        fn with_get_error(detail: &str) -> Self {
            Self {
                value: RefCell::new(None),
                get_error: Some(AppError::credential("凭证操作失败").with_detail(detail)),
            }
        }
    }

    impl CredentialBackend for FakeBackend {
        fn get(&self) -> Result<Option<String>, AppError> {
            if let Some(error) = &self.get_error {
                return Err(error.clone());
            }
            Ok(self.value.borrow().clone())
        }

        fn set(&self, secret: &str) -> Result<(), AppError> {
            *self.value.borrow_mut() = Some(secret.to_string());
            Ok(())
        }

        fn delete(&self) -> Result<(), AppError> {
            *self.value.borrow_mut() = None;
            Ok(())
        }
    }

    #[test]
    fn keychain_takes_precedence_over_environment() {
        let backend = FakeBackend::with_value(Some("keychain-secret"));

        let resolved = resolve_with_environment(&backend, Some("environment-secret")).unwrap();

        assert_eq!(resolved.source(), EffectiveCredentialSource::Keychain);
        assert_eq!(resolved.secret(), Some("keychain-secret"));
    }

    #[test]
    fn blank_keychain_value_is_ignored_and_environment_is_used() {
        for blank in ["", "  \n "] {
            let backend = FakeBackend::with_value(Some(blank));

            let resolved = resolve_with_environment(&backend, Some("environment-secret")).unwrap();

            assert_eq!(resolved.source(), EffectiveCredentialSource::Environment);
            assert_eq!(resolved.secret(), Some("environment-secret"));
        }
    }

    #[test]
    fn environment_fallback_ignores_blank_values() {
        let backend = FakeBackend::default();

        let environment = resolve_with_environment(&backend, Some(" env-secret ")).unwrap();
        let blank = resolve_with_environment(&backend, Some("   ")).unwrap();

        assert_eq!(environment.source(), EffectiveCredentialSource::Environment);
        assert_eq!(environment.secret(), Some("env-secret"));
        assert_eq!(blank.source(), EffectiveCredentialSource::None);
        assert_eq!(blank.secret(), None);
    }

    #[test]
    fn keychain_read_error_falls_back_to_nonblank_environment_and_status() {
        let backend = FakeBackend::with_get_error("secret backend diagnostic");

        let resolved = resolve_with_environment(&backend, Some(" env-secret ")).unwrap();
        let status = status_with_environment(&backend, Some("env-secret")).unwrap();

        assert_eq!(resolved.source(), EffectiveCredentialSource::Environment);
        assert_eq!(resolved.secret(), Some("env-secret"));
        assert_eq!(
            status,
            CredentialStatus {
                configured: true,
                source: EffectiveCredentialSource::Environment,
            }
        );
    }

    #[test]
    fn keychain_read_error_without_environment_explains_environment_fallback() {
        let backend = FakeBackend::with_get_error("secret backend diagnostic");

        for environment in [None, Some(" \n ")] {
            let error = resolve_with_environment(&backend, environment).unwrap_err();
            assert_eq!(error.code, crate::error::AppErrorCode::Credential);
            assert!(error.message.contains(ENVIRONMENT_VARIABLE));
            assert_eq!(error.detail.as_deref(), Some("secret backend diagnostic"));
        }
    }

    #[test]
    fn set_and_delete_update_effective_status() {
        let backend = FakeBackend::default();

        set_with_backend(&backend, "  saved-secret  ").unwrap();
        assert_eq!(
            status_with_environment(&backend, None).unwrap(),
            CredentialStatus {
                configured: true,
                source: EffectiveCredentialSource::Keychain,
            }
        );

        delete_with_backend(&backend).unwrap();
        assert_eq!(
            status_with_environment(&backend, Some("env-secret")).unwrap(),
            CredentialStatus {
                configured: true,
                source: EffectiveCredentialSource::Environment,
            }
        );
    }

    #[test]
    fn blank_secret_cannot_be_saved() {
        let backend = FakeBackend::default();

        let error = set_with_backend(&backend, " \n ").unwrap_err();

        assert_eq!(error.code, crate::error::AppErrorCode::Validation);
        assert!(backend.value.borrow().is_none());
    }

    #[test]
    fn serialized_status_never_contains_the_secret() {
        let backend = FakeBackend::with_value(Some("never-serialize-me"));
        let status = status_with_environment(&backend, None).unwrap();

        let serialized = serde_json::to_string(&status).unwrap();

        assert_eq!(serialized, r#"{"configured":true,"source":"keychain"}"#);
        assert!(!serialized.contains("never-serialize-me"));
    }

    #[test]
    fn keyring_identifiers_and_environment_name_are_stable() {
        assert_eq!(KEYRING_SERVICE, "fuck_job");
        assert_eq!(KEYRING_USER, "llm_api_key");
        assert_eq!(ENVIRONMENT_VARIABLE, "FUCKJOB_LLM_API_KEY");
    }
}
