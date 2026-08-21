use crate::agent::tasks::{JobFilterRulesTask, ResumeOptimizeTask, ResumeQuestionsTask};
use crate::command::base::CommandResult;
use crate::config::RegexRule;
use crate::error::AppError;
use serde::Serialize;

// 这些类型定义在 agent 层，命令层只做转发：调试入口和实际运行必须是同一条链路，
// 各自维护一份输入结构迟早会漂移
pub use crate::agent::tasks::{OptimizeWithAnswerRequest, PredictedQuestion};

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
