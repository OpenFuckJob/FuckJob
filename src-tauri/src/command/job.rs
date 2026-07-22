use crate::command::base::CommandResult;
use crate::config;
use crate::dao::model::{ChatMessageRecord, InterviewJobAnalysis, JobDetail};
use crate::dao::{analysis_dao, chat_message_dao, job_detail_dao};
use crate::llm::service::LlmService;
use serde::Deserialize;

#[tauri::command]
pub fn job_list() -> CommandResult<Vec<JobDetail>> {
    match job_detail_dao::list() {
        Ok(list) => CommandResult::ok(list),
        Err(e) => CommandResult::err(e.to_string()),
    }
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
    let credential = match crate::credential::resolve() {
        Ok(v) => v,
        Err(e) => return CommandResult::err(e),
    };
    let service = match LlmService::from_runtime(&app_config, &credential) {
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
