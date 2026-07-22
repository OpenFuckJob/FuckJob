use std::{
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    sync::{Mutex, RwLock},
};

use anyhow::{anyhow, Result};
use chrono::Local;
use once_cell::sync::Lazy;
use tauri::Manager;

use crate::rpa::run_flow::PlatformKind;

static LOG_FILE_PATH: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));
static CURRENT_PLATFORM: Lazy<RwLock<Option<PlatformKind>>> = Lazy::new(|| RwLock::new(None));
static LOG_IO_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

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
    let mut current = CURRENT_PLATFORM
        .write()
        .map_err(|e| anyhow!("获取日志平台写锁失败: {}", e))?;
    *current = platform;
    Ok(())
}

pub fn info(msg: impl ToString) -> Result<()> {
    write_log("INFO", &msg.to_string())
}

pub fn warning(msg: impl ToString) -> Result<()> {
    write_log("WARN", &msg.to_string())
}

fn write_log(level: &str, msg: &str) -> Result<()> {
    let _io = LOG_IO_LOCK
        .lock()
        .map_err(|e| anyhow!("获取日志文件锁失败: {}", e))?;
    let path = path()?;
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let platform = CURRENT_PLATFORM
        .read()
        .map_err(|e| anyhow!("获取日志平台读锁失败: {}", e))?;
    let platform_prefix = platform
        .as_ref()
        .map(|platform| format!("[{}] ", platform_log_label(*platform)))
        .unwrap_or_default();
    let line = format!(
        "[{}] [{}] {}{}\n",
        timestamp,
        level,
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

    fn use_temp_log_file() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("rpa.log");
        *LOG_FILE_PATH.write().expect("lock log path") = Some(path);
        set_platform(Some(PlatformKind::Liepin)).expect("set platform");
        dir
    }

    #[test]
    fn prefixes_logs_with_current_platform_and_clears_context() {
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
}
