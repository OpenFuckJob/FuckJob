use std::{future::Future, net::TcpListener, path::PathBuf, pin::Pin, sync::RwLock};

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use rust_drission::{ChromiumPage, Page};
use serde::Serialize;

use crate::config::{self, BrowserConfig};

// ================================
// 浏览器环境检测
// ================================

#[derive(Serialize, Debug, Clone)]
pub struct BrowserEnvStatus {
    pub browser_found: bool,
    pub browser_name: Option<String>,
    pub browser_path: Option<String>,
    pub user_data_dir_ok: bool,
    pub user_data_dir: Option<String>,
}

/// 检测系统中已安装的浏览器路径，优先级：Chrome > Edge
pub fn detect_browser_path() -> Option<(&'static str, PathBuf)> {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[
            (
                "Chrome",
                &["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"],
            ),
            (
                "Edge",
                &["/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"],
            ),
        ]
    } else if cfg!(target_os = "windows") {
        &[
            (
                "Chrome",
                &[
                    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
                ],
            ),
            (
                "Edge",
                &[
                    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
                    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                ],
            ),
        ]
    } else {
        &[
            (
                "Chrome",
                &[
                    "/usr/bin/google-chrome",
                    "/usr/bin/google-chrome-stable",
                    "/usr/bin/chromium-browser",
                    "/usr/bin/chromium",
                ],
            ),
            (
                "Edge",
                &["/usr/bin/microsoft-edge", "/usr/bin/microsoft-edge-stable"],
            ),
        ]
    };

    for (name, paths) in candidates {
        for path_str in *paths {
            let path = PathBuf::from(path_str);
            if path.exists() {
                return Some((*name, path));
            }
        }
    }
    None
}

pub fn check_browser_env_status(config: &BrowserConfig) -> BrowserEnvStatus {
    let detected = detect_browser_path();
    let has_explicit_path = config
        .chrome_exe_path
        .as_ref()
        .is_some_and(|p| !p.trim().is_empty() && p.trim() != "null" && p.trim() != "None");

    let (browser_found, browser_name, browser_path) = if has_explicit_path {
        let path = config.chrome_exe_path.as_ref().unwrap();
        (
            PathBuf::from(path).exists(),
            Some("自定义路径".to_string()),
            Some(path.clone()),
        )
    } else if let Some((name, path)) = detected {
        (
            true,
            Some(name.to_string()),
            Some(path.to_string_lossy().to_string()),
        )
    } else {
        (false, None, None)
    };

    let user_data_dir_ok = !config.user_data_dir.trim().is_empty()
        && config.user_data_dir.trim() != "null"
        && config.user_data_dir.trim() != "None";
    let user_data_dir = if user_data_dir_ok {
        Some(config.user_data_dir.clone())
    } else {
        None
    };

    BrowserEnvStatus {
        browser_found,
        browser_name,
        browser_path,
        user_data_dir_ok,
        user_data_dir,
    }
}

static BROWSER_SESSION: Lazy<RwLock<BrowserSession>> =
    Lazy::new(|| RwLock::new(BrowserSession::Empty));
static APP_HANDLE: Lazy<RwLock<Option<tauri::AppHandle>>> = Lazy::new(|| RwLock::new(None));

const BROWSER_START_ATTEMPTS: usize = 3;

enum BrowserSession {
    Empty,
    Ready(ChromiumPage),
    InUse { close_requested: bool },
}

pub fn init_app_handle(app_handle: tauri::AppHandle) -> Result<()> {
    let mut handle = APP_HANDLE
        .write()
        .map_err(|e| anyhow!("获取应用句柄写锁失败: {}", e))?;
    *handle = Some(app_handle);
    Ok(())
}

pub fn app_handle() -> Option<tauri::AppHandle> {
    APP_HANDLE.read().ok().and_then(|h| h.clone())
}

fn load_browser_config() -> Result<BrowserConfig> {
    let app_handle = APP_HANDLE
        .read()
        .map_err(|e| anyhow!("获取应用句柄读锁失败: {}", e))?
        .clone()
        .ok_or_else(|| anyhow!("应用句柄尚未初始化"))?;

    let config = config::load_app_config_inner(app_handle).map_err(|e| anyhow!(e))?;
    Ok(config.browser_config)
}

/// 初始化浏览器会话
pub fn init_browser_session(config: &BrowserConfig) -> Result<()> {
    let mut session = BROWSER_SESSION
        .write()
        .map_err(|e| anyhow!("获取浏览器会话写锁失败: {}", e))?;

    match &*session {
        BrowserSession::Ready(browser) if is_browser_session_usable(browser) => return Ok(()),
        BrowserSession::InUse { .. } => return Ok(()),
        _ => {}
    }

    let previous_session = std::mem::replace(
        &mut *session,
        BrowserSession::InUse {
            close_requested: false,
        },
    );
    let previous_session = match previous_session {
        BrowserSession::Ready(mut browser) => {
            browser.close_browser();
            BrowserSession::Empty
        }
        previous => previous,
    };

    let browser = match create_browser(config) {
        Ok(browser) => browser,
        Err(err) => {
            *session = previous_session;
            return Err(err);
        }
    };

    *session = BrowserSession::Ready(browser);

    Ok(())
}

fn create_browser(config: &BrowserConfig) -> Result<ChromiumPage> {
    let mut last_error = None;

    for attempt in 1..=BROWSER_START_ATTEMPTS {
        let port = allocate_browser_debug_port()?;
        let mut browser_config = rust_drission::BrowserConfig::new()
            .headless(false)
            .set_local_port(port)
            .user_data_dir(config.user_data_dir.clone());

        if let Some(chrome_exe_path) = config.chrome_exe_path.as_ref() {
            browser_config = browser_config.chrome_path(chrome_exe_path.clone());
        }

        match ChromiumPage::new(browser_config) {
            Ok(browser) => return Ok(browser),
            Err(error) if is_browser_start_retryable(&error.to_string()) => {
                last_error = Some(anyhow!(
                    "第 {attempt}/{BROWSER_START_ATTEMPTS} 次启动使用调试端口 {port} 失败：{error}"
                ));
            }
            Err(error) => return Err(error.into()),
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("浏览器启动失败")))
}

