pub mod analysis_dao;
pub mod auto_reply_log_dao;
pub mod chat_message_dao;
pub mod job_detail_dao;
pub mod manual_review_dao;
pub mod model;
pub mod profile_snapshot_dao;
pub mod store;

use anyhow::Result;
use std::path::Path;

pub(crate) fn notify_job_data_changed() {
    use tauri::Emitter;
    if let Some(handle)=crate::browser::app_handle() {
        let _=handle.emit("job-data-changed",());
    }
}

pub fn init(data_dir: &Path) -> Result<()> {
    job_detail_dao::init(data_dir)?;
    analysis_dao::init(data_dir)?;
    chat_message_dao::init(data_dir)?;
    profile_snapshot_dao::init(data_dir)?;
    auto_reply_log_dao::init(data_dir)?;
    manual_review_dao::init(data_dir)?;
    Ok(())
}
