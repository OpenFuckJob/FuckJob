use crate::dao::model::JobDetail;
use crate::dao::store::JsonStore;
use anyhow::Result;
use std::path::Path;
use std::sync::OnceLock;

static STORE: OnceLock<JsonStore<JobDetail>> = OnceLock::new();

pub fn init(data_dir: &Path) -> Result<()> {
    let store = JsonStore::new(data_dir, "job_details.json")?;
    STORE
        .set(store)
        .map_err(|_| anyhow::anyhow!("JobDetailDao 已经初始化"))?;
    Ok(())
}

fn store() -> &'static JsonStore<JobDetail> {
    STORE.get().expect("JobDetailDao 未初始化")
}

pub fn list() -> Result<Vec<JobDetail>> {
    store().load_all()
}

pub fn get_by_id(id: &str) -> Result<Option<JobDetail>> {
    store().get_by_id(id)
}

pub fn create(job: JobDetail) -> Result<()> {
    store().insert(job)
}

pub fn update(id: &str, job: JobDetail) -> Result<bool> {
    store().update_by_id(id, job)
}

pub fn delete(id: &str) -> Result<bool> {
    store().delete_by_id(id)
}

pub fn find_by_company(name: &str) -> Result<Vec<JobDetail>> {
    let name_lower = name.to_lowercase();
    store().query(|j| j.company_name.to_lowercase().contains(&name_lower))
}

pub fn find_replied() -> Result<Vec<JobDetail>> {
    store().query(|j| j.is_reply)
}

pub fn find_resume_sent() -> Result<Vec<JobDetail>> {
    store().query(|j| j.is_send_resume)
}