fn allocate_browser_debug_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| anyhow!("无法分配浏览器调试端口：{error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| anyhow!("无法读取浏览器调试端口：{error}"))?
        .port();
    drop(listener);
    Ok(port)
}

fn is_browser_start_retryable(error: &str) -> bool {
    error.contains("Chrome did not become ready within 5 seconds after launch")
}

fn is_browser_session_usable(browser: &ChromiumPage) -> bool {
    browser.url().is_ok()
}

pub async fn with_browser<T, F>(f: F) -> Result<T>
where
    F: for<'a> FnOnce(&'a ChromiumPage) -> Pin<Box<dyn Future<Output = Result<T>> + 'a>>,
{
    let config = load_browser_config()?;
    init_browser_session(&config)?;

    let browser = take_browser_session()?;

    let result = f(&browser).await;
    restore_browser_session(browser, result.as_ref().err())?;

    result
}

fn take_browser_session() -> Result<ChromiumPage> {
    let mut session = BROWSER_SESSION
        .write()
        .map_err(|e| anyhow!("获取浏览器会话写锁失败: {}", e))?;

    match std::mem::replace(
        &mut *session,
        BrowserSession::InUse {
            close_requested: false,
        },
    ) {
        BrowserSession::Ready(browser) => Ok(browser),
        BrowserSession::Empty => {
            *session = BrowserSession::Empty;
            Err(anyhow!("浏览器尚未初始化"))
        }
        BrowserSession::InUse { close_requested } => {
            *session = BrowserSession::InUse { close_requested };
            Err(anyhow!("浏览器正在使用中"))
        }
    }
}

fn restore_browser_session(mut browser: ChromiumPage, error: Option<&anyhow::Error>) -> Result<()> {
    let mut session = BROWSER_SESSION
        .write()
        .map_err(|e| anyhow!("获取浏览器会话写锁失败: {}", e))?;

    let close_requested = matches!(
        &*session,
        BrowserSession::InUse {
            close_requested: true
        }
    );
    *session =
        if close_requested || error.is_some_and(|err| !reuse_browser_session_after_error(err)) {
            browser.close_browser();
            BrowserSession::Empty
        } else {
            BrowserSession::Ready(browser)
        };

    Ok(())
}

pub fn close_browser_session() -> Result<()> {
    let mut session = BROWSER_SESSION
        .write()
        .map_err(|e| anyhow!("获取浏览器会话写锁失败: {}", e))?;

    match std::mem::replace(&mut *session, BrowserSession::Empty) {
        BrowserSession::Ready(mut browser) => {
            browser.close_browser();
        }
        BrowserSession::InUse { .. } => {
            *session = BrowserSession::InUse {
                close_requested: true,
            };
        }
        BrowserSession::Empty => {}
    }

    Ok(())
}

// 开启一个tab 执行op后 关闭tab
pub async fn with_new_tab<T, F>(op: F) -> Result<T>
where
    F: for<'a> FnOnce(&'a Page) -> Pin<Box<dyn Future<Output = Result<T>> + 'a>>,
{
    let config = load_browser_config()?;
    init_browser_session(&config)?;
    let tab = {
        let session = BROWSER_SESSION
            .read()
            .map_err(|e| anyhow!("获取浏览器会话读锁失败: {}", e))?;
        let BrowserSession::Ready(browser) = &*session else {
            return Err(anyhow!("浏览器尚未初始化"));
        };
        browser.new_tab(None)?
    };

    let result = op(&tab).await;
    let close_result = tab.close();

    match (result, close_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err.into()),
    }
}

fn reuse_browser_session_after_error(error: &anyhow::Error) -> bool {
    !is_browser_disconnected_error(error)
}

fn is_browser_disconnected_error(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("Connection closed")
        || message.contains("Trying to work with closed connection")
        || message.contains("Session with given id not found")
        || message.contains("code=-32001")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_connection_errors_discard_browser_session() {
        let error = anyhow!("Connection closed");

        assert!(!reuse_browser_session_after_error(&error));
    }

    #[test]
    fn closed_connection_errors_discard_browser_session() {
        let error = anyhow!("Trying to work with closed connection");

        assert!(!reuse_browser_session_after_error(&error));
    }

    #[test]
    fn stale_cdp_session_errors_discard_browser_session() {
        let error = anyhow!(
            "CDP error: id=Some(11), code=-32001, message=Session with given id not found."
        );

        assert!(!reuse_browser_session_after_error(&error));
    }

    #[test]
    fn normal_operation_errors_keep_browser_session() {
        let error = anyhow!("登录失败");

        assert!(reuse_browser_session_after_error(&error));
    }

    #[test]
    fn allocates_a_nonzero_local_debug_port() {
        let port = allocate_browser_debug_port().expect("allocate browser debug port");

        assert_ne!(port, 0);
        let _listener = TcpListener::bind(("127.0.0.1", port))
            .expect("allocated browser debug port should be available");
    }

    #[test]
    fn retries_only_browser_readiness_timeout() {
        assert!(is_browser_start_retryable(
            "HTTP request failed: Chrome did not become ready within 5 seconds after launch"
        ));
        assert!(!is_browser_start_retryable(
            "Failed to launch Chrome: No such file or directory"
        ));
    }
}
