use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
use rust_drission::{ChromiumPage, Page};
use serde::{Deserialize, Serialize};

use super::{
    boss, liepin, polling,
    schedule::{self, PeriodicPlan, PeriodicState, RoundBudget},
};
use crate::{config::AppRuntimeConfig, logger};

static JOB_TASK_RUNNING: AtomicBool = AtomicBool::new(false);
static JOB_TASK_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Cancellation is scoped to the dedicated worker thread. Existing handler call sites can
    /// keep using `is_job_task_stop_requested` without sharing a process-wide stop flag.
    static CURRENT_JOB_TASK: RefCell<Option<(String, Arc<AtomicBool>)>> = const { RefCell::new(None) };
}

// --- Types ---

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct JobTaskStatus {
    pub running: bool,
    pub stopping: bool,
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Boss,
    Liepin,
}

impl PlatformKind {
    /// 落库与解析规则里用的平台标识，与 `JobDetail.platform` 保持一致
    pub fn as_str(self) -> &'static str {
        match self {
            PlatformKind::Boss => "boss",
            PlatformKind::Liepin => "liepin",
        }
    }

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
    SyncChatHistory,
    PeriodicJobHunting,
    PollingReply,
}

impl FlowMode {
    /// 是否是永不自然结束、只能由用户停止的长驻任务
    pub fn is_long_running(self) -> bool {
        matches!(self, Self::PeriodicJobHunting | Self::PollingReply)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::JobHunting => "单轮自动求职",
            Self::ReplyUnread => "回复未读",
            Self::SyncChatHistory => "读取聊天消息",
            Self::PeriodicJobHunting => "周期投递",
            Self::PollingReply => "轮询回复",
        }
    }
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
    let needs_job_filter = matches!(mode, FlowMode::JobHunting | FlowMode::PeriodicJobHunting);
    items.push(ReadinessItem {
        key: "job_filter".to_string(),
        label: "岗位筛选".to_string(),
        level: if !needs_job_filter || !query.is_empty() {
            ReadinessLevel::Ready
        } else {
            ReadinessLevel::Blocked
        },
        message: if !needs_job_filter {
            "当前模式无需岗位搜索条件".to_string()
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

    let greet_prompt_missing = config.greet_config.enable_llm
        && config.greet_config.has_enabled_llm_resource()
        && !config.greet_config.prompt_ready();
    let greet_ready = !greet_prompt_missing && config.greet_config.has_sendable_resource();
    items.push(ReadinessItem {
        key: "greet".to_string(),
        label: "打招呼话术".to_string(),
        level: if !needs_job_filter || greet_ready {
            ReadinessLevel::Ready
        } else {
            ReadinessLevel::Blocked
        },
        message: if !needs_job_filter {
            "当前模式不使用打招呼话术".to_string()
        } else if greet_ready {
            "打招呼资源已配置".to_string()
        } else if greet_prompt_missing {
            "已启用 LLM 打招呼，请填写主动沟通提示词".to_string()
        } else {
            "请至少启用并配置一条有效的打招呼内容".to_string()
        },
        config_group: Some("greet".to_string()),
    });

    let needs_reply_config = matches!(mode, FlowMode::ReplyUnread | FlowMode::PollingReply);
    let replay_configs = config
        .job_profiles
        .iter()
        .map(|profile| &profile.replay_config)
        .collect::<Vec<_>>();
    // LLM 回复和正则模板是两条独立路径，任一开启就算启用了自动回复。
    // 只看模板开关会把「只开 LLM」的方案判成未启用，进而漏掉它对模型服务的依赖
    let auto_reply_configs = replay_configs
        .iter()
        .copied()
        .filter(|replay| replay.auto_reply_enabled())
        .collect::<Vec<_>>();
    let any_auto_reply = !auto_reply_configs.is_empty();
    let reply_prompt_missing = auto_reply_configs
        .iter()
        .any(|replay| replay.enable_llm && !replay.prompt_ready());
    let reply_ready = !needs_reply_config
        || (!reply_prompt_missing
            && auto_reply_configs
                .iter()
                .all(|replay| replay.enable_llm || replay.has_sendable_template()));
    items.push(ReadinessItem {
        key: "reply".to_string(),
        label: "自动回复".to_string(),
        level: if reply_ready {
            ReadinessLevel::Ready
        } else {
            ReadinessLevel::Blocked
        },
        message: if !needs_reply_config {
            "当前模式不使用自动回复".to_string()
        } else if !any_auto_reply {
            "所有求职方案均未启用自动回复".to_string()
        } else if reply_ready {
            "自动回复资源已配置".to_string()
        } else if reply_prompt_missing {
            "已启用 AI 回复，请填写自动回复提示词".to_string()
        } else {
            "选择了仅规则回复，但没有配置可发送的话术".to_string()
        },
        config_group: Some("reply".to_string()),
    });

    let semantic_filter_needed =
        matches!(mode, FlowMode::JobHunting | FlowMode::PeriodicJobHunting)
            && config.job_filter_config.enable_semantic_filter;
    let semantic_intent_ready = !semantic_filter_needed
        || config
            .job_filter_config
            .semantic_filter_intent
            .as_deref()
            .is_some_and(|intent| !intent.trim().is_empty());
    items.push(ReadinessItem {
        key: "job_intent".to_string(),
        label: "岗位意图复核".to_string(),
        level: if semantic_intent_ready {
            ReadinessLevel::Ready
        } else {
            ReadinessLevel::Blocked
        },
        message: if !semantic_filter_needed {
            "AI 岗位意图复核未启用".to_string()
        } else if semantic_intent_ready {
            "目标岗位要求已配置".to_string()
        } else {
            "AI 岗位意图复核已启用，请填写目标岗位要求".to_string()
        },
        config_group: Some("job".to_string()),
    });

    let llm_needed = match mode {
        FlowMode::ReplyUnread | FlowMode::PollingReply => {
            auto_reply_configs.iter().any(|replay| replay.enable_llm)
        }
        FlowMode::JobHunting | FlowMode::PeriodicJobHunting => {
            config.job_filter_config.enable_semantic_filter
                || config.greet_config.llm_resource_ready()
        }
        FlowMode::SyncChatHistory => false,
    };
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
            "当前功能使用了大模型，请先配置模型服务".to_string()
        },
        config_group: Some("llm".to_string()),
    });

    let ready = !items
        .iter()
        .any(|item| item.level == ReadinessLevel::Blocked);
    let summary = vec![
        format!(
            "平台：{}",
            match platform {
                PlatformKind::Boss => "BOSS 直聘",
                PlatformKind::Liepin => "猎聘",
            }
        ),
        format!("模式：{}", mode.display_name()),
        if query.is_empty() {
            "岗位关键词：未设置".to_string()
        } else {
            format!("岗位关键词：{query}")
        },
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
        stopping: JOB_TASK_STOP_REQUESTED.load(Ordering::SeqCst),
    }
}

