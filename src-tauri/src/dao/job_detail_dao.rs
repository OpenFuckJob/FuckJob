use crate::dao::model::JobDetail;
use crate::dao::store::{BatchResult, JsonStore};
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

/// 平台侧有时会给出带/不带平台前缀的岗位标识，按两种形式查找归属。
pub fn find_by_platform_job_id(platform: &str, id: &str) -> Result<Option<JobDetail>> {
    if let Some(job) = get_by_id(id)? {
        return Ok(Some(job));
    }
    let prefixed = format!("{platform}:{id}");
    if let Some(job) = get_by_id(&prefixed)? {
        return Ok(Some(job));
    }
    Ok(list()?.into_iter().find(|job| {
        job.platform.eq_ignore_ascii_case(platform)
            && job
                .id
                .strip_prefix(&format!("{platform}:"))
                .unwrap_or(&job.id)
                == id
    }))
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

pub fn batch_upsert<F>(items: Vec<JobDetail>, should_update: F) -> Result<BatchResult>
where
    F: Fn(&JobDetail, &JobDetail) -> bool,
{
    store().batch_upsert(items, should_update)
}

pub fn replace_all(items: Vec<JobDetail>) -> Result<()> {
    store().replace_all(items)
}
