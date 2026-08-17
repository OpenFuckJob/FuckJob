use std::{
    cell::RefCell,
    fs::OpenOptions,
    io::Write,
    marker::PhantomData,
    path::PathBuf,
    rc::Rc,
    sync::{Mutex, RwLock},
};

use anyhow::{anyhow, Result};
use chrono::Local;
use once_cell::sync::Lazy;
use tauri::Manager;

use crate::rpa::run_flow::PlatformKind;

static LOG_FILE_PATH: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));
static LOG_IO_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LogContext {
    pub task_id: Option<String>,
    pub platform: Option<PlatformKind>,
}

thread_local! {
    /// Logging context belongs to the task's dedicated worker thread. Keeping it
    /// thread-local prevents concurrent BOSS and Liepin workers from overwriting
    /// each other's labels.
    static CURRENT_CONTEXT: RefCell<LogContext> = RefCell::new(LogContext::default());
}

/// Restores the previous context when a scoped logging operation finishes.
/// The marker makes the guard non-Send so it cannot be dropped on another thread.
pub struct LogContextGuard {
    previous: Option<LogContext>,
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for LogContextGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            CURRENT_CONTEXT.with(|current| *current.borrow_mut() = previous);
        }
    }
}

pub fn init(app_handle: &tauri::AppHandle) -> Result<()> {
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| anyhow!("获取应用数据目录失败: {}", e))?;
    std::fs::create_dir_all(&data_dir)?;
    let log_path = data_dir.join("rpa.log");
    let mut path = LOG_FILE_PATH
        .write()
        .map_err(|e| anyhow!("获取日志路径写锁失败: {}", e))?;
    *path = Some(log_path);
    Ok(())
}

pub fn path() -> Result<PathBuf> {
    LOG_FILE_PATH
        .read()
        .map_err(|e| anyhow!("获取日志路径读锁失败: {}", e))?
        .clone()
        .ok_or_else(|| anyhow!("日志文件路径未初始化"))
}

pub fn clear() -> Result<()> {
    let _io = LOG_IO_LOCK
        .lock()
        .map_err(|e| anyhow!("获取日志文件锁失败: {}", e))?;
    let path = path()?;
    if path.exists() {
        std::fs::write(&path, "")?;
    }
    Ok(())
}

pub fn set_platform(platform: Option<PlatformKind>) -> Result<()> {
    CURRENT_CONTEXT.with(|current| current.borrow_mut().platform = platform);
    Ok(())
}

pub fn set_task_id(task_id: Option<String>) -> Result<()> {
    CURRENT_CONTEXT.with(|current| current.borrow_mut().task_id = task_id);
    Ok(())
}

/// Installs a complete context for the current worker thread and restores the
/// prior context when the returned guard is dropped.
pub fn scoped_context(task_id: Option<String>, platform: Option<PlatformKind>) -> LogContextGuard {
    let context = LogContext { task_id, platform };
    let previous = CURRENT_CONTEXT.with(|current| current.replace(context));
    LogContextGuard {
        previous: Some(previous),
        _thread_bound: PhantomData,
    }
}

pub fn current_context() -> LogContext {
    CURRENT_CONTEXT.with(|current| current.borrow().clone())
}

pub fn info(msg: impl ToString) -> Result<()> {
    write_log("INFO", &msg.to_string())
}

pub fn warning(msg: impl ToString) -> Result<()> {
    write_log("WARN", &msg.to_string())
}

pub fn error(msg: impl ToString) -> Result<()> {
    write_log("ERROR", &msg.to_string())
}

fn write_log(level: &str, msg: &str) -> Result<()> {
    let _io = LOG_IO_LOCK
        .lock()
        .map_err(|e| anyhow!("获取日志文件锁失败: {}", e))?;
    let path = path()?;
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let context = current_context();
    let task_prefix = context
        .task_id
        .as_ref()
        .map(|task_id| format!("[task={task_id}] "))
        .unwrap_or_default();
    let platform_prefix = context
        .platform
        .map(|platform| format!("[{}] ", platform_log_label(platform)))
        .unwrap_or_default();
    let line = format!(
        "[{}] [{}] {}{}{}\n",
        timestamp,
        level,
        task_prefix,
        platform_prefix,
        redact(msg)
    );
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

fn platform_log_label(platform: PlatformKind) -> &'static str {
    match platform {
        PlatformKind::Boss => "BOSS",
        PlatformKind::Liepin => "猎聘",
    }
}

pub fn read_tail_lines(n: usize) -> Result<String> {
    let _io = LOG_IO_LOCK
        .lock()
        .map_err(|e| anyhow!("获取日志文件锁失败: {}", e))?;
    let path = path()?;
    if !path.exists() {
        return Ok(String::new());
    }
    let content = std::fs::read_to_string(&path)?;
    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(n);
    Ok(all_lines[start..].join("\n"))
}

