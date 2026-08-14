use crate::agent::tasks::{
    GreetTask, JobFilterRulesTask, ReplyDecisionTask, ResumeOptimizeTask, ResumeQuestionsTask,
};
use crate::command::base::CommandResult;
use crate::config::RegexRule;
use crate::error::AppError;
use crate::rpa::common::{ChatMessage, RpaJob};
use crate::rpa::conversation::{ConversationContext, GreetAction, ReplyDecision, ResumeState};
use crate::rpa::run_flow::PlatformKind;
use serde::{Deserialize, Serialize};

// 这些类型定义在 agent 层，命令层只做转发：调试入口和实际运行必须是同一条链路，
// 各自维护一份输入结构迟早会漂移
pub use crate::agent::tasks::{OptimizeWithAnswerRequest, PredictedQuestion};

#[derive(Debug, Deserialize)]
pub struct DebugReplayRequest {
    pub job_title: String,
    pub company_name: String,
    pub job_detail: String,
    pub salary: String,
    pub location: String,
    pub messages: Vec<DebugChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugChatMessage {
    pub text: String,
    pub from_name: String,
    pub received: bool,
}

#[derive(Debug, Deserialize)]
pub struct DebugGreetRequest {
    pub job_title: String,
    pub company_name: String,
    pub job_detail: String,
    pub salary: String,
    pub location: String,
}

#[derive(Debug, Serialize)]
pub struct ResumeLlmResult {
    pub success: bool,
    pub data: String,
}

#[tauri::command]
pub async fn generate_job_filter_rules(
    app_handle: tauri::AppHandle,
    requirement: String,
) -> CommandResult<Vec<RegexRule>> {
    let requirement = requirement.trim();
    if requirement.is_empty() {
        return CommandResult::err(AppError::validation("请输入岗位筛选需求"));
    }

    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(value) => value,
        Err(error) => return CommandResult::err(error),
    };

    match crate::agent::run(&JobFilterRulesTask::new(requirement), &config).await {
        Ok(outcome) => CommandResult::ok(outcome.output),
        Err(error) => CommandResult::err(error),
    }
}

/// 调试入口：预演一次自动回复。
///
/// 走的是和实际运行**完全相同**的任务，包括决策、护栏与发送前体检。
/// 此前这里自己拼了一套参数（还漏掉了 chat_history，导致调试页必然报错），
/// 于是调试看到的效果和真正跑出来的并不是一回事。
#[tauri::command]
pub async fn debug_generate_replay(
    app_handle: tauri::AppHandle,
    req: DebugReplayRequest,
) -> CommandResult<String> {
    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(cfg) => cfg,
        Err(err) => return CommandResult::err(err),
    };

    let context = ConversationContext {
        platform: PlatformKind::Boss,
        conversation_id: "debug".to_string(),
        job: Some(debug_job_detail(&req)),
        messages: req
            .messages
            .iter()
            .enumerate()
            .map(|(index, message)| ChatMessage {
                mid: index as i64,
                received: message.received,
                text: message.text.clone(),
                time: index as i64,
                from_name: message.from_name.clone(),
            })
            .collect(),
        // 调试没有真实页面可读，按最保守的状态给：不允许预演里出现投递动作
        resume_state: ResumeState::Unknown,
    };

    match crate::agent::run(&ReplyDecisionTask::new(&config, &context), &config).await {
        Ok(outcome) => CommandResult::ok(describe_decision(&outcome.output)),
        Err(err) => CommandResult::err(err),
    }
}

/// 决策结果渲染成调试页能直接看的一段文本。
///
/// 不回复和转人工也要说清楚，否则用户只看到空白，会以为是生成失败
fn describe_decision(decision: &ReplyDecision) -> String {
    if decision.action.needs_text() {
        return decision.reply.clone();
    }
    format!(
        "（AI 判断本轮不发送：{}。理由：{}）",
        match decision.action {
            crate::rpa::conversation::ReplyAction::Skip => "无需回复",
            _ => "需要转人工",
        },
        decision.reason
    )
}

fn debug_job_detail(req: &DebugReplayRequest) -> crate::dao::model::JobDetail {
    crate::dao::model::JobDetail {
        id: "debug".to_string(),
        platform: "boss".to_string(),
        source_task_id: None,
        profile_id: None,
        profile_name: None,
        profile_snapshot_id: None,
        title: req.job_title.clone(),
        company_name: req.company_name.clone(),
        detail: req.job_detail.clone(),
        salary: req.salary.clone(),
        location: Some(req.location.clone()),
        is_reply: false,
        is_send_resume: false,
        created_at: String::new(),
        resume_sent_at: None,
        updated_at: String::new(),
    }
}

/// 调试入口：预演一次打招呼。与实际运行共用同一个任务
#[tauri::command]
pub async fn debug_generate_greet(
    app_handle: tauri::AppHandle,
    req: DebugGreetRequest,
) -> CommandResult<String> {
    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(cfg) => cfg,
        Err(err) => return CommandResult::err(err),
    };

    let job = RpaJob {
        platform: PlatformKind::Boss,
        platform_job_id: "debug".to_string(),
        title: req.job_title,
        company_name: req.company_name,
        detail: req.job_detail,
        salary: req.salary,
        location: Some(req.location),
        detail_url: String::new(),
    };

    match crate::agent::run(&GreetTask::new(&config, &job), &config).await {
        Ok(outcome) => {
            let decision = outcome.output;
            // 「不该投」也要让用户在调试页看见，否则只看到空白会以为是生成失败
            if decision.action == GreetAction::Skip {
                return CommandResult::ok(format!(
                    "（AI 判断该岗位不适合投递，实际运行时整轮都不会发送。理由：{}）",
                    decision.reason
                ));
            }
            CommandResult::ok(decision.greeting)
        }
        Err(err) => CommandResult::err(err),
    }
}

#[tauri::command]
pub async fn predict_resume_questions(
    app_handle: tauri::AppHandle,
    resume_content: String,
) -> CommandResult<Vec<PredictedQuestion>> {
    if resume_content.trim().is_empty() {
        return CommandResult::err("请先输入/导入简历内容");
    }

    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(v) => v,
        Err(e) => return CommandResult::err(e),
    };

    match crate::agent::run(&ResumeQuestionsTask::new(&resume_content), &config).await {
        Ok(outcome) => CommandResult::ok(outcome.output),
        Err(err) => CommandResult::err(err),
    }
}

#[tauri::command]
pub async fn optimize_resume_with_answer(
    app_handle: tauri::AppHandle,
    request: OptimizeWithAnswerRequest,
) -> CommandResult<ResumeLlmResult> {
    if request.resume_content.trim().is_empty() {
        return CommandResult::err("请先输入/导入简历内容");
    }
    if request.question.trim().is_empty() {
        return CommandResult::err("请选择要回答的问题");
    }
    if request.user_answer.trim().is_empty() {
        return CommandResult::err("请输入您的真实回答");
    }
    if request.section_title.trim().is_empty() {
        return CommandResult::err("缺少关联优化章节");
    }

    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(v) => v,
        Err(e) => return CommandResult::err(e),
    };

    match crate::agent::run(&ResumeOptimizeTask::new(&request), &config).await {
        Ok(outcome) => CommandResult::ok(ResumeLlmResult {
            success: true,
            data: outcome.output,
        }),
        Err(err) => CommandResult::err(err),
    }
}
