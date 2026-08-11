use crate::command::base::CommandResult;
use crate::config;
use crate::dao::model::{ChatMessageRecord, InterviewJobAnalysis, JobDetail};
use crate::dao::{analysis_dao, chat_message_dao, job_detail_dao};
use crate::llm::service::LlmChainService;
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
}

#[derive(Debug, Serialize)]
pub struct OverviewDailyActivity {
    pub date: String,
    pub jobs: usize,
    pub replies: usize,
}

#[derive(Debug, Serialize)]
pub struct OverviewConversation {
    pub job_id: String,
    pub company_name: String,
    pub title: String,
    pub last_message: String,
    pub last_message_at: i64,
    pub received: bool,
    pub message_count: usize,
}

#[derive(Debug, Serialize)]
pub struct JobSearchOverview {
    pub days: u32,
    pub metrics: OverviewMetrics,
    pub daily_activity: Vec<OverviewDailyActivity>,
    pub active_conversations: Vec<OverviewConversation>,
}

#[tauri::command]
pub fn job_search_overview(days: Option<u32>) -> CommandResult<JobSearchOverview> {
    match build_job_search_overview(days.unwrap_or(30).clamp(1, 365)) {
        Ok(overview) => CommandResult::ok(overview),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

fn build_job_search_overview(days: u32) -> anyhow::Result<JobSearchOverview> {
    let jobs = job_detail_dao::list()?;
    let messages = chat_message_dao::list()?;
    let analyses = analysis_dao::list()?;
    let now = Local::now();
    let cutoff = now - Duration::days(i64::from(days));
    let cutoff_millis = cutoff.timestamp_millis();
    let recent_message_job_ids: HashSet<&str> = messages
        .iter()
        .filter(|message| message.time >= cutoff_millis)
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
                    .any(|time| time >= cutoff)
        })
        .collect();
    let selected_job_ids: HashSet<&str> = selected_jobs.iter().map(|job| job.id.as_str()).collect();
    let selected_messages: Vec<&ChatMessageRecord> = messages
        .iter()
        .filter(|message| {
            selected_job_ids.contains(message.job_id.as_str()) && message.time >= cutoff_millis
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
    let high_match_ids: HashSet<&str> = analyses
        .iter()
        .filter(|analysis| analysis.match_score >= 80)
        .map(|analysis| analysis.job_id.as_str())
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
    };

    let mut jobs_by_date: HashMap<NaiveDate, usize> = HashMap::new();
    for job in &selected_jobs {
        let time = job
            .resume_sent_at
            .as_deref()
            .and_then(parse_local_datetime)
            .or_else(|| parse_local_datetime(&job.created_at));
        if let Some(time) = time {
            *jobs_by_date.entry(time.date_naive()).or_default() += 1;
        }
    }
    let mut replies_by_date: HashMap<NaiveDate, usize> = HashMap::new();
    for message in selected_messages.iter().filter(|message| message.received) {
        if let Some(time) = Local.timestamp_millis_opt(message.time).single() {
            *replies_by_date.entry(time.date_naive()).or_default() += 1;
        }
    }
    let daily_activity = (0..days.min(30))
        .rev()
        .map(|offset| {
            let date = (now - Duration::days(i64::from(offset))).date_naive();
            OverviewDailyActivity {
                date: date.format("%m-%d").to_string(),
                jobs: jobs_by_date.get(&date).copied().unwrap_or(0),
                replies: replies_by_date.get(&date).copied().unwrap_or(0),
            }
        })
        .collect();

    let job_map: HashMap<&str, &JobDetail> =
        jobs.iter().map(|job| (job.id.as_str(), job)).collect();
    let mut grouped: HashMap<&str, Vec<&ChatMessageRecord>> = HashMap::new();
    for message in selected_messages {
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
                message_count: items.len(),
            })
        })
        .collect();
    active_conversations
        .sort_by_key(|conversation| std::cmp::Reverse(conversation.last_message_at));
    active_conversations.truncate(8);

    Ok(JobSearchOverview {
        days,
        metrics,
        daily_activity,
        active_conversations,
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
    let location = job.location.as_deref().unwrap_or("-");
    format!(
        r#"你是候选人的面试准备助手。请结合岗位 JD、候选人简历和背景补充，生成这个岗位专属的面试准备分析。

候选人简历：{{{{resume_context}}}}

背景补充：
{{{{background_context}}}}

岗位沟通记录：
{{{{chat_context}}}}

岗位信息：
- 职位：{title}
- 公司：{company}
- 薪资：{salary}
- 地点：{location}
- JD：
{jd}

输出要求：
只输出一个 JSON 对象，不要 Markdown，不要解释。字段必须包含：
{{
  "fit_summary": "岗位匹配度总结，指出最需要准备的方向",
  "match_score": 0,
  "strengths": ["简历中能支撑该岗位的亮点"],
  "risks": ["可能被面试官追问或质疑的薄弱点"],
  "skill_matrix": [
    {{
      "requirement": "JD 要求或隐含能力",
      "resume_evidence": "简历中对应证据，没有则写空字符串",
      "gap": "简历/JD 之间的缺口",
      "prep_action": "面试前具体准备动作"
    }}
  ],
  "likely_questions": [
    {{
      "category": "技术/项目/业务/行为/反问",
      "question": "面试官可能问的问题",
      "why": "为什么该岗位容易问这个问题",
      "answer_outline": "回答提纲，按背景-行动-结果组织"
    }}
  ],
  "questions_to_ask_interviewer": ["候选人可以反问面试官的问题"]
}}

match_score 必须是 0 到 100 的整数。likely_questions 至少给 8 个，覆盖 JD 中的核心技能、简历项目追问和沟通记录暴露的信息。"#,
        title = job.title,
        company = job.company_name,
        salary = job.salary,
        location = location,
        jd = job.detail,
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
        Err(e) => return CommandResult::err(format!("加载沟通记录失败: {}", e)),
    };
    let chat_context = format_chat_context(&chat_messages);

    let prompt_template = build_analysis_prompt(&job);
    let params = serde_json::json!({
        "resume_context": resume_context,
        "background_context": background_context,
        "chat_context": chat_context,
    });
    // 非流式调用，走带重试与降级的链式服务
    let service = match LlmChainService::from_runtime(&app_config) {
        Ok(v) => v,
        Err(e) => return CommandResult::err(e),
    };
    let vo = match service.generate_template(&prompt_template, &params).await {
        Ok(v) => v,
        Err(e) => return CommandResult::err(format!("生成分析失败: {}", e)),
    };

    let raw = vo.content;
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
        return CommandResult::err(format!("保存分析结果失败: {}", e));
    }

    CommandResult::ok(analysis)
}
