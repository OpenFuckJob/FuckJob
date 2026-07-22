use crate::command::base::CommandResult;
use crate::dao::analysis_dao;
use crate::dao::model::InterviewJobAnalysis;

#[tauri::command]
pub fn analysis_list() -> CommandResult<Vec<InterviewJobAnalysis>> {
    match analysis_dao::list() {
        Ok(list) => CommandResult::ok(list),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn analysis_get_by_job_id(job_id: String) -> CommandResult<InterviewJobAnalysis> {
    match analysis_dao::get_by_job_id(&job_id) {
        Ok(Some(analysis)) => CommandResult::ok(analysis),
        Ok(None) => CommandResult::err(format!("分析结果不存在: {}", job_id)),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn analysis_create(analysis: InterviewJobAnalysis) -> CommandResult<()> {
    match analysis_dao::create(analysis) {
        Ok(()) => CommandResult::ok(()),
        Err(e) => CommandResult::err(e.to_string()),
    }
}

#[tauri::command]
pub fn analysis_delete(job_id: String) -> CommandResult<()> {
    match analysis_dao::delete(&job_id) {
        Ok(true) => CommandResult::ok(()),
        Ok(false) => CommandResult::err(format!("分析结果不存在: {}", job_id)),
        Err(e) => CommandResult::err(e.to_string()),
    }
}
