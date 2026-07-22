use crate::dao::model::InterviewJobAnalysis;
use crate::dao::store::JsonStore;
use anyhow::Result;
use std::path::Path;
use std::sync::OnceLock;

static STORE: OnceLock<JsonStore<InterviewJobAnalysis>> = OnceLock::new();

pub fn init(data_dir: &Path) -> Result<()> {
    let store = JsonStore::new(data_dir, "interview_analyses.json")?;
    STORE
        .set(store)
        .map_err(|_| anyhow::anyhow!("AnalysisDao 已经初始化"))?;
    Ok(())
}

fn store() -> &'static JsonStore<InterviewJobAnalysis> {
    STORE.get().expect("AnalysisDao 未初始化")
}

pub fn list() -> Result<Vec<InterviewJobAnalysis>> {
    store().load_all()
}

pub fn get_by_job_id(job_id: &str) -> Result<Option<InterviewJobAnalysis>> {
    store().get_by_id(job_id)
}

pub fn create(analysis: InterviewJobAnalysis) -> Result<()> {
    store().insert(analysis)
}

pub fn update(job_id: &str, analysis: InterviewJobAnalysis) -> Result<bool> {
    store().update_by_id(job_id, analysis)
}

pub fn delete(job_id: &str) -> Result<bool> {
    store().delete_by_id(job_id)
}
