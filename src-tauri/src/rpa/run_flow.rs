use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use super::{boss, liepin};
use crate::{config::AppRuntimeConfig, logger};

static JOB_TASK_RUNNING: AtomicBool = AtomicBool::new(false);
static JOB_TASK_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

// --- Types ---

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct JobTaskStatus {
    pub running: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnvCheckStep {
    Browser,
    PlatformLogin,
    Completed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnvCheckStatus {
    LoginRequired,
    Completed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct EnvCheckResult {
    pub platform: PlatformKind,
    pub current_step: EnvCheckStep,
    pub status: EnvCheckStatus,
    pub qr_code_base64: Option<String>,
    pub message: String,
}

impl EnvCheckResult {
    pub fn completed(platform: PlatformKind) -> Self {
        Self {
            platform,
            current_step: EnvCheckStep::Completed,
            status: EnvCheckStatus::Completed,
            qr_code_base64: None,
            message: "环境检查完成".to_string(),
        }
    }

    pub fn login_required(platform: PlatformKind, qr_code_base64: String, message: String) -> Self {
        Self {
            platform,
            current_step: EnvCheckStep::PlatformLogin,
            status: EnvCheckStatus::LoginRequired,
            qr_code_base64: Some(qr_code_base64),
            message,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Boss,
    Liepin,
}

impl PlatformKind {
    pub fn login_message(self) -> &'static str {
        match self {
            PlatformKind::Boss => "请使用 BOSS 直聘 App 扫码登录",
            PlatformKind::Liepin => "请使用猎聘 App 扫码登录",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlowMode {
    JobHunting,
    ReplyUnread,
    PeriodicJobHunting,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessLevel {
    Ready,
    Warning,
    Blocked,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ReadinessItem {
    pub key: String,
    pub label: String,
    pub level: ReadinessLevel,
    pub message: String,
    pub config_group: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ReadinessReport {
    pub ready: bool,
    pub platform: PlatformKind,
    pub mode: FlowMode,
    pub items: Vec<ReadinessItem>,
    pub summary: Vec<String>,
}

pub fn inspect_readiness(
    platform: PlatformKind,
    mode: FlowMode,
    config: &AppRuntimeConfig,
) -> ReadinessReport {
    let mut items = Vec::new();
    let browser = crate::browser::check_browser_env_status(&config.browser_config);
    items.push(ReadinessItem {
        key: "browser".to_string(),
        label: "浏览器环境".to_string(),
        level: if browser.browser_found && browser.user_data_dir_ok {
            ReadinessLevel::Ready
        } else {
            ReadinessLevel::Blocked
        },
        message: if browser.browser_found && browser.user_data_dir_ok {
            "浏览器与用户数据目录可用".to_string()
        } else {
            "请先配置可用的浏览器路径和用户数据目录".to_string()
        },
        config_group: Some("browser".to_string()),
    });

    let query = config
        .job_filter_config
        .query
        .as_deref()
        .unwrap_or_default()
        .trim();
    let needs_job_filter = !matches!(mode, FlowMode::ReplyUnread);
    items.push(ReadinessItem {
        key: "job_filter".to_string(),
        label: "岗位筛选".to_string(),
        level: if !needs_job_filter || !query.is_empty() {
            ReadinessLevel::Ready
        } else {
            ReadinessLevel::Blocked
        },
        message: if !needs_job_filter {
            "回复未读模式无需岗位搜索条件".to_string()
        } else if query.is_empty() {
            "请填写岗位关键词".to_string()
        } else {
            format!("岗位关键词：{query}")
        },
        config_group: Some("job".to_string()),
    });

    let resume_ready = config
        .resume_config
        .resume_content
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    items.push(ReadinessItem {
        key: "resume".to_string(),
        label: "简历".to_string(),
        level: if resume_ready {
            ReadinessLevel::Ready
        } else {
            ReadinessLevel::Warning
        },
        message: if resume_ready {
            "已配置简历内容".to_string()
        } else {
            "未配置简历，AI 上下文和投递内容可能不完整".to_string()
        },
        config_group: Some("resume".to_string()),
    });

    let greet_ready = config
        .greet_config
        .default_template
        .iter()
        .any(|resource| !resource.content.trim().is_empty())
        || config
            .greet_config
            .reply_prompt
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
    items.push(ReadinessItem {
        key: "greet".to_string(),
        label: "打招呼话术".to_string(),
        level: if !needs_job_filter || greet_ready {
            ReadinessLevel::Ready
        } else {
            ReadinessLevel::Blocked
        },
        message: if !needs_job_filter {
            "回复未读模式不使用打招呼话术".to_string()
        } else if greet_ready {
            "打招呼资源已配置".to_string()
        } else {
            "请至少配置一条有效打招呼资源或提示词".to_string()
        },
        config_group: Some("greet".to_string()),
    });

    let reply_ready = !config.replay_config.enable_auto_replay
        || config.replay_config.enable_llm
        || config.replay_config.templates.iter().any(|template| {
            template
                .content
                .iter()
                .any(|resource| !resource.content.trim().is_empty())
        });
    items.push(ReadinessItem {
        key: "reply".to_string(),
        label: "自动回复".to_string(),
        level: if reply_ready {
            ReadinessLevel::Ready
        } else {
            ReadinessLevel::Blocked
        },
        message: if !config.replay_config.enable_auto_replay {
            "自动回复未启用".to_string()
        } else if reply_ready {
            "自动回复资源已配置".to_string()
        } else {
            "自动回复已启用，但没有可用回复资源".to_string()
        },
        config_group: Some("reply".to_string()),
    });

    let llm_needed = config.replay_config.enable_llm
        || config
            .greet_config
            .default_template
            .iter()
            .any(|resource| matches!(resource.resource_type, crate::config::ReplayResourceType::LLM));
    items.push(ReadinessItem {
        key: "llm".to_string(),
        label: "大模型".to_string(),
        level: if !llm_needed || config.llm_config.is_some() {
            ReadinessLevel::Ready
        } else {
            ReadinessLevel::Blocked
        },
        message: if !llm_needed {
            "当前模式不依赖大模型".to_string()
        } else if config.llm_config.is_some() {
            "大模型服务已配置".to_string()
        } else {
            "话术使用了大模型，请先配置模型服务".to_string()
        },
        config_group: Some("llm".to_string()),
    });

    let ready = !items
        .iter()
        .any(|item| item.level == ReadinessLevel::Blocked);
    let summary = vec![
        format!("平台：{}", match platform { PlatformKind::Boss => "BOSS 直聘", PlatformKind::Liepin => "猎聘" }),
        format!("模式：{}", match mode { FlowMode::JobHunting => "单轮自动求职", FlowMode::ReplyUnread => "回复未读", FlowMode::PeriodicJobHunting => "周期投递" }),
        if query.is_empty() { "岗位关键词：未设置".to_string() } else { format!("岗位关键词：{query}") },
    ];

    ReadinessReport {
        ready,
        platform,
        mode,
        items,
        summary,
    }
}

// --- Job Task State ---

pub struct JobTaskRunningGuard;

impl Drop for JobTaskRunningGuard {
    fn drop(&mut self) {
        JOB_TASK_STOP_REQUESTED.store(false, Ordering::SeqCst);
        JOB_TASK_RUNNING.store(false, Ordering::SeqCst);
    }
}

pub fn get_job_task_status() -> JobTaskStatus {
    JobTaskStatus {
        running: JOB_TASK_RUNNING.load(Ordering::SeqCst),
    }
}

pub fn stop_job_task() -> Result<(), String> {
    if !JOB_TASK_RUNNING.load(Ordering::SeqCst) {
        return Err("求职任务未在运行".to_string());
    }

    JOB_TASK_STOP_REQUESTED.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn is_job_task_stop_requested() -> bool {
    JOB_TASK_STOP_REQUESTED.load(Ordering::SeqCst)
}

pub fn try_start_job_task() -> Result<JobTaskRunningGuard, String> {
    let Ok(_) = JOB_TASK_RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
    else {
        return Err("求职任务正在运行".to_string());
    };
    JOB_TASK_STOP_REQUESTED.store(false, Ordering::SeqCst);
    Ok(JobTaskRunningGuard)
}

// --- Check Env ---

pub async fn check_env(platform: PlatformKind) -> Result<EnvCheckResult, anyhow::Error> {
    let result = match platform {
        PlatformKind::Boss => boss::handler::login_check().await?,
        PlatformKind::Liepin => liepin::handler::login_check().await?,
    };
    let success = result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if success {
        Ok(EnvCheckResult::completed(platform))
    } else {
        let qr_base64 = match platform {
            PlatformKind::Boss => boss::handler::login().await?,
            PlatformKind::Liepin => liepin::handler::login().await?,
        };
        Ok(EnvCheckResult::login_required(
            platform,
            qr_base64,
            platform.login_message().to_string(),
        ))
    }
}

// --- RPA Flow ---

pub async fn execute_rpa_flow(
    platform: PlatformKind,
    mode: FlowMode,
    interval_minutes: Option<u64>,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    match mode {
        FlowMode::JobHunting => execute_job_hunting(platform, config).await,
        FlowMode::ReplyUnread => {
            execute_reply_unread(platform, config).await?;
            Ok(())
        }
        FlowMode::PeriodicJobHunting => {
            let interval = resolve_periodic_interval_minutes(interval_minutes)
                .map_err(|e| anyhow::anyhow!(e))?;
            periodic_position_say_hello(platform, config, interval).await
        }
    }
}

pub async fn execute_boss_flow(
    mode: FlowMode,
    interval_minutes: Option<u64>,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    execute_rpa_flow(PlatformKind::Boss, mode, interval_minutes, config).await
}

async fn execute_job_hunting(
    platform: PlatformKind,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    match platform {
        PlatformKind::Boss => boss::handler::position_say_hello(config).await,
        PlatformKind::Liepin => liepin::handler::position_say_hello(config).await,
    }
}

async fn execute_reply_unread(
    platform: PlatformKind,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    match platform {
        PlatformKind::Boss => {
            boss::handler::reply_unread(config).await?;
            Ok(())
        }
        PlatformKind::Liepin => {
            liepin::handler::reply_unread(config).await?;
            Ok(())
        }
    }
}

fn resolve_periodic_interval_minutes(interval_minutes: Option<u64>) -> Result<u64, String> {
    match interval_minutes {
        Some(value) if value > 0 => Ok(value),
        Some(_) => Err("周期性投递间隔必须大于 0 分钟".to_string()),
        None => Err("周期性投递缺少执行间隔".to_string()),
    }
}

async fn periodic_position_say_hello(
    platform: PlatformKind,
    config: &AppRuntimeConfig,
    interval_minutes: u64,
) -> Result<(), anyhow::Error> {
    loop {
        if is_job_task_stop_requested() {
            logger::info("周期性投递任务已结束")?;
            return Ok(());
        }

        logger::info("开始执行本轮周期性投递")?;
        execute_job_hunting(platform, config).await?;

        if is_job_task_stop_requested() {
            logger::info("周期性投递任务已结束")?;
            return Ok(());
        }

        logger::info(format!("本轮投递完成，等待{}分钟后继续", interval_minutes))?;
        wait_periodic_interval(interval_minutes).await?;
    }
}

async fn wait_periodic_interval(interval_minutes: u64) -> Result<(), anyhow::Error> {
    let mut remaining_seconds = interval_minutes.saturating_mul(60);

    while remaining_seconds > 0 {
        if is_job_task_stop_requested() {
            logger::info("周期性投递任务已结束")?;
            return Ok(());
        }

        let sleep_seconds = remaining_seconds.min(1);
        tokio::time::sleep(Duration::from_secs(sleep_seconds)).await;
        remaining_seconds -= sleep_seconds;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_mode_deserializes_periodic_job_hunting() {
        let mode: FlowMode = serde_json::from_str("\"periodic_job_hunting\"").unwrap();

        assert_eq!(mode, FlowMode::PeriodicJobHunting);
    }

    #[test]
    fn platform_kind_deserializes_supported_platforms() {
        let boss: PlatformKind = serde_json::from_str("\"boss\"").unwrap();
        let liepin: PlatformKind = serde_json::from_str("\"liepin\"").unwrap();

        assert_eq!(boss, PlatformKind::Boss);
        assert_eq!(liepin, PlatformKind::Liepin);
    }

    #[test]
    fn resolve_periodic_interval_minutes_requires_positive_minutes() {
        assert_eq!(resolve_periodic_interval_minutes(Some(10)).unwrap(), 10);
        assert!(resolve_periodic_interval_minutes(Some(0)).is_err());
        assert!(resolve_periodic_interval_minutes(None).is_err());
    }

    #[test]
    fn env_check_completed_result_has_completed_step() {
        let result = EnvCheckResult::completed(PlatformKind::Boss);

        assert_eq!(result.platform, PlatformKind::Boss);
        assert_eq!(result.current_step, EnvCheckStep::Completed);
        assert_eq!(result.status, EnvCheckStatus::Completed);
        assert_eq!(result.qr_code_base64, None);
        assert_eq!(result.message, "环境检查完成");
    }

    #[test]
    fn env_check_login_required_result_uses_platform_login_step() {
        let result = EnvCheckResult::login_required(
            PlatformKind::Liepin,
            "abc123".to_string(),
            "请使用猎聘 App 扫码登录".to_string(),
        );

        assert_eq!(result.current_step, EnvCheckStep::PlatformLogin);
        assert_eq!(result.status, EnvCheckStatus::LoginRequired);
        assert_eq!(result.qr_code_base64, Some("abc123".to_string()));
        assert_eq!(result.message, "请使用猎聘 App 扫码登录");
        assert_eq!(result.platform, PlatformKind::Liepin);
    }

    #[test]
    fn stop_job_task_requests_running_task_to_stop() {
        JOB_TASK_RUNNING.store(true, Ordering::SeqCst);
        JOB_TASK_STOP_REQUESTED.store(false, Ordering::SeqCst);

        let result = stop_job_task();

        assert!(result.is_ok());
        assert!(is_job_task_stop_requested());
        assert!(JOB_TASK_RUNNING.load(Ordering::SeqCst));

        JOB_TASK_RUNNING.store(false, Ordering::SeqCst);
        JOB_TASK_STOP_REQUESTED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn job_task_status_reflects_running_flag() {
        JOB_TASK_RUNNING.store(false, Ordering::SeqCst);
        assert!(!get_job_task_status().running);

        JOB_TASK_RUNNING.store(true, Ordering::SeqCst);
        assert!(get_job_task_status().running);

        JOB_TASK_RUNNING.store(false, Ordering::SeqCst);
    }
}
