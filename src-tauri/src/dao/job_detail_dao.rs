use crate::dao::model::JobDetail;
use crate::dao::store::{BatchResult, JsonStore};
use anyhow::Result;
use std::path::Path;
use std::sync::OnceLock;

static STORE: OnceLock<JsonStore<JobDetail>> = OnceLock::new();

pub fn init(data_dir: &Path) -> Result<()> {
    let store = JsonStore::<JobDetail>::new(data_dir, "job_details.json")?;
    store.transaction(|jobs| {
        let mut changed = false;
        for job in jobs {
            let before = job.source_url.clone();
            fill_source_url(job);
            changed |= before != job.source_url;
        }
        Ok(((), changed))
    })?;
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

pub fn create(mut job: JobDetail) -> Result<()> {
    fill_source_url(&mut job);
    store().insert(job)?;
    super::notify_job_data_changed();
    Ok(())
}

pub fn update(id: &str, mut job: JobDetail) -> Result<bool> {
    fill_source_url(&mut job);
    let changed=store().update_by_id(id, job)?;
    if changed { super::notify_job_data_changed(); }
    Ok(changed)
}

pub fn delete(id: &str) -> Result<bool> {
    let changed=store().delete_by_id(id)?;
    if changed { super::notify_job_data_changed(); }
    Ok(changed)
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

pub fn batch_upsert<F>(mut items: Vec<JobDetail>, should_update: F) -> Result<BatchResult>
where
    F: Fn(&JobDetail, &JobDetail) -> bool,
{
    for job in &mut items { fill_source_url(job); }
    let result=store().batch_upsert(items, should_update)?;
    super::notify_job_data_changed();
    Ok(result)
}

pub fn replace_all(mut items: Vec<JobDetail>) -> Result<()> {
    for job in &mut items { fill_source_url(job); }
    store().replace_all(items)?;
    super::notify_job_data_changed();
    Ok(())
}

/// The UI may resolve a pending application while its worker confirms success.
/// Re-check under the same mutation lock, so a stale retry cannot undo success.
pub fn resolve_job51_delivery(id: &str, delivered: bool) -> Result<JobDetail> {
    let job=store().transaction(|jobs| {
        let job=jobs.iter_mut().find(|job|job.id==id).ok_or_else(|| anyhow::anyhow!("岗位不存在"))?;
        if job.platform != "51job" || !id.starts_with("51job:") { anyhow::bail!("该操作仅用于51job投递确认"); }
        if !job.resume_delivery_pending {
            if job.is_send_resume == delivered {
                if delivered && job.resume_sent_at.is_none() {
                    job.resume_sent_at=Some(job.updated_at.clone());
                    return Ok((job.clone(),true));
                }
                return Ok((job.clone(),false));
            }
            anyhow::bail!("该岗位的投递状态已变化，请刷新后核对");
        }
        job.resume_delivery_pending=false;
        job.is_send_resume=delivered;
        job.updated_at=chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        job.resume_sent_at=delivered.then(||job.updated_at.clone());
        Ok((job.clone(),true))
    })?;
    super::notify_job_data_changed();
    Ok(job)
}

/// Historical records retain stable platform IDs. Restore their canonical JD URL;
/// captured URLs always win, including their original query parameters.
fn fill_source_url(job: &mut JobDetail) {
    if job.source_url.as_deref().is_some_and(|url| !url.trim().is_empty()) { return; }
    let (platform, id) = job.id.split_once(':').unwrap_or((job.platform.as_str(), &job.id));
    let platform = if platform.is_empty() { "boss" } else { platform };
    let id = if platform == "liepin" { id.strip_suffix(".shtml").unwrap_or(id) } else { id };
    job.source_url = match platform {
        "boss" if (16..=100).contains(&id.len()) && id.bytes().all(|ch| ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-')
            && !id.starts_with("manual-") => Some(format!("https://www.zhipin.com/job_detail/{id}.html")),
        "liepin" if (6..=20).contains(&id.len()) && id.bytes().all(|ch| ch.is_ascii_digit()) =>
            Some(format!("https://www.liepin.com/job/{id}.shtml")),
        "51job" if (6..=20).contains(&id.len()) && id.bytes().all(|ch| ch.is_ascii_digit()) =>
            Some(format!("https://we.51job.com/pc/jobdetail?jobId={id}")),
        _ => None,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    fn job(id: &str, platform: &str) -> JobDetail {
        serde_json::from_value(serde_json::json!({"id":id,"platform":platform,"title":"岗位","company_name":"公司",
            "detail":"职责","salary":"","location":null,"is_reply":false,"is_send_resume":true,
            "created_at":"now","resume_sent_at":"now","updated_at":"now"})).unwrap()
    }
    #[test]
    fn restores_canonical_urls_for_stable_legacy_ids_but_never_overwrites_captured_urls() {
        let mut boss=job("a5de0e2bc67cb6a90nFz3925FlpZ", "boss");
        fill_source_url(&mut boss);
        assert_eq!(boss.source_url.as_deref(),Some("https://www.zhipin.com/job_detail/a5de0e2bc67cb6a90nFz3925FlpZ.html"));
        let mut liepin=job("liepin:123456789", "liepin");
        fill_source_url(&mut liepin);
        assert_eq!(liepin.source_url.as_deref(),Some("https://www.liepin.com/job/123456789.shtml"));
        let mut legacy_suffix=job("liepin:1970857929.shtml", "liepin");
        fill_source_url(&mut legacy_suffix);
        assert_eq!(legacy_suffix.source_url.as_deref(),Some("https://www.liepin.com/job/1970857929.shtml"));
        liepin.source_url=Some("https://www.liepin.com/job/123456789.shtml?original=1".into());
        fill_source_url(&mut liepin);
        assert!(liepin.source_url.unwrap().ends_with("?original=1"));
        let mut manual=job("manual-unknown-position", "boss");
        fill_source_url(&mut manual);
        assert!(manual.source_url.is_none());
    }
}
