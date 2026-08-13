use crate::config::PRIMARY_LLM_ENTRY_ID;
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

/// 降级链中每个服务的密钥独立存储。主用服务沿用旧条目名，
/// 老用户升级后无需重新填写密钥；备用服务按标识各存一条。
pub fn keyring_user_for_entry(entry_id: &str) -> String {
    if entry_id == PRIMARY_LLM_ENTRY_ID {
        KEYRING_USER.to_string()
    } else {
        format!("{KEYRING_USER}:{entry_id}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyringCredentialBackend {
    user: String,
}

impl Default for KeyringCredentialBackend {
    fn default() -> Self {
        Self {
            user: KEYRING_USER.to_string(),
        }
    }
}

impl KeyringCredentialBackend {
    pub fn for_entry(entry_id: &str) -> Self {
        Self {
            user: keyring_user_for_entry(entry_id),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, AppError> {
        keyring::Entry::new(KEYRING_SERVICE, &self.user).map_err(AppError::from)
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
    resolve_for_entry(PRIMARY_LLM_ENTRY_ID)
}

pub fn status() -> Result<CredentialStatus, AppError> {
    status_for_entry(PRIMARY_LLM_ENTRY_ID)
}

pub fn set(secret: &str) -> Result<(), AppError> {
    set_for_entry(PRIMARY_LLM_ENTRY_ID, secret)
}

pub fn delete() -> Result<(), AppError> {
    delete_for_entry(PRIMARY_LLM_ENTRY_ID)
}

/// 环境变量兜底只对主用服务生效：备用服务的标识是前端生成的随机串，
/// 让用户去猜一个带随机串的环境变量名没有任何意义。
fn environment_for_entry(entry_id: &str) -> Option<String> {
    if entry_id == PRIMARY_LLM_ENTRY_ID {
        std::env::var(ENVIRONMENT_VARIABLE).ok()
    } else {
        None
    }
}

pub fn resolve_for_entry(entry_id: &str) -> Result<ResolvedCredential, AppError> {
    let environment = environment_for_entry(entry_id);
    resolve_with_environment(
        &KeyringCredentialBackend::for_entry(entry_id),
        environment.as_deref(),
    )
}

pub fn status_for_entry(entry_id: &str) -> Result<CredentialStatus, AppError> {
    let environment = environment_for_entry(entry_id);
    status_with_environment(
        &KeyringCredentialBackend::for_entry(entry_id),
        environment.as_deref(),
    )
}

pub fn set_for_entry(entry_id: &str, secret: &str) -> Result<(), AppError> {
    set_with_backend(&KeyringCredentialBackend::for_entry(entry_id), secret)
}

pub fn delete_for_entry(entry_id: &str) -> Result<(), AppError> {
    delete_with_backend(&KeyringCredentialBackend::for_entry(entry_id))
}

/// 互换两个服务已保存的密钥。
///
/// 降级链上调整主用/备用顺序时，密钥必须跟着各自的服务一起走。
/// 由于密钥明文不允许离开 Rust（`ResolvedCredential` 刻意不实现 `Serialize`），
/// 这件事只能在这里整体完成，不能拆成前端读取再写回。
pub fn swap_entries(entry_a: &str, entry_b: &str) -> Result<(), AppError> {
    if entry_a == entry_b {
        return Ok(());
    }
    swap_with_backends(
        &KeyringCredentialBackend::for_entry(entry_a),
        &KeyringCredentialBackend::for_entry(entry_b),
    )
}

pub fn swap_with_backends<A: CredentialBackend + ?Sized, B: CredentialBackend + ?Sized>(
    backend_a: &A,
    backend_b: &B,
) -> Result<(), AppError> {
    // 只搬运存放在系统凭证里的密钥。环境变量兜底得到的值不属于任何一个条目，
    // 把它写进 keyring 会凭空生成一份用户从未保存过的副本。
    let secret_a = backend_a.get()?;
    let secret_b = backend_b.get()?;

    if secret_a.is_none() && secret_b.is_none() {
        return Ok(());
    }

    apply_secret(backend_a, secret_b.as_deref())?;

    // 第二步失败时把第一步改回去，避免出现「A 拿到了 B 的密钥，B 的还在原地」
    // 这种两个服务共用同一把密钥的中间态。
    if let Err(error) = apply_secret(backend_b, secret_a.as_deref()) {
        let _ = apply_secret(backend_a, secret_a.as_deref());
        return Err(error);
    }

    Ok(())
}

fn apply_secret<B: CredentialBackend + ?Sized>(
    backend: &B,
    secret: Option<&str>,
) -> Result<(), AppError> {
    match secret {
        Some(secret) => backend.set(secret),
        None => backend.delete(),
    }
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
mod entry_tests {
    use super::*;

    #[test]
    fn primary_entry_keeps_the_legacy_keyring_user() {
        // 老用户升级后不应被要求重新填写主用服务的密钥
        assert_eq!(keyring_user_for_entry(PRIMARY_LLM_ENTRY_ID), KEYRING_USER);
    }

    #[test]
    fn fallback_entries_get_their_own_keyring_user() {
        assert_eq!(
            keyring_user_for_entry("backup-a"),
            format!("{KEYRING_USER}:backup-a")
        );
        assert_ne!(
            keyring_user_for_entry("backup-a"),
            keyring_user_for_entry("backup-b")
        );
    }

    #[test]
    fn environment_fallback_only_applies_to_the_primary_entry() {
        assert!(
            KeyringCredentialBackend::for_entry(PRIMARY_LLM_ENTRY_ID)
                == KeyringCredentialBackend::default()
        );
        assert_ne!(
            KeyringCredentialBackend::for_entry("backup-a"),
            KeyringCredentialBackend::default()
        );
    }
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

    struct FailingSetBackend {
        value: RefCell<Option<String>>,
    }

    impl CredentialBackend for FailingSetBackend {
        fn get(&self) -> Result<Option<String>, AppError> {
            Ok(self.value.borrow().clone())
        }

        fn set(&self, _secret: &str) -> Result<(), AppError> {
            Err(AppError::credential("写入凭证失败"))
        }

        fn delete(&self) -> Result<(), AppError> {
            Err(AppError::credential("删除凭证失败"))
        }
    }

    #[test]
    fn swapping_moves_each_secret_to_the_other_entry() {
        let a = FakeBackend::with_value(Some("secret-a"));
        let b = FakeBackend::with_value(Some("secret-b"));

        swap_with_backends(&a, &b).unwrap();

        assert_eq!(a.value.borrow().as_deref(), Some("secret-b"));
        assert_eq!(b.value.borrow().as_deref(), Some("secret-a"));
    }

    #[test]
    fn swapping_with_one_side_unset_moves_the_absence_too() {
        let a = FakeBackend::with_value(Some("secret-a"));
        let b = FakeBackend::with_value(None);

        swap_with_backends(&a, &b).unwrap();

        // 未配置密钥的一侧不能凭空得到一份，否则两个服务会共用同一把密钥
        assert_eq!(a.value.borrow().as_deref(), None);
        assert_eq!(b.value.borrow().as_deref(), Some("secret-a"));
    }

    #[test]
    fn swapping_two_empty_entries_is_a_noop() {
        let a = FakeBackend::with_value(None);
        let b = FakeBackend::with_value(None);

        swap_with_backends(&a, &b).unwrap();

        assert_eq!(a.value.borrow().as_deref(), None);
        assert_eq!(b.value.borrow().as_deref(), None);
    }

    #[test]
    fn swapping_rolls_back_when_the_second_write_fails() {
        let a = FakeBackend::with_value(Some("secret-a"));
        let b = FailingSetBackend {
            value: RefCell::new(Some("secret-b".to_string())),
        };

        let error = swap_with_backends(&a, &b).unwrap_err();

        assert!(error.message.contains("凭证"));
        // A 必须被还原成自己的密钥，不能停在「持有 B 的密钥」这种中间态
        assert_eq!(a.value.borrow().as_deref(), Some("secret-a"));
        assert_eq!(b.value.borrow().as_deref(), Some("secret-b"));
    }

    #[test]
    fn read_failure_aborts_before_touching_anything() {
        let a = FakeBackend::with_get_error("读取失败");
        let b = FakeBackend::with_value(Some("secret-b"));

        assert!(swap_with_backends(&a, &b).is_err());
        assert_eq!(b.value.borrow().as_deref(), Some("secret-b"));
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
