pub mod analysis_dao;
pub mod chat_message_dao;
pub mod job_detail_dao;
pub mod model;
pub mod store;

use anyhow::Result;
use std::path::Path;

pub fn init(data_dir: &Path) -> Result<()> {
    job_detail_dao::init(data_dir)?;
    analysis_dao::init(data_dir)?;
    chat_message_dao::init(data_dir)?;
    Ok(())
}
