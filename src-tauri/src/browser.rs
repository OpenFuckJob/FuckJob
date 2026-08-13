#[cfg(test)]
use std::net::TcpListener;
use std::{
    fs,
    future::Future,
    path::PathBuf,
    pin::Pin,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex, RwLock,
    },
    time::Duration,
};

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use rust_drission::{stealth_inject, ChromiumPage, Page};
use serde::{Deserialize, Serialize};
use tauri::Manager;

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
static ACTIVE_DEBUG_PORT: Lazy<RwLock<Option<u16>>> = Lazy::new(|| RwLock::new(None));
/// Serializes the check-and-launch sequence. `BROWSER_SESSION` also protects its
/// own state, but this lock additionally closes the race between ensuring the
/// managed process and establishing a task-scoped CDP connection.
static MANAGED_BROWSER_START_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static ACTIVE_TASK_BROWSER_LEASES: AtomicUsize = AtomicUsize::new(0);

const DEFAULT_RPA_DEBUG_PORT: u16 = 9876;
const BROWSER_START_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedBrowserRecord {
    pid: u32,
    port: u16,
    user_data_dir: String,
}

enum BrowserSession {
    Empty,
    Ready(ChromiumPage),
    InUse { close_requested: bool },
}

/// Tabs created through a task lease. Keeping ownership explicit is important:
/// taking a before/after snapshot of all Chrome tabs is racy when two tasks
/// create tabs at the same time.
struct TaskOwnedTabs<T> {
    tabs: Vec<T>,
}

impl<T> Default for TaskOwnedTabs<T> {
    fn default() -> Self {
        Self { tabs: Vec::new() }
    }
}

impl<T> TaskOwnedTabs<T> {
    fn register(&mut self, tab: T) {
        self.tabs.push(tab);
    }

    fn drain(&mut self) -> Vec<T> {
        std::mem::take(&mut self.tabs)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.tabs.len()
    }
}

/// A task-isolated CDP connection and the tabs created through it.
///
/// The underlying `ChromiumPage` is deliberately only exposed by shared
/// reference. This prevents task code from calling `close_browser()`, while
/// still allowing it to create listeners and use browser-level read APIs.
pub struct TaskBrowserLease {
    browser: ChromiumPage,
    main_tab: Page,
    owned_tabs: Mutex<TaskOwnedTabs<Page>>,
    counted_active: bool,
}

impl TaskBrowserLease {
    /// The task's dedicated initial tab.
    pub fn tab(&self) -> &Page {
        &self.main_tab
    }

    /// The task's independent CDP connection. Only a shared reference is
    /// returned, so this lease cannot terminate the managed Chrome process.
    pub fn browser(&self) -> &ChromiumPage {
        &self.browser
    }

    /// Create another stealth tab owned by this task. It will be closed along
    /// with the main tab when the lease finishes.
    pub fn new_stealth_tab(&self) -> Result<Page> {
        let tab = new_stealth_tab(&self.browser)?;
        self.owned_tabs
            .lock()
            .map_err(|e| anyhow!("获取任务标签页锁失败: {}", e))?
            .register(tab.clone());
        Ok(tab)
    }

    fn close_owned_tabs(&self) -> Result<()> {
        let tabs = self
            .owned_tabs
            .lock()
            .map_err(|e| anyhow!("获取任务标签页锁失败: {}", e))?
            .drain();
        close_task_owned_tabs(tabs)
    }
}