pub fn redact(message: &str) -> String {
    let mut safe = message.to_string();
    let credential_patterns = [
        r#"(?i)(authorization\s*[:=]\s*)([^\r\n,;}]+)"#,
        r#"(?i)((?:api[_-]?key|token|cookie)\s*[:=]\s*)([^\s,;}]+)"#,
    ];
    for pattern in credential_patterns {
        if let Ok(regex) = regex::Regex::new(pattern) {
            safe = regex.replace_all(&safe, "$1[REDACTED]").into_owned();
        }
    }
    if let Ok(regex) = regex::Regex::new(
        r#"(?i)("(?:api[_-]?key|authorization|token|cookie|resume_content|chat_body|prompt)"\s*:\s*")[^"]*(")"#,
    ) {
        safe = regex.replace_all(&safe, "$1[REDACTED]$2").into_owned();
    }
    if let Ok(regex) = regex::Regex::new(
        r#"(?i)((?:resume_content|resume|chat_body|chat_messages|prompt)\s*[:=]\s*)(?:Some\("[^"]*"\)|"[^"]*"|[^,;}]+)"#,
    ) {
        safe = regex.replace_all(&safe, "$1[REDACTED]").into_owned();
    }
    safe
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpa::run_flow::PlatformKind;
    use std::sync::{Arc, Barrier, Mutex as TestMutex};

    static TEST_LOG_LOCK: TestMutex<()> = TestMutex::new(());

    fn use_temp_log_file() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("rpa.log");
        *LOG_FILE_PATH.write().expect("lock log path") = Some(path);
        set_platform(Some(PlatformKind::Liepin)).expect("set platform");
        dir
    }

    #[test]
    fn prefixes_logs_with_current_platform_and_clears_context() {
        let _test_guard = TEST_LOG_LOCK.lock().expect("lock logger test");
        let _dir = use_temp_log_file();

        info("开始处理岗位").expect("write platform log");
        set_platform(None).expect("clear platform");
        info("无平台日志").expect("write unscoped log");

        let content = read_tail_lines(10).expect("read log");

        assert!(content.contains("[猎聘] 开始处理岗位"));
        assert!(content.contains("[INFO] 无平台日志"));
        assert!(!content.contains("[猎聘] 无平台日志"));
    }

    #[test]
    fn redacts_credentials_and_sensitive_bodies() {
        let input = r#"Authorization: Bearer standard-secret api_key=sk-live cookie=session123 {"resume_content":"my resume","prompt":"private prompt"}"#;
        let safe = redact(input);
        assert!(!safe.contains("Bearer-secret"));
        assert!(!safe.contains("sk-live"));
        assert!(!safe.contains("session123"));
        assert!(!safe.contains("my resume"));
        assert!(!safe.contains("private prompt"));
        assert!(!safe.contains("standard-secret"));
    }

    #[test]
    fn concurrent_thread_contexts_do_not_cross_contaminate_logs() {
        let _test_guard = TEST_LOG_LOCK.lock().expect("lock logger test");
        let _dir = use_temp_log_file();
        set_platform(None).expect("clear test thread platform");

        let start = Arc::new(Barrier::new(2));
        let boss_start = Arc::clone(&start);
        let boss = std::thread::spawn(move || {
            let _context = scoped_context(Some("boss-1".into()), Some(PlatformKind::Boss));
            boss_start.wait();
            for index in 0..20 {
                info(format!("boss-message-{index}")).expect("write boss log");
                std::thread::yield_now();
            }
        });

        let liepin_start = Arc::clone(&start);
        let liepin = std::thread::spawn(move || {
            let _context = scoped_context(Some("liepin-2".into()), Some(PlatformKind::Liepin));
            liepin_start.wait();
            for index in 0..20 {
                info(format!("liepin-message-{index}")).expect("write liepin log");
                std::thread::yield_now();
            }
        });

        boss.join().expect("boss worker");
        liepin.join().expect("liepin worker");

        let content = read_tail_lines(100).expect("read concurrent log");
        for line in content.lines() {
            if line.contains("boss-message-") {
                assert!(line.contains("[task=boss-1] [BOSS]"), "{line}");
                assert!(!line.contains("猎聘"), "{line}");
            }
            if line.contains("liepin-message-") {
                assert!(line.contains("[task=liepin-2] [猎聘]"), "{line}");
                assert!(!line.contains("BOSS"), "{line}");
            }
        }
    }

    #[test]
    fn scoped_context_restores_previous_thread_context() {
        let _test_guard = TEST_LOG_LOCK.lock().expect("lock logger test");
        set_platform(Some(PlatformKind::Boss)).expect("set outer platform");
        set_task_id(Some("outer".into())).expect("set outer task");

        {
            let _context = scoped_context(Some("inner".into()), Some(PlatformKind::Liepin));
            assert_eq!(
                current_context(),
                LogContext {
                    task_id: Some("inner".into()),
                    platform: Some(PlatformKind::Liepin),
                }
            );
        }

        assert_eq!(
            current_context(),
            LogContext {
                task_id: Some("outer".into()),
                platform: Some(PlatformKind::Boss),
            }
        );
        set_task_id(None).expect("clear outer task");
        set_platform(None).expect("clear outer platform");
    }
}