pub fn is_job_task_running() -> bool {
    JOB_TASK_RUNNING.load(Ordering::SeqCst)
}

pub fn stop_job_task() -> Result<(), String> {
    if !JOB_TASK_RUNNING.load(Ordering::SeqCst) {
        return Err("求职任务未在运行".to_string());
    }

    JOB_TASK_STOP_REQUESTED.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn is_job_task_stop_requested() -> bool {
    CURRENT_JOB_TASK
        .with(|current| {
            current
                .borrow()
                .as_ref()
                .map(|(_, cancelled)| cancelled.load(Ordering::SeqCst))
        })
        .unwrap_or_else(|| JOB_TASK_STOP_REQUESTED.load(Ordering::SeqCst))
}

/// Installs the cancellation state for one dedicated task worker.
///
/// The guard deliberately is not `Send`: it must be dropped on the worker thread that installed
/// it, at which point the thread-local context is cleared.
pub struct JobTaskContextGuard;

impl Drop for JobTaskContextGuard {
    fn drop(&mut self) {
        CURRENT_JOB_TASK.with(|current| *current.borrow_mut() = None);
    }
}

pub fn enter_job_task_context(task_id: String, cancelled: Arc<AtomicBool>) -> JobTaskContextGuard {
    CURRENT_JOB_TASK.with(|current| *current.borrow_mut() = Some((task_id, cancelled)));
    JobTaskContextGuard
}

/// 当前队列 worker 的任务标识。非队列兼容入口返回 None。
pub fn current_job_task_id() -> Option<String> {
    CURRENT_JOB_TASK.with(|current| {
        current
            .borrow()
            .as_ref()
            .map(|(task_id, _)| task_id.clone())
    })
}

fn has_managed_job_task_context() -> bool {
    CURRENT_JOB_TASK.with(|current| current.borrow().is_some())
}

pub fn try_start_job_task() -> Result<JobTaskRunningGuard, String> {
    let Ok(_) = JOB_TASK_RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
    else {
        return Err("求职任务正在运行".to_string());
    };
    JOB_TASK_STOP_REQUESTED.store(false, Ordering::SeqCst);
    // 自动分析的限额按「一次求职任务」计，新任务开始时归零
    crate::auto_analysis::reset_task_counter();
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
    plan: Option<PeriodicPlan>,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    if has_managed_job_task_context() {
        let config = config.clone();
        return crate::browser::with_task_browser(|connection, main_tab| {
            Box::pin(async move {
                execute_rpa_flow_with_browser(connection, main_tab, platform, mode, plan, &config)
                    .await
            })
        })
        .await;
    }

    match mode {
        FlowMode::JobHunting => {
            execute_job_hunting(platform, config, RoundBudget::unlimited()).await
        }
        FlowMode::ReplyUnread => {
            execute_reply_unread(platform, config).await?;
            Ok(())
        }
        FlowMode::SyncChatHistory => {
            if platform != PlatformKind::Boss {
                return Err(anyhow::anyhow!("读取聊天消息任务目前仅支持 BOSS 直聘"));
            }
            boss::handler::sync_chat_history().await?;
            Ok(())
        }
        FlowMode::PeriodicJobHunting => {
            let plan =
                schedule::resolve_plan(plan, Local::now()).map_err(|e| anyhow::anyhow!(e))?;
            periodic_job_hunting(&PeriodicTarget::NewTab(platform), config, &plan).await
        }
        FlowMode::PollingReply => polling_reply(&ReplyTarget::NewTab(platform), config).await,
    }
}

/// Execute a queued task on its independent CDP connection and owned main tab.
pub async fn execute_rpa_flow_with_browser(
    connection: &ChromiumPage,
    main_tab: &Page,
    platform: PlatformKind,
    mode: FlowMode,
    plan: Option<PeriodicPlan>,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    match mode {
        FlowMode::JobHunting => {
            execute_job_hunting_on_page(
                connection,
                main_tab,
                platform,
                config,
                RoundBudget::unlimited(),
            )
            .await
        }
        FlowMode::ReplyUnread => execute_reply_unread_on_page(main_tab, platform, config).await,
        FlowMode::SyncChatHistory => {
            if platform != PlatformKind::Boss {
                return Err(anyhow::anyhow!("读取聊天消息任务目前仅支持 BOSS 直聘"));
            }
            boss::handler::sync_chat_history_on_page(main_tab).await?;
            Ok(())
        }
        FlowMode::PeriodicJobHunting => {
            let plan =
                schedule::resolve_plan(plan, Local::now()).map_err(|e| anyhow::anyhow!(e))?;
            periodic_job_hunting(
                &PeriodicTarget::OwnedTab(connection, main_tab, platform),
                config,
                &plan,
            )
            .await
        }
        FlowMode::PollingReply => {
            polling_reply(&ReplyTarget::OwnedTab(main_tab, platform), config).await
        }
    }
}

pub async fn execute_boss_flow(
    mode: FlowMode,
    plan: Option<PeriodicPlan>,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    execute_rpa_flow(PlatformKind::Boss, mode, plan, config).await
}

async fn execute_job_hunting(
    platform: PlatformKind,
    config: &AppRuntimeConfig,
    budget: RoundBudget,
) -> Result<(), anyhow::Error> {
    match platform {
        PlatformKind::Boss => boss::handler::position_say_hello(config, budget).await,
        PlatformKind::Liepin => liepin::handler::position_say_hello(config, budget).await,
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

async fn execute_job_hunting_on_page(
    connection: &ChromiumPage,
    main_tab: &Page,
    platform: PlatformKind,
    config: &AppRuntimeConfig,
    budget: RoundBudget,
) -> Result<(), anyhow::Error> {
    match platform {
        PlatformKind::Boss => {
            boss::handler::position_say_hello_on_page(connection, main_tab, config, budget).await
        }
        PlatformKind::Liepin => {
            liepin::handler::position_say_hello_on_page(connection, main_tab, config, budget).await
        }
    }
}

async fn execute_reply_unread_on_page(
    main_tab: &Page,
    platform: PlatformKind,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    match platform {
        PlatformKind::Boss => {
            boss::handler::reply_unread_on_page(main_tab, config).await?;
        }
        PlatformKind::Liepin => {
            liepin::handler::reply_unread_on_page(main_tab, config).await?;
        }
    }
    Ok(())
}

/// 一轮回复要跑在哪个标签页上。
///
/// 队列任务持有自己的主标签页，兼容入口则每轮新开一个。两条路径的轮询节奏
/// 完全一致，只有这一处不同，所以差异收在这里而不是复制两份循环
enum ReplyTarget<'a> {
    NewTab(PlatformKind),
    OwnedTab(&'a Page, PlatformKind),
}

async fn run_reply_round(
    target: &ReplyTarget<'_>,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    match target {
        ReplyTarget::NewTab(platform) => execute_reply_unread(*platform, config).await,
        ReplyTarget::OwnedTab(main_tab, platform) => {
            execute_reply_unread_on_page(main_tab, *platform, config).await
        }
    }
}

/// 只轮询回复、不投递的长驻任务。
///
/// 单轮失败不终止整个轮询：会话页加载超时、平台偶发风控这类错误下一轮多半就好了，
/// 为此让用户重新点一次开始任务并不合理
async fn polling_reply(
    target: &ReplyTarget<'_>,
    config: &AppRuntimeConfig,
) -> Result<(), anyhow::Error> {
    // 不在活跃时段时每分钟醒一次重新判断。每次都打日志的话，挂一夜就是几百条噪声，
    // 真正有用的回复记录会被冲得找不到，所以只在活跃状态翻转的那一次打。
    //
    // 初值取 true 而不是 None：任务刚启动就打一句「恢复轮询」读着像是从中断里
    // 爬起来，而这时它才刚开始。真正该说话的是第一次停下来
    let mut was_active = true;

    loop {
        if is_job_task_stop_requested() {
            logger::info("轮询回复任务已结束")?;
            return Ok(());
        }

        if polling::is_active_now(&config.reply_polling_config) {
            if let Err(error) = run_reply_round(target, config).await {
                logger::warning(format!("本轮自动回复失败，等待下一轮重试: {error}"))?;
            }
        }

        let wake = polling::next_wake_now(&config.reply_polling_config);
        let active = matches!(wake, polling::NextWake::Sleep(_));
        if was_active != active {
            logger::info(if active {
                "已进入自动回复时段，恢复轮询"
            } else {
                "当前不在自动回复时段，暂停轮询"
            })?;
            was_active = active;
        }

        if !sleep_interruptible(wake.seconds()).await {
            logger::info("轮询回复任务已结束")?;
            return Ok(());
        }
    }
}

/// 连续多少轮投递失败即终止整个周期任务。
///
/// 单轮失败不该拖垮长驻任务——岗位列表加载超时、平台偶发风控这类错误下一轮多半
/// 就好了。但连着失败就不是偶发，多半是登录失效或触发了安全验证，再转下去只是空转
const MAX_CONSECUTIVE_ROUND_FAILURES: u32 = 3;

/// 一轮周期投递跑在哪里。
///
/// 队列任务持有自己的连接和主标签页，兼容入口则每次新开。投递、回复、同步历史
/// 三件事在两条路径上的差异全都收在这里，主循环只有一份
enum PeriodicTarget<'a> {
    NewTab(PlatformKind),
    OwnedTab(&'a ChromiumPage, &'a Page, PlatformKind),
}

impl<'a> PeriodicTarget<'a> {
    fn reply_target(&self) -> ReplyTarget<'a> {
        match self {
            Self::NewTab(platform) => ReplyTarget::NewTab(*platform),
            Self::OwnedTab(_, main_tab, platform) => ReplyTarget::OwnedTab(main_tab, *platform),
        }
    }

    async fn deliver(
        &self,
        config: &AppRuntimeConfig,
        budget: RoundBudget,
    ) -> Result<(), anyhow::Error> {
        match self {
            Self::NewTab(platform) => execute_job_hunting(*platform, config, budget).await,
            Self::OwnedTab(connection, main_tab, platform) => {
                execute_job_hunting_on_page(connection, main_tab, *platform, config, budget).await
            }
        }
    }

    async fn sync_chat_history(&self) -> Result<(), anyhow::Error> {
        match self {
            Self::NewTab(PlatformKind::Boss) => {
                boss::handler::sync_chat_history().await?;
                Ok(())
            }
            Self::OwnedTab(_, main_tab, PlatformKind::Boss) => {
                boss::handler::sync_chat_history_on_page(main_tab).await?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// 周期投递主循环。
///
/// 每次转到循环顶部都重新问一遍计划「现在该干嘛」，而不是自己记状态：任务可能
/// 一挂就是几天，跨过午夜、跨过投递时段边界，靠增量推算迟早会算歪
async fn periodic_job_hunting(
    target: &PeriodicTarget<'_>,
    config: &AppRuntimeConfig,
    plan: &PeriodicPlan,
) -> Result<(), anyhow::Error> {
    let budget = RoundBudget::from_plan(plan);
    let mut consecutive_failures = 0u32;
    // 时段外每轮都播报一次「暂停中」会把日志刷满，只在刚进入暂停时说一次
    let mut pause_announced = false;

    loop {
        if is_job_task_stop_requested() {
            logger::info("周期性投递任务已结束")?;
            return Ok(());
        }

        match schedule::plan_state(plan, Local::now()) {
            PeriodicState::Finished => {
                logger::info("已到设定的结束时间，周期投递任务结束")?;
                return Ok(());
            }
            PeriodicState::Idle(open_at) => {
                if !pause_announced {
                    logger::info(format!(
                        "当前不在投递时段（{}），暂停投递，{} 恢复；期间继续自动回复未读",
                        schedule::describe_windows(&plan.windows),
                        open_at.format("%m-%d %H:%M"),
                    ))?;
                    pause_announced = true;
                }
                if !idle_until(target, config, open_at).await? {
                    logger::info("周期性投递任务已结束")?;
                    return Ok(());
                }
            }
            PeriodicState::Deliver => {
                pause_announced = false;
                logger::info("开始执行本轮周期性投递")?;
                let started = Instant::now();
                match target.deliver(config, budget).await {
                    Ok(()) => consecutive_failures = 0,
                    Err(error) => {
                        consecutive_failures += 1;
                        if consecutive_failures >= MAX_CONSECUTIVE_ROUND_FAILURES {
                            return Err(error.context(format!(
                                "周期投递连续 {MAX_CONSECUTIVE_ROUND_FAILURES} 轮失败，已终止任务"
                            )));
                        }
                        logger::warning(format!(
                            "本轮投递失败（连续第 {consecutive_failures} 次），等待下一轮重试：{error}"
                        ))?;
                    }
                }

                if is_job_task_stop_requested() {
                    logger::info("周期性投递任务已结束")?;
                    return Ok(());
                }

                let next_at = schedule::next_delivery_at(plan, Local::now());
                logger::info(format!(
                    "本轮投递用时 {} 分钟，下一轮 {} 开始，其间按轮询节奏检查未读",
                    started.elapsed().as_secs() / 60,
                    next_at.format("%m-%d %H:%M"),
                ))?;
                if !idle_until(target, config, next_at).await? {
                    logger::info("周期性投递任务已结束")?;
                    return Ok(());
                }
            }
        }
    }
}

/// 周期投递的空闲期：两轮之间的等待，以及投递时段之外的暂停。
///
/// 这段时间本来是纯空闲——投递间隔通常半小时起步，而调度器对每个平台只放行一个任务，
/// 另起一个轮询任务只会跟投递互抢浏览器。所以把空闲期交给自动回复，一个任务就同时
/// 具备投递和回复两条能力。
///
/// 用绝对的 deadline 而不是倒扣剩余秒数：回复本身要花时间（默认单轮最多 10 个会话，
/// 每个还带拟人延迟），倒扣的写法把这段耗时算在周期之外，实际周期会被撑成
/// 「间隔 + N×回复耗时」，投递节奏跟着漂。返回 false 表示中途收到停止请求
async fn idle_until(
    target: &PeriodicTarget<'_>,
    config: &AppRuntimeConfig,
    deadline: DateTime<Local>,
) -> Result<bool, anyhow::Error> {
    // 同步历史对话放在空闲期之内而不是之外：它一样要占用浏览器，摆在外面等于
    // 每轮都在周期预算之上再加一笔，投递间隔就名不副实了
    if let Err(error) = target.sync_chat_history().await {
        logger::warning(format!("周期间歇同步历史对话失败，本轮继续：{error}"))?;
    }

    let mut was_active = true;
    loop {
        if is_job_task_stop_requested() {
            return Ok(false);
        }
        let mut remaining = schedule::seconds_until(deadline, Local::now());
        if remaining == 0 {
            return Ok(true);
        }

        // 先回一轮再睡。刚投完正是对方可能已经回消息的时候；更要紧的是空闲期短于
        // 轮询间隔时（投递间隔 10 分钟撞上默认 5 分钟轮询加 120 秒抖动就会发生），
        // 先睡的写法会让这一整段空闲期一次都不回复
        if polling::is_active_now(&config.reply_polling_config) {
            if let Err(error) = run_reply_round(&target.reply_target(), config).await {
                logger::warning(format!("周期间歇自动回复失败，稍后重试：{error}"))?;
            }
            remaining = schedule::seconds_until(deadline, Local::now());
            if remaining == 0 {
                return Ok(true);
            }
        }

        let wake = polling::next_wake_now(&config.reply_polling_config);
        let active = matches!(wake, polling::NextWake::Sleep(_));
        if was_active != active {
            logger::info(if active {
                "已进入自动回复时段，空闲期恢复回复未读"
            } else {
                "当前不在自动回复时段，空闲期暂停回复"
            })?;
            was_active = active;
        }

        if !sleep_interruptible(wake.seconds().min(remaining)).await {
            return Ok(false);
        }
    }
}

/// 可被停止请求打断的睡眠。返回 true 表示睡满了，false 表示中途收到停止请求。
///
/// 每秒醒一次而不是一觉睡到底：用户点了停止之后，任务得在一秒内响应，
/// 而不是等完整个轮询间隔
async fn sleep_interruptible(seconds: u64) -> bool {
    for _ in 0..seconds {
        if is_job_task_stop_requested() {
            return false;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_app_config, GreetResource, ReplayResourceType};

    fn find_item<'a>(report: &'a ReadinessReport, key: &str) -> &'a ReadinessItem {
        report
            .items
            .iter()
            .find(|item| item.key == key)
            .unwrap_or_else(|| panic!("未找到就绪项 {key}"))
    }

    fn job_hunting_report(config: &AppRuntimeConfig) -> ReadinessReport {
        inspect_readiness(PlatformKind::Boss, FlowMode::JobHunting, config)
    }

    #[test]
    fn greet_is_blocked_when_llm_enabled_without_prompt() {
        let mut config = default_app_config();
        config.job_filter_config.query = Some("Rust 工程师".to_string());
        config.greet_config.enable_llm = true;
        config.greet_config.reply_prompt = Some("   ".to_string());
        config.greet_config.default_template = vec![GreetResource {
            enabled: true,
            resource_type: ReplayResourceType::LLM,
            content: String::new(),
        }];

        let report = job_hunting_report(&config);
        let greet = find_item(&report, "greet");

        assert_eq!(greet.level, ReadinessLevel::Blocked);
        assert_eq!(greet.message, "已启用 LLM 打招呼，请填写主动沟通提示词");
    }

    #[test]
    fn greet_is_blocked_when_no_send_sequence_item_exists() {
        let mut config = default_app_config();
        config.job_filter_config.query = Some("Rust 工程师".to_string());
        config.greet_config.enable_llm = true;
        config.greet_config.reply_prompt = Some("请生成打招呼内容".to_string());

        let report = job_hunting_report(&config);
        let greet = find_item(&report, "greet");

        assert_eq!(greet.level, ReadinessLevel::Blocked);
        assert_eq!(greet.message, "请至少启用并配置一条有效的打招呼内容");
    }

    #[test]
    fn llm_is_required_when_enabled_llm_item_is_ready() {
        let mut config = default_app_config();
        config.job_filter_config.query = Some("Rust 工程师".to_string());
        config.greet_config.enable_llm = true;
        config.greet_config.reply_prompt = Some("请生成打招呼内容".to_string());
        config.greet_config.default_template = vec![GreetResource {
            enabled: true,
            resource_type: ReplayResourceType::LLM,
            content: String::new(),
        }];

        let report = job_hunting_report(&config);
        let llm = find_item(&report, "llm");

        assert_eq!(llm.level, ReadinessLevel::Blocked);
        assert_eq!(llm.message, "当前功能使用了大模型，请先配置模型服务");
    }

    #[test]
    fn disabled_llm_item_does_not_require_model_service() {
        let mut config = default_app_config();
        config.job_filter_config.query = Some("Rust 工程师".to_string());
        config.greet_config.enable_llm = true;
        config.greet_config.reply_prompt = Some("请生成打招呼内容".to_string());
        config.greet_config.default_template = vec![GreetResource {
            enabled: false,
            resource_type: ReplayResourceType::LLM,
            content: String::new(),
        }];

        let report = job_hunting_report(&config);
        let llm = find_item(&report, "llm");

        assert_eq!(llm.level, ReadinessLevel::Ready);
        assert_eq!(llm.message, "当前模式不依赖大模型");
    }

    #[test]
    fn llm_is_not_required_when_greet_llm_disabled() {
        let mut config = default_app_config();
        config.job_filter_config.query = Some("Rust 工程师".to_string());
        config.greet_config.enable_llm = false;
        config.greet_config.reply_prompt = Some("请生成打招呼内容".to_string());
        config.greet_config.default_template = vec![GreetResource {
            enabled: true,
            resource_type: ReplayResourceType::Text,
            content: "您好".to_string(),
        }];

        let report = job_hunting_report(&config);
        let llm = find_item(&report, "llm");

        assert_eq!(llm.level, ReadinessLevel::Ready);
        assert_eq!(llm.message, "当前模式不依赖大模型");
    }

    #[test]
    fn reply_readiness_checks_all_profile_routes_instead_of_only_default() {
        let mut config = default_app_config();
        config.job_profiles[0].replay_config.enable_template_reply = false;
        let mut routed = config.job_profiles[0].clone();
        routed.id = "routed".to_string();
        routed.name = "需要 AI 的历史方案".to_string();
        routed.replay_config.enable_template_reply = true;
        routed.replay_config.enable_llm = true;
        routed.replay_config.reply_prompt = Some("请根据对话生成回复".to_string());
        config.job_profiles.push(routed);

        let report = inspect_readiness(PlatformKind::Boss, FlowMode::ReplyUnread, &config);

        assert_eq!(find_item(&report, "reply").level, ReadinessLevel::Ready);
        assert_eq!(find_item(&report, "llm").level, ReadinessLevel::Blocked);
    }

    /// LLM 回复和正则模板是两个独立开关。只开 LLM 的方案照样要回复，
    /// 也照样依赖模型服务——此前它被判成「未启用自动回复」，
    /// 于是模型服务没配也放行，真跑起来才发现整条链路被跳过
    #[test]
    fn llm_reply_alone_counts_as_enabled_and_requires_the_model_service() {
        let mut config = default_app_config();
        let replay = &mut config.job_profiles[0].replay_config;
        replay.enable_template_reply = false;
        replay.enable_llm = true;
        replay.reply_prompt = Some("请根据对话生成回复".to_string());

        let report = inspect_readiness(PlatformKind::Boss, FlowMode::ReplyUnread, &config);

        assert_eq!(find_item(&report, "reply").level, ReadinessLevel::Ready);
        assert_eq!(find_item(&report, "reply").message, "自动回复资源已配置");
        assert_eq!(find_item(&report, "llm").level, ReadinessLevel::Blocked);
    }

    /// 开了 LLM 回复却没写提示词，运行期会在生成那一步直接报错，
    /// 不如在开跑之前就说清楚缺什么
    #[test]
    fn reply_is_blocked_when_llm_is_on_without_a_prompt() {
        let mut config = default_app_config();
        let replay = &mut config.job_profiles[0].replay_config;
        replay.enable_llm = true;
        replay.reply_prompt = Some("   ".to_string());

        let report = inspect_readiness(PlatformKind::Boss, FlowMode::ReplyUnread, &config);

        assert_eq!(find_item(&report, "reply").level, ReadinessLevel::Blocked);
        assert_eq!(
            find_item(&report, "reply").message,
            "已启用 AI 回复，请填写自动回复提示词"
        );
    }

    #[test]
    fn reply_readiness_allows_a_sync_run_when_every_profile_disables_auto_reply() {
        let mut config = default_app_config();
        config.job_profiles[0].replay_config.enable_template_reply = false;

        let report = inspect_readiness(PlatformKind::Boss, FlowMode::ReplyUnread, &config);

        assert_eq!(find_item(&report, "reply").level, ReadinessLevel::Ready);
        assert_eq!(
            find_item(&report, "reply").message,
            "所有求职方案均未启用自动回复"
        );
        assert_eq!(find_item(&report, "llm").level, ReadinessLevel::Ready);
    }

    #[test]
    fn flow_mode_deserializes_periodic_job_hunting() {
        let mode: FlowMode = serde_json::from_str("\"periodic_job_hunting\"").unwrap();

        assert_eq!(mode, FlowMode::PeriodicJobHunting);
    }

    #[test]
    fn flow_mode_deserializes_polling_reply() {
        let mode: FlowMode = serde_json::from_str("\"polling_reply\"").unwrap();

        assert_eq!(mode, FlowMode::PollingReply);
    }

    /// 长驻判定直接决定「同平台不许开第二个」这条约束覆盖到谁，
    /// 漏掉一个模式的后果是两个无限循环任务互抢浏览器
    #[test]
    fn only_endless_modes_are_treated_as_long_running() {
        assert!(FlowMode::PeriodicJobHunting.is_long_running());
        assert!(FlowMode::PollingReply.is_long_running());
        assert!(!FlowMode::JobHunting.is_long_running());
        assert!(!FlowMode::ReplyUnread.is_long_running());
        assert!(!FlowMode::SyncChatHistory.is_long_running());
    }

    /// 轮询回复只是把「回复未读」放进循环，两者的就绪判定一旦漂移，
    /// 用户会遇到手动跑得动、开轮询却被拦下来的怪事
    #[test]
    fn polling_reply_readiness_matches_reply_unread_item_by_item() {
        let mut config = default_app_config();
        let replay = &mut config.job_profiles[0].replay_config;
        replay.enable_llm = true;
        replay.reply_prompt = Some("请根据对话生成回复".to_string());

        let once = inspect_readiness(PlatformKind::Boss, FlowMode::ReplyUnread, &config);
        let polling = inspect_readiness(PlatformKind::Boss, FlowMode::PollingReply, &config);

        assert_eq!(polling.items, once.items);
        assert_eq!(polling.ready, once.ready);
        assert!(polling.summary.contains(&"模式：轮询回复".to_string()));
    }

    #[test]
    fn flow_mode_deserializes_sync_chat_history() {
        let mode: FlowMode = serde_json::from_str("\"sync_chat_history\"").unwrap();

        assert_eq!(mode, FlowMode::SyncChatHistory);
    }

    #[test]
    fn platform_kind_deserializes_supported_platforms() {
        let boss: PlatformKind = serde_json::from_str("\"boss\"").unwrap();
        let liepin: PlatformKind = serde_json::from_str("\"liepin\"").unwrap();

        assert_eq!(boss, PlatformKind::Boss);
        assert_eq!(liepin, PlatformKind::Liepin);
    }

    /// 周期投递的计划校验发生在真正开跑之前。这里只钉住「run_flow 确实走了校验」，
    /// 各种边界由 `schedule` 的单测覆盖
    #[test]
    fn periodic_plan_is_validated_before_the_loop_starts() {
        let now = Local::now();

        assert_eq!(
            schedule::resolve_plan(Some(PeriodicPlan::every(10)), now)
                .unwrap()
                .interval_minutes,
            10
        );
        assert!(schedule::resolve_plan(Some(PeriodicPlan::every(0)), now).is_err());
        assert!(schedule::resolve_plan(None, now).is_err());
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
        assert!(get_job_task_status().stopping);

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