impl Drop for TaskBrowserLease {
    fn drop(&mut self) {
        // Best-effort fallback for cancellation/panic paths. `with_task_browser`
        // performs the same cleanup explicitly so it can report an error.
        let _ = self.close_owned_tabs();
        if self.counted_active {
            ACTIVE_TASK_BROWSER_LEASES.fetch_sub(1, Ordering::AcqRel);
            self.counted_active = false;
        }
    }
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
        BrowserSession::Ready(browser) => {
            if is_browser_session_usable(browser) {
                return Ok(());
            } else {
                *session = BrowserSession::Empty;
            }
        }
        BrowserSession::InUse { .. } => {
            // A legacy `with_browser` caller currently owns the anchor
            // connection. Task-scoped callers can still attach through CDP;
            // never reset the anchor here because that would race its later
            // restore and could launch/attach a second session unnecessarily.
            let active_port = ACTIVE_DEBUG_PORT
                .read()
                .ok()
                .and_then(|port| *port)
                .is_some_and(is_cdp_port_active);
            if active_port {
                return Ok(());
            }
            return Err(anyhow!("浏览器会话正在初始化或使用中，请稍后重试"));
        }
        BrowserSession::Empty => {}
    }

    let previous_session = std::mem::replace(
        &mut *session,
        BrowserSession::InUse {
            close_requested: false,
        },
    );
    let previous_session = match previous_session {
        BrowserSession::Ready(mut browser) => {
            if !is_browser_session_usable(&browser) {
                browser.close_browser();
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
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

fn is_cdp_port_active(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(300),
    )
    .is_ok()
}

fn should_connect_to_existing_browser(cdp_port_active: bool) -> bool {
    cdp_port_active
}

fn cdp_connection_requires_stealth_injection() -> bool {
    true
}

fn managed_browser_can_be_reused(
    record: &ManagedBrowserRecord,
    config: &BrowserConfig,
    cdp_port_active: bool,
) -> bool {
    cdp_port_active && record.user_data_dir == config.user_data_dir
}

fn managed_browser_record_path() -> Result<PathBuf> {
    let app_handle = app_handle().ok_or_else(|| anyhow!("应用句柄尚未初始化"))?;
    Ok(app_handle.path().app_data_dir()?.join("rpa-browser.json"))
}

fn load_managed_browser_record() -> Option<ManagedBrowserRecord> {
    let path = managed_browser_record_path().ok()?;
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_managed_browser_record(record: &ManagedBrowserRecord) -> Result<()> {
    let path = managed_browser_record_path()?;
    let content = serde_json::to_vec(record)?;
    fs::write(path, content)?;
    Ok(())
}

fn browser_executable(config: &BrowserConfig) -> Result<PathBuf> {
    if let Some(path) = config
        .chrome_exe_path
        .as_ref()
        .filter(|path| !path.trim().is_empty())
    {
        return Ok(PathBuf::from(path));
    }
    detect_browser_path()
        .map(|(_, path)| path)
        .ok_or_else(|| anyhow!("未找到可用的 Chrome 或 Edge"))
}

fn connect_to_browser(port: u16) -> Result<ChromiumPage> {
    let endpoint = format!("127.0.0.1:{port}");
    let browser = ChromiumPage::connect(&endpoint)?;
    // ChromiumPage::connect() 不会像 ChromiumPage::new() 一样自动注入。
    // 必须在任何 BOSS 页面导航前注册新文档脚本，避免首个页面加载暴露自动化特征。
    if cdp_connection_requires_stealth_injection() {
        stealth_inject(browser.tab())?;
    }
    if is_browser_session_usable(&browser) {
        Ok(browser)
    } else {
        Err(anyhow!("浏览器调试端口 {port} 的会话不可用"))
    }
}

/// Establish a connection for one task without touching the tab selected by
/// `ChromiumPage::connect`. The caller immediately creates its own tab below.
fn connect_task_browser(port: u16) -> Result<ChromiumPage> {
    let endpoint = format!("127.0.0.1:{port}");
    let browser = ChromiumPage::connect(&endpoint)?;
    if is_browser_session_usable(&browser) {
        Ok(browser)
    } else {
        Err(anyhow!("浏览器调试端口 {port} 的任务连接不可用"))
    }
}

fn launch_managed_browser(config: &BrowserConfig, port: u16) -> Result<ChromiumPage> {
    fs::create_dir_all(&config.user_data_dir)?;
    let executable = browser_executable(config)?;
    let mut child = Command::new(executable)
        .args([
            format!("--remote-debugging-port={port}"),
            format!("--user-data-dir={}", config.user_data_dir),
            "--window-size=1920,1080".to_string(),
            "--no-default-browser-check".to_string(),
            "--disable-suggestions-ui".to_string(),
            "--no-first-run".to_string(),
            "--disable-infobars".to_string(),
            "--disable-popup-blocking".to_string(),
            "--hide-crash-restore-bubble".to_string(),
            "--disable-features=PrivacySandboxSettings4".to_string(),
            "--disable-blink-features=AutomationControlled".to_string(),
            "--no-sandbox".to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    for _ in 0..BROWSER_START_ATTEMPTS * 20 {
        if should_connect_to_existing_browser(is_cdp_port_active(port)) {
            if let Ok(browser) = connect_to_browser(port) {
                save_managed_browser_record(&ManagedBrowserRecord {
                    pid: child.id(),
                    port,
                    user_data_dir: config.user_data_dir.clone(),
                })?;
                return Ok(browser);
            }
        }
        if child.try_wait()?.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let _ = child.kill();
    Err(anyhow!("Chrome 启动后未能建立 CDP 连接"))
}

fn create_browser(config: &BrowserConfig) -> Result<ChromiumPage> {
    // 候选端口列表：优先检查已记录的活跃端口、默认固定端口(9876)及附近备用端口
    let mut candidate_ports = Vec::new();
    if let Ok(guard) = ACTIVE_DEBUG_PORT.read() {
        if let Some(port) = *guard {
            candidate_ports.push(port);
        }
    }
    if !candidate_ports.contains(&DEFAULT_RPA_DEBUG_PORT) {
        candidate_ports.push(DEFAULT_RPA_DEBUG_PORT);
    }

    let managed_record = load_managed_browser_record();

    // 1. 端口已被占用时必须只连接已有 Chrome，不能用 ChromiumPage::new()
    // 再次 launch 会删除同一 user-data-dir 的 Singleton 锁并启动第二个 Chrome，
    // 导致 macOS 上出现“打开您的个人资料时出了点问题”。即使窗口被用户关闭，
    // 只要 CDP 进程仍在，ChromiumPage::connect 会复用它并按需新建标签页。
    for &port in &candidate_ports {
        if should_connect_to_existing_browser(is_cdp_port_active(port)) {
            let record_matches = managed_record
                .as_ref()
                .map(|record| managed_browser_can_be_reused(record, config, true))
                .unwrap_or(port == DEFAULT_RPA_DEBUG_PORT);
            if !record_matches {
                return Err(anyhow!(
                    "检测到非当前 profile 的浏览器调试端口 {port}，已拒绝启动或接管浏览器"
                ));
            }
            match connect_to_browser(port) {
                Ok(browser) => {
                    if let Ok(mut guard) = ACTIVE_DEBUG_PORT.write() {
                        *guard = Some(port);
                    }
                    if managed_record.is_none() {
                        let _ = save_managed_browser_record(&ManagedBrowserRecord {
                            pid: 0,
                            port,
                            user_data_dir: config.user_data_dir.clone(),
                        });
                    }
                    return Ok(browser);
                }
                Err(error) => return Err(anyhow!("检测到浏览器调试端口 {port}，但无法连接：{error}。已拒绝启动第二个浏览器以保护个人资料")),
            }
        }
    }

    // 2. 没有活动 CDP 实例时，由应用直接启动并记录 PID。不能交给
    // rust_drission::ChromiumPage::new()，它会删除 profile 锁文件。
    let target_port = DEFAULT_RPA_DEBUG_PORT;
    let browser = launch_managed_browser(config, target_port)?;
    if let Ok(mut guard) = ACTIVE_DEBUG_PORT.write() {
        *guard = Some(target_port);
    }
    Ok(browser)
}

#[cfg(test)]
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

#[cfg(test)]
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

/// Run one RPA task on an independent CDP connection and an independently
/// owned main tab.
///
/// Calls may run concurrently. Chrome/profile startup is serialized, but the
/// lock is released before the task begins. Cleanup closes only tabs registered
/// to this lease; it never closes Chrome or tabs belonging to another task.
pub async fn with_task_browser<T, F>(op: F) -> Result<T>
where
    F: for<'a> FnOnce(&'a ChromiumPage, &'a Page) -> Pin<Box<dyn Future<Output = Result<T>> + 'a>>,
{
    let config = load_browser_config()?;
    let start_guard = MANAGED_BROWSER_START_LOCK
        .lock()
        .map_err(|e| anyhow!("获取浏览器启动锁失败: {}", e))?;
    let port = ensure_managed_browser_for_task(&config)?;
    // Count the task while still holding the lifecycle lock. This closes the
    // gap in which an explicit browser-close request could otherwise arrive
    // after startup but before the lease became visible.
    ACTIVE_TASK_BROWSER_LEASES.fetch_add(1, Ordering::AcqRel);
    drop(start_guard);

    let browser = match connect_task_browser(port) {
        Ok(browser) => browser,
        Err(error) => {
            ACTIVE_TASK_BROWSER_LEASES.fetch_sub(1, Ordering::AcqRel);
            return Err(error);
        }
    };
    let main_tab = match new_stealth_tab(&browser) {
        Ok(tab) => tab,
        Err(error) => {
            ACTIVE_TASK_BROWSER_LEASES.fetch_sub(1, Ordering::AcqRel);
            return Err(error);
        }
    };
    let mut owned_tabs = TaskOwnedTabs::default();
    owned_tabs.register(main_tab.clone());

    let lease = TaskBrowserLease {
        browser,
        main_tab,
        owned_tabs: Mutex::new(owned_tabs),
        counted_active: true,
    };
    let result = op(lease.browser(), lease.tab()).await;
    // Closing a tab after its renderer/browser has already exited is benign
    // for task cleanup. Never turn an otherwise successful RPA task into a
    // failure solely because best-effort teardown could not close a dead tab.
    let _ = lease.close_owned_tabs();

    result
}

fn ensure_managed_browser_for_task(config: &BrowserConfig) -> Result<u16> {
    // The legacy session owns no task connection. It remains as the process
    // health anchor for environment/login APIs and also serializes launch with
    // callers that have not migrated to task leases yet.
    init_browser_session(config)?;

    let port = ACTIVE_DEBUG_PORT
        .read()
        .map_err(|e| anyhow!("获取浏览器调试端口读锁失败: {}", e))?
        .ok_or_else(|| anyhow!("浏览器已初始化但调试端口未知"))?;
    if !is_cdp_port_active(port) {
        return Err(anyhow!("浏览器调试端口 {port} 不可用"));
    }
    Ok(port)
}

fn close_task_owned_tabs(tabs: Vec<Page>) -> Result<()> {
    close_owned_items(tabs, |tab| tab.close().map_err(anyhow::Error::from))
}

/// Close every owned resource even when one close fails. Returning the first
/// error preserves useful diagnostics without leaking the remaining tabs.
fn close_owned_items<T, F>(items: Vec<T>, mut close: F) -> Result<()>
where
    F: FnMut(T) -> Result<()>,
{
    let mut first_error = None;
    for item in items {
        if let Err(error) = close(item) {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
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
    let task_leases_active = ACTIVE_TASK_BROWSER_LEASES.load(Ordering::Acquire) > 0;
    *session = if !task_leases_active && close_requested {
        browser.close_browser();
        BrowserSession::Empty
    } else if error.is_some_and(|err| !reuse_browser_session_after_error(err)) {
        // Discard the broken CDP connection, but do not terminate the managed
        // Chrome process: another task may be between connection setup and
        // lease publication. The next initialization can reconnect by port.
        BrowserSession::Empty
    } else {
        BrowserSession::Ready(browser)
    };

    Ok(())
}

pub fn close_browser_session() -> Result<()> {
    let _lifecycle_guard = MANAGED_BROWSER_START_LOCK
        .lock()
        .map_err(|e| anyhow!("获取浏览器生命周期锁失败: {}", e))?;
    if ACTIVE_TASK_BROWSER_LEASES.load(Ordering::Acquire) > 0 {
        return Err(anyhow!("仍有自动化任务正在使用浏览器，暂不能关闭"));
    }
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

/// 创建一个尚未导航的标签，并在首个文档加载前注册反检测脚本。
///
/// 不使用 `ChromiumPage::new_tab()` 的隐式行为，避免调用方在“网页点击后才
/// 创建的标签”上错过首个页面加载时机。
pub fn new_stealth_tab(browser: &ChromiumPage) -> Result<Page> {
    let tab = browser.new_tab_without_stealth(None)?;
    stealth_inject(&tab)?;
    Ok(tab)
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
        new_stealth_tab(browser)?
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

    #[test]
    fn active_cdp_port_uses_connection_instead_of_launching_a_second_browser() {
        assert!(should_connect_to_existing_browser(true));
        assert!(!should_connect_to_existing_browser(false));
    }

    #[test]
    fn live_managed_browser_is_reused_even_when_no_page_is_open() {
        let config = BrowserConfig {
            user_data_dir: "/tmp/offer-flow-profile".to_string(),
            chrome_exe_path: None,
            max_parallel_tasks: 2,
        };
        let record = ManagedBrowserRecord {
            pid: 123,
            port: DEFAULT_RPA_DEBUG_PORT,
            user_data_dir: config.user_data_dir.clone(),
        };

        assert!(managed_browser_can_be_reused(&record, &config, true));
        assert!(!managed_browser_can_be_reused(&record, &config, false));
    }

    #[test]
    fn cdp_connections_install_stealth_before_the_next_navigation() {
        assert!(cdp_connection_requires_stealth_injection());
    }

    #[test]
    fn task_owned_tabs_track_only_explicitly_registered_tabs() {
        let mut owned = TaskOwnedTabs::default();
        owned.register("task-main");
        owned.register("task-child");

        assert_eq!(owned.len(), 2);
        assert_eq!(owned.drain(), vec!["task-main", "task-child"]);
        assert_eq!(owned.len(), 0);
    }

    #[test]
    fn task_cleanup_attempts_every_owned_tab_and_reports_first_error() {
        let mut closed = Vec::new();
        let result = close_owned_items(vec![1, 2, 3], |tab_id| {
            closed.push(tab_id);
            if tab_id == 2 {
                Err(anyhow!("tab 2 close failed"))
            } else {
                Ok(())
            }
        });

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "tab 2 close failed");
        assert_eq!(closed, vec![1, 2, 3]);
    }

    #[test]
    fn task_cleanup_with_no_owned_tabs_is_a_noop() {
        let mut close_called = false;
        let result = close_owned_items(Vec::<u8>::new(), |_| {
            close_called = true;
            Ok(())
        });

        assert!(result.is_ok());
        assert!(!close_called);
    }
}
