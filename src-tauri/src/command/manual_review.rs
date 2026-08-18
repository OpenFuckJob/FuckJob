use crate::command::base::CommandResult;
use crate::dao::manual_review_dao;
use crate::dao::model::ManualReviewRecord;

#[tauri::command]
pub fn manual_review_list() -> CommandResult<Vec<ManualReviewRecord>> {
    match manual_review_dao::list() {
        Ok(list) => CommandResult::ok(list),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

/// 未处理条数。用于在任务面板上挂角标，不必拉整张列表
#[tauri::command]
pub fn manual_review_count() -> CommandResult<usize> {
    match manual_review_dao::count() {
        Ok(count) => CommandResult::ok(count),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

/// 手工消项。用户在平台里回过了、或者判断无需处理时调用。
///
/// 记录不存在也算成功：用户点「已处理」时它可能刚被自动消项消掉，
/// 这种情况报错只会让人以为操作失败
#[tauri::command]
pub fn manual_review_resolve(platform: String, conversation_id: String) -> CommandResult<()> {
    match manual_review_dao::resolve(&platform, &conversation_id) {
        Ok(_) => CommandResult::ok(()),
        Err(error) => CommandResult::err(error.to_string()),
    }
}

#[tauri::command]
pub fn manual_review_clear() -> CommandResult<()> {
    match manual_review_dao::clear() {
        Ok(()) => CommandResult::ok(()),
        Err(error) => CommandResult::err(error.to_string()),
    }
}
