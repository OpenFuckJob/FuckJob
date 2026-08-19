use crate::command::base::CommandResult;
use crate::config;
use crate::dao::model::{ChatMessageRecord, InterviewJobAnalysis, JobDetail};
use crate::dao::{analysis_dao, chat_message_dao, job_detail_dao};
use crate::job_description::ParsedJobDescription;
use chrono::{Duration, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[tauri::command]
pub fn job_list() -> CommandResult<Vec<JobDetail>> {
    match job_detail_dao::list() {
        Ok(list) => CommandResult::ok(list),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommunicationStatus {
    Rejected,
    Replied,
    NoReply,
}

#[derive(Debug, Serialize)]
pub struct JobListItem {
    #[serde(flatten)]
    pub job: JobDetail,
    pub communication_status: CommunicationStatus,
}

fn is_explicit_rejection(text: &str) -> bool {
    let normalized = text
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    const REJECTION_PHRASES: &[&str] = &[
        "不太合适",
        "不合适",
        "暂不考虑",
        "暂时不考虑",
        "不匹配",
        "不符合",
        "岗位已招满",
        "已经招满",
        "职位已招满",
        "岗位关闭",
        "职位关闭",
        "停止招聘",
        "没有hc",
        "无hc",
        "不通过",
        "暂时没有合适",
        "目前没有合适",
    ];

    REJECTION_PHRASES
        .iter()
        .any(|phrase| normalized.contains(phrase))
}

#[tauri::command]
pub fn job_list_with_status() -> CommandResult<Vec<JobListItem>> {
    let result = (|| -> anyhow::Result<Vec<JobListItem>> {
        let jobs = job_detail_dao::list()?;
        let messages = chat_message_dao::list()?;
        let mut replied_job_ids = HashSet::new();
        let mut rejected_job_ids = HashSet::new();

        for message in messages
            .into_iter()
            .filter(|message| message.received && !message.text.trim().is_empty())
        {
            replied_job_ids.insert(message.job_id.clone());
            if is_explicit_rejection(&message.text) {
                rejected_job_ids.insert(message.job_id);
            }
        }

        Ok(jobs
            .into_iter()
            .map(|job| {
                let communication_status = if rejected_job_ids.contains(&job.id) {
                    CommunicationStatus::Rejected
                } else if replied_job_ids.contains(&job.id) {
                    CommunicationStatus::Replied
                } else {
                    CommunicationStatus::NoReply
                };
                JobListItem {
                    job,
                    communication_status,
                }
            })
            .collect())
    })();

    match result {
        Ok(list) => CommandResult::ok(list),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

#[cfg(test)]
mod communication_status_tests {
    use super::is_explicit_rejection;

    #[test]
    fn recognizes_explicit_rejection_messages() {
        assert!(is_explicit_rejection("您好，您的经历和岗位不太合适"));
        assert!(is_explicit_rejection("这个职位已经招满了"));
        assert!(is_explicit_rejection("目前没有 HC，感谢关注"));
    }

    #[test]
    fn does_not_treat_normal_replies_as_rejection() {
        assert!(!is_explicit_rejection("你好，可以发一份简历吗？"));
        assert!(!is_explicit_rejection("感谢关注，我们先沟通一下"));
    }
}

#[derive(Debug, Serialize)]
pub struct OverviewMetrics {
    pub total_jobs: usize,
    pub communicated_jobs: usize,
    pub replied_jobs: usize,
    pub reply_rate: f64,
    pub resume_sent_jobs: usize,
    pub high_match_jobs: usize,
    /// 窗口内已经跑过 AI 分析的岗位数，用于区分「没数据」和「匹配度低」
    pub analyzed_jobs: usize,
}

#[derive(Debug, Serialize)]
pub struct OverviewDailyActivity {
    pub date: String,
    pub jobs: usize,
    pub replies: usize,
    pub communicated: usize,
    pub resume_sent: usize,
    pub high_match: usize,
}

/// 岗位来源分布切片
#[derive(Debug, Serialize)]
pub struct OverviewSourceSlice {
    pub source: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct OverviewConversation {
    pub job_id: String,
    pub company_name: String,
    pub title: String,
    pub last_message: String,
    pub last_message_at: i64,
    /// 最后一条消息是否来自招聘方
    pub received: bool,
    /// 会话内是否出现过招聘方消息
    pub has_reply: bool,
    pub message_count: usize,
}

#[derive(Debug, Serialize)]
pub struct JobSearchOverview {
    pub days: u32,
    pub metrics: OverviewMetrics,
    /// 上一个等长周期的指标，用于计算环比变化
    pub previous_metrics: OverviewMetrics,
    pub daily_activity: Vec<OverviewDailyActivity>,
    pub source_distribution: Vec<OverviewSourceSlice>,
    pub active_conversations: Vec<OverviewConversation>,
    /// 高匹配的判定分数线，取自默认求职方案的岗位分析配置
    pub high_match_score: u8,
}

/// `days` 为 0 表示只统计今日（自然日），其余表示最近 N 天
#[tauri::command]
pub fn job_search_overview(
    app_handle: tauri::AppHandle,
    days: Option<u32>,
) -> CommandResult<JobSearchOverview> {
    // 概览是跨方案的全局视图，阈值取默认方案；读不到配置时回落到内置默认值
    let high_match_score = config::load_app_config_inner(app_handle)
        .ok()
        .and_then(|app_config| {
            app_config
                .job_profile(None)
                .ok()
                .map(|profile| profile.analysis_config.high_match_score)
        })
        .unwrap_or(config::DEFAULT_HIGH_MATCH_SCORE);

    match build_job_search_overview(days.unwrap_or(30).clamp(0, 365), high_match_score) {
        Ok(overview) => CommandResult::ok(overview),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

/// 单个统计周期的取数结果
struct PeriodSnapshot<'a> {
    metrics: OverviewMetrics,
    jobs: Vec<&'a JobDetail>,
    messages: Vec<&'a ChatMessageRecord>,
}

/// 统计窗口取半开区间 [start, end)
fn collect_period<'a>(
    jobs: &'a [JobDetail],
    messages: &'a [ChatMessageRecord],
    high_match_ids: &HashSet<&str>,
    analyzed_ids: &HashSet<&str>,
    start: chrono::DateTime<Local>,
    end: chrono::DateTime<Local>,
) -> PeriodSnapshot<'a> {
    let start_millis = start.timestamp_millis();
    let end_millis = end.timestamp_millis();
    let recent_message_job_ids: HashSet<&str> = messages
        .iter()
        .filter(|message| message.time >= start_millis && message.time < end_millis)
        .map(|message| message.job_id.as_str())
        .collect();

    let selected_jobs: Vec<&JobDetail> = jobs
        .iter()
        .filter(|job| {
            recent_message_job_ids.contains(job.id.as_str())
                || job
                    .resume_sent_at
                    .as_deref()
                    .into_iter()
                    .chain([job.updated_at.as_str(), job.created_at.as_str()])
                    .filter_map(parse_local_datetime)
                    .any(|time| time >= start && time < end)
        })
        .collect();
    let selected_job_ids: HashSet<&str> = selected_jobs.iter().map(|job| job.id.as_str()).collect();
    let selected_messages: Vec<&ChatMessageRecord> = messages
        .iter()
        .filter(|message| {
            selected_job_ids.contains(message.job_id.as_str())
                && message.time >= start_millis
                && message.time < end_millis
        })
        .collect();

    let communicated_ids: HashSet<&str> = selected_messages
        .iter()
        .map(|message| message.job_id.as_str())
        .chain(
            selected_jobs
                .iter()
                .filter(|job| job.is_reply)
                .map(|job| job.id.as_str()),
        )
        .collect();
    let replied_ids: HashSet<&str> = selected_messages
        .iter()
        .filter(|message| message.received)
        .map(|message| message.job_id.as_str())
        .chain(
            selected_jobs
                .iter()
                .filter(|job| job.is_reply)
                .map(|job| job.id.as_str()),
        )
        .collect();
    let communicated_jobs = communicated_ids.len();

    let metrics = OverviewMetrics {
        total_jobs: selected_jobs.len(),
        communicated_jobs,
        replied_jobs: replied_ids.len(),
        reply_rate: if communicated_jobs == 0 {
            0.0
        } else {
            replied_ids.len() as f64 * 100.0 / communicated_jobs as f64
        },
        resume_sent_jobs: selected_jobs
            .iter()
            .filter(|job| job.is_send_resume)
            .count(),
        high_match_jobs: selected_jobs
            .iter()
            .filter(|job| high_match_ids.contains(job.id.as_str()))
            .count(),
        analyzed_jobs: selected_jobs
            .iter()
            .filter(|job| analyzed_ids.contains(job.id.as_str()))
            .count(),
    };

    PeriodSnapshot {
        metrics,
        jobs: selected_jobs,
        messages: selected_messages,
    }
}

fn start_of_day(moment: chrono::DateTime<Local>) -> chrono::DateTime<Local> {
    moment
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|naive| Local.from_local_datetime(&naive).single())
        .unwrap_or(moment)
}

/// 岗位来源平台展示名，与岗位管理页的判定保持一致
fn platform_label(job: &JobDetail) -> &'static str {
    if job.platform == "liepin" || job.id.starts_with("liepin:") {
        "猎聘"
    } else {
        "BOSS 直聘"
    }
}

fn build_source_distribution(jobs: &[&JobDetail]) -> Vec<OverviewSourceSlice> {
    let mut counter: HashMap<&'static str, usize> = HashMap::new();
    for job in jobs {
        *counter.entry(platform_label(job)).or_default() += 1;
    }
    let mut slices: Vec<OverviewSourceSlice> = counter
        .into_iter()
        .map(|(source, count)| OverviewSourceSlice {
            source: source.to_string(),
            count,
        })
        .collect();
    // 「其他」固定排在末尾，其余按数量降序
    slices.sort_by(|left, right| {
        let rank = |slice: &OverviewSourceSlice| u8::from(slice.source == "其他");
        rank(left)
            .cmp(&rank(right))
            .then(right.count.cmp(&left.count))
            .then(left.source.cmp(&right.source))
    });
    slices
}

fn build_job_search_overview(
    days: u32,
    high_match_score: u8,
) -> anyhow::Result<JobSearchOverview> {
    let jobs = job_detail_dao::list()?;
    let messages = chat_message_dao::list()?;
    let analyses = analysis_dao::list()?;
    let now = Local::now();
    let today_start = start_of_day(now);
    // 统计窗口右端固定到今天结束，days = 0 时只覆盖今天
    let end = today_start + Duration::days(1);
    let start = if days == 0 {
        today_start
    } else {
        now - Duration::days(i64::from(days))
    };
    let span = end - start;

    let high_match_ids: HashSet<&str> = analyses
        .iter()
        .filter(|analysis| analysis.match_score >= high_match_score)
        .map(|analysis| analysis.job_id.as_str())
        .collect();
    let analyzed_ids: HashSet<&str> = analyses
        .iter()
        .map(|analysis| analysis.job_id.as_str())
        .collect();

    let current = collect_period(&jobs, &messages, &high_match_ids, &analyzed_ids, start, end);
    let previous = collect_period(
        &jobs,
        &messages,
        &high_match_ids,
        &analyzed_ids,
        start - span,
        start,
    );
    let source_distribution = build_source_distribution(&current.jobs);

    // 趋势图独立于统计窗口取全量数据，保证「今日」视图仍能看到走势
    let trend_days = days.clamp(7, 30);
    let mut jobs_by_date: HashMap<NaiveDate, usize> = HashMap::new();
    let mut resume_sent_by_date: HashMap<NaiveDate, usize> = HashMap::new();
    let mut high_match_by_date: HashMap<NaiveDate, usize> = HashMap::new();
    for job in &jobs {
        let created = job
            .resume_sent_at
            .as_deref()
            .and_then(parse_local_datetime)
            .or_else(|| parse_local_datetime(&job.created_at));
        if let Some(time) = created {
            *jobs_by_date.entry(time.date_naive()).or_default() += 1;
            if high_match_ids.contains(job.id.as_str()) {
                *high_match_by_date.entry(time.date_naive()).or_default() += 1;
            }
        }
        if job.is_send_resume {
            if let Some(time) = job.resume_sent_at.as_deref().and_then(parse_local_datetime) {
                *resume_sent_by_date.entry(time.date_naive()).or_default() += 1;
            }
        }
    }
    let mut replies_by_date: HashMap<NaiveDate, usize> = HashMap::new();
    let mut communicated_by_date: HashMap<NaiveDate, HashSet<&str>> = HashMap::new();
    for message in &messages {
        let Some(time) = Local.timestamp_millis_opt(message.time).single() else {
            continue;
        };
        let date = time.date_naive();
        if message.received {
            *replies_by_date.entry(date).or_default() += 1;
        }
        communicated_by_date
            .entry(date)
            .or_default()
            .insert(message.job_id.as_str());
    }
    let daily_activity = (0..trend_days)
        .rev()
        .map(|offset| {
            let date = (now - Duration::days(i64::from(offset))).date_naive();
            OverviewDailyActivity {
                date: date.format("%m-%d").to_string(),
                jobs: jobs_by_date.get(&date).copied().unwrap_or(0),
                replies: replies_by_date.get(&date).copied().unwrap_or(0),
                communicated: communicated_by_date.get(&date).map_or(0, HashSet::len),
                resume_sent: resume_sent_by_date.get(&date).copied().unwrap_or(0),
                high_match: high_match_by_date.get(&date).copied().unwrap_or(0),
            }
        })
        .collect();

    let job_map: HashMap<&str, &JobDetail> =
        jobs.iter().map(|job| (job.id.as_str(), job)).collect();
    let mut grouped: HashMap<&str, Vec<&ChatMessageRecord>> = HashMap::new();
    for message in current.messages {
        grouped
            .entry(message.job_id.as_str())
            .or_default()
            .push(message);
    }
    let mut active_conversations: Vec<OverviewConversation> = grouped
        .into_iter()
        .filter_map(|(job_id, mut items)| {
            let job = job_map.get(job_id)?;
            items.sort_by_key(|message| message.time);
            let last = items.last()?;
            Some(OverviewConversation {
                job_id: job_id.to_string(),
                company_name: job.company_name.clone(),
                title: job.title.clone(),
                last_message: last.text.clone(),
                last_message_at: last.time,
                received: last.received,
                has_reply: items.iter().any(|message| message.received),
                message_count: items.len(),
            })
        })
        .collect();
    active_conversations
        .sort_by_key(|conversation| std::cmp::Reverse(conversation.last_message_at));
    active_conversations.truncate(8);

    Ok(JobSearchOverview {
        days,
        metrics: current.metrics,
        previous_metrics: previous.metrics,
        daily_activity,
        source_distribution,
        active_conversations,
        high_match_score,
    })
}

fn parse_local_datetime(value: &str) -> Option<chrono::DateTime<Local>> {
    NaiveDateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S")
        .ok()
        .and_then(|value| Local.from_local_datetime(&value).single())
}

#[tauri::command]
pub fn job_get(id: String) -> CommandResult<JobDetail> {
    match job_detail_dao::get_by_id(&id) {
        Ok(Some(job)) => CommandResult::ok(job),
        Ok(None) => CommandResult::err(format!("岗位不存在: {}", id)),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn job_create(job: JobDetail) -> CommandResult<()> {
    match job_detail_dao::create(job) {
        Ok(()) => CommandResult::ok(()),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn job_update(id: String, job: JobDetail) -> CommandResult<()> {
    match job_detail_dao::update(&id, job) {
        Ok(true) => CommandResult::ok(()),
        Ok(false) => CommandResult::err(format!("岗位不存在: {}", id)),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn job_delete(id: String) -> CommandResult<()> {
    match job_detail_dao::delete(&id) {
        Ok(true) => CommandResult::ok(()),
        Ok(false) => CommandResult::err(format!("岗位不存在: {}", id)),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

/// 岗位描述的结构化视图。
///
/// 前端不再自己洗一遍 JD：清洗规则跟着平台页面结构走，前后端各存一份必然漂移。
/// 页面要展示什么就从这个出口取，和喂给模型的是同一份文本
#[tauri::command]
pub fn job_description_view(job_id: String) -> CommandResult<ParsedJobDescription> {
    match job_detail_dao::get_by_id(&job_id) {
        Ok(Some(job)) => CommandResult::ok(crate::job_description::parse(
            &job.detail,
            &job.platform,
        )),
        Ok(None) => CommandResult::err(format!("岗位不存在: {}", job_id)),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn chat_messages_by_job(job_id: String) -> CommandResult<Vec<ChatMessageRecord>> {
    match chat_message_dao::find_by_job_id(&job_id) {
        Ok(list) => CommandResult::ok(list),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[derive(serde::Deserialize)]
pub struct JobQueryParam {
    pub company_name: Option<String>,
    pub replied_only: Option<bool>,
    pub resume_sent_only: Option<bool>,
}

#[tauri::command]
pub fn job_query(param: JobQueryParam) -> CommandResult<Vec<JobDetail>> {
    let result = if let Some(ref name) = param.company_name {
        job_detail_dao::find_by_company(name)
    } else if param.replied_only == Some(true) {
        job_detail_dao::find_replied()
    } else if param.resume_sent_only == Some(true) {
        job_detail_dao::find_resume_sent()
    } else {
        job_detail_dao::list()
    };

    match result {
        Ok(list) => CommandResult::ok(list),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[derive(Deserialize)]
struct LlmAnalysisOutput {
    fit_summary: String,
    match_score: u8,
    strengths: Vec<String>,
    risks: Vec<String>,
    skill_matrix: Vec<crate::dao::model::SkillEvidence>,
    likely_questions: Vec<crate::dao::model::InterviewQuestion>,
    questions_to_ask_interviewer: Vec<String>,
}

fn build_analysis_prompt(job: &JobDetail) -> String {
    // 抓下来的 JD 混着反爬注入的样式代码和噪声词，原样喂进去既占额度又干扰判断，
    // 统一走 job_description 这个出口洗一遍
    let detail = crate::job_description::clean_text(&job.detail, &job.platform);
    // 只填岗位骨架；resume_context / background_context / chat_context 这些
    // 业务变量留给后续的模板渲染，两层的替换时机不同不能混做
    crate::agent::prompts::compose(
        &crate::agent::prompts::with_shared(crate::agent::prompts::JOB_ANALYSIS),
        &[
            ("JOB_TITLE", &job.title),
            ("JOB_COMPANY", &job.company_name),
            ("JOB_SALARY", &job.salary),
            ("JOB_LOCATION", job.location.as_deref().unwrap_or("-")),
            ("JOB_DETAIL", &detail),
        ],
    )
}

fn format_chat_context(messages: &[ChatMessageRecord]) -> String {
    messages
        .iter()
        .map(|message| {
            let role = if message.received { "招聘方" } else { "我" };
            format!("{}({}): {}", message.from_name, role, message.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tauri::command]
pub async fn job_analyze(
    app_handle: tauri::AppHandle,
    job_id: String,
) -> CommandResult<InterviewJobAnalysis> {
    let job = match job_detail_dao::get_by_id(&job_id) {
        Ok(Some(j)) => j,
        Ok(None) => return CommandResult::err(format!("岗位不存在: {}", job_id)),
        Err(e) => return CommandResult::err(e.to_string()),
    };

    let app_config = match config::load_app_config_inner(app_handle) {
        Ok(c) => c,
        Err(e) => return CommandResult::err(format!("加载配置失败: {}", e)),
    };

    match analyze_job(&job, &app_config).await {
        Ok(analysis) => CommandResult::ok(analysis),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

#[derive(Debug, Default, Serialize)]
pub struct BatchAnalysisResult {
    pub analyzed: usize,
    pub skipped: usize,
    pub failed: usize,
    /// 「岗位名：原因」形式的失败摘要，界面直接展示
    pub failures: Vec<String>,
}

/// 批量分析选中的岗位。
///
/// 串行执行：分析是重调用，并发跑既容易触发服务端限流，也会和求职任务抢配额。
#[tauri::command]
pub async fn job_analyze_batch(
    app_handle: tauri::AppHandle,
    job_ids: Vec<String>,
    skip_analyzed: Option<bool>,
) -> CommandResult<BatchAnalysisResult> {
    let app_config = match config::load_app_config_inner(app_handle) {
        Ok(c) => c,
        Err(e) => return CommandResult::err(format!("加载配置失败: {}", e)),
    };
    if app_config.llm_chain().is_empty() {
        return CommandResult::err("请先配置大模型服务".to_string());
    }
    let skip_analyzed = skip_analyzed.unwrap_or(true);

    let mut result = BatchAnalysisResult::default();
    for job_id in job_ids {
        let job = match job_detail_dao::get_by_id(&job_id) {
            Ok(Some(job)) => job,
            Ok(None) => {
                result.failed += 1;
                result.failures.push(format!("{job_id}：岗位不存在"));
                continue;
            }
            Err(error) => {
                result.failed += 1;
                result.failures.push(format!("{job_id}：{error}"));
                continue;
            }
        };
        if skip_analyzed
            && matches!(analysis_dao::get_by_job_id(&job_id), Ok(Some(existing)) if existing.parse_error.is_none())
        {
            result.skipped += 1;
            continue;
        }
        match analyze_job(&job, &app_config).await {
            Ok(analysis) if analysis.parse_error.is_none() => {
                result.analyzed += 1;
                let _ = crate::logger::info(format!(
                    "已分析岗位「{}」，匹配度 {} 分",
                    job.title, analysis.match_score
                ));
            }
            Ok(_) => {
                result.failed += 1;
                result
                    .failures
                    .push(format!("{}：模型输出解析不完整", job.title));
            }
            Err(error) => {
                result.failed += 1;
                result.failures.push(format!("{}：{error}", job.title));
            }
        }
    }

    CommandResult::ok(result)
}

/// 跑一次岗位分析并落库。
///
/// 界面手动触发和 RPA 自动触发共用这条路径，区别只在于谁提供配置快照——
/// 自动触发时配置来自任务绑定的方案快照，不能再回头去读当前界面上的配置。
pub async fn analyze_job(
    job: &JobDetail,
    app_config: &config::AppRuntimeConfig,
) -> anyhow::Result<InterviewJobAnalysis> {
    let job_id = job.id.clone();
    let resume_context = app_config
        .resume_config
        .resume_content
        .clone()
        .unwrap_or_default();
    let background_context = app_config
        .replay_config
        .background_context
        .clone()
        .unwrap_or_default();
    let chat_messages = match chat_message_dao::find_by_job_id(&job_id) {
        Ok(mut messages) => {
            messages.sort_by_key(|message| message.time);
            messages
        }
        Err(e) => anyhow::bail!("加载沟通记录失败: {}", e),
    };
    let chat_context = format_chat_context(&chat_messages);

    let prompt_template = build_analysis_prompt(job);
    let params = serde_json::json!({
        "resume_context": resume_context,
        "background_context": background_context,
        "chat_context": chat_context,
    });
    // 与其他所有模型用途一样走统一的 Agent 循环：流式采集、重试降级、输出净化
    let task = crate::agent::tasks::TemplateTask::new("岗位面试分析", &prompt_template, params);
    let raw = match crate::agent::run(&task, app_config).await {
        Ok(outcome) => outcome.output,
        Err(e) => anyhow::bail!("生成分析失败: {}", e),
    };
    let analyzed_at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let analysis = match serde_json::from_str::<LlmAnalysisOutput>(&raw) {
        Ok(output) => InterviewJobAnalysis {
            job_id: job_id.clone(),
            analyzed_at,
            fit_summary: output.fit_summary,
            match_score: output.match_score,
            strengths: output.strengths,
            risks: output.risks,
            skill_matrix: output.skill_matrix,
            likely_questions: output.likely_questions,
            questions_to_ask_interviewer: output.questions_to_ask_interviewer,
            search_summary: String::new(),
            search_sources: vec![],
            chat_context,
            raw_response: raw,
            parse_error: None,
        },
        Err(e) => InterviewJobAnalysis {
            job_id: job_id.clone(),
            analyzed_at,
            fit_summary: String::new(),
            match_score: 0,
            strengths: vec![],
            risks: vec![],
            skill_matrix: vec![],
            likely_questions: vec![],
            questions_to_ask_interviewer: vec![],
            search_summary: String::new(),
            search_sources: vec![],
            chat_context,
            raw_response: raw,
            parse_error: Some(e.to_string()),
        },
    };

    let save_result = match analysis_dao::get_by_job_id(&job_id) {
        Ok(Some(_)) => analysis_dao::update(&job_id, analysis.clone()).map(|_| ()),
        Ok(None) => analysis_dao::create(analysis.clone()),
        Err(e) => Err(e),
    };
    if let Err(e) = save_result {
        anyhow::bail!("保存分析结果失败: {}", e);
    }

    Ok(analysis)
}
