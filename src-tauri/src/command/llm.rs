use crate::agent::tasks::{
    JobFilterRulesTask, JobKeywordFilterTask, ParsedKeywordFilters,
};
use crate::command::base::CommandResult;
use crate::config::RegexRule;
use crate::error::AppError;

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
pub async fn parse_job_keyword_filters(
    app_handle: tauri::AppHandle,
    requirement: String,
) -> CommandResult<ParsedKeywordFilters> {
    let requirement = requirement.trim();
    if requirement.is_empty() {
        return CommandResult::err(AppError::validation("请输入岗位或公司关键词需求"));
    }

    let config = match crate::config::load_app_config_inner(app_handle) {
        Ok(value) => value,
        Err(error) => return CommandResult::err(error),
    };

    match crate::agent::run(&JobKeywordFilterTask::new(requirement), &config).await {
        Ok(outcome) => CommandResult::ok(outcome.output),
        Err(error) => CommandResult::err(error),
    }
}
