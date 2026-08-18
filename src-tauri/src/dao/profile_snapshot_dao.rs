use std::{path::Path, sync::OnceLock};

use crate::dao::store::BatchResult;
use anyhow::Result;

use crate::{
    config::{resolve_job_profile, AppRuntimeConfig},
    dao::{
        model::{JobDetail, JobProfileSnapshot},
        store::JsonStore,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionSource {
    Snapshot,
    CurrentProfile,
    DefaultProfile,
}

static STORE: OnceLock<JsonStore<JobProfileSnapshot>> = OnceLock::new();

pub fn init(data_dir: &Path) -> Result<()> {
    STORE
        .set(JsonStore::new(data_dir, "job_profile_snapshots.json")?)
        .map_err(|_| anyhow::anyhow!("JobProfileSnapshotDao 已经初始化"))
}

fn store() -> &'static JsonStore<JobProfileSnapshot> {
    STORE.get().expect("JobProfileSnapshotDao 未初始化")
}

pub fn get_by_id(snapshot_id: &str) -> Result<Option<JobProfileSnapshot>> {
    store().get_by_id(snapshot_id)
}

pub fn list() -> Result<Vec<JobProfileSnapshot>> {
    store().load_all()
}

pub fn replace_all(items: Vec<JobProfileSnapshot>) -> Result<()> {
    store().replace_all(items)
}

pub fn batch_upsert(items: Vec<JobProfileSnapshot>) -> Result<BatchResult> {
    store().batch_upsert(items, |existing, incoming| existing != incoming)
}

/// 内容哈希相同的快照只保存一次。
pub fn upsert(snapshot: JobProfileSnapshot) -> Result<()> {
    store().batch_upsert(vec![snapshot], |existing, incoming| existing != incoming)?;
    Ok(())
}

/// 恢复历史会话首次建联时的策略。快照是首选；升级前的数据或已损坏的引用才回退。
pub fn resolve_for_job(
    base: &AppRuntimeConfig,
    job: Option<&JobDetail>,
) -> Result<(AppRuntimeConfig, ResolutionSource), String> {
    let snapshot = match job.and_then(|job| job.profile_snapshot_id.as_deref()) {
        Some(snapshot_id) => get_by_id(snapshot_id).map_err(|error| error.to_string())?,
        None => None,
    };
    resolve_from_sources(base, job, snapshot.as_ref())
}

fn resolve_from_sources(
    base: &AppRuntimeConfig,
    job: Option<&JobDetail>,
    snapshot: Option<&JobProfileSnapshot>,
) -> Result<(AppRuntimeConfig, ResolutionSource), String> {
    if let Some(snapshot) = snapshot {
        return Ok((snapshot.apply_to(base), ResolutionSource::Snapshot));
    }
    if let Some(profile_id) = job.and_then(|job| job.profile_id.as_deref()) {
        // 历史会话允许继续使用后来已归档的同名方案；归档只阻止创建新投递任务。
        if let Some(profile) = base
            .job_profiles
            .iter()
            .find(|profile| profile.id == profile_id)
        {
            let mut config = base.clone();
            config.job_filter_config = profile.job_filter_config.clone();
            config.platform_filter_config = profile.platform_filter_config.clone();
            config.resume_config = profile.resume_config.clone();
            config.greet_config = profile.greet_config.clone();
            config.replay_config = profile.replay_config.clone();
            config.analysis_config = profile.analysis_config.clone();
            config.active_job_profile = None;
            return Ok((config, ResolutionSource::CurrentProfile));
        }
    }

    resolve_job_profile(base, None)
        .map(|resolved| (resolved.config, ResolutionSource::DefaultProfile))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_app_config, resolve_job_profile};

    fn legacy_job(profile_id: Option<&str>) -> JobDetail {
        JobDetail {
            id: "job".into(),
            platform: "boss".into(),
            source_task_id: None,
            profile_id: profile_id.map(str::to_string),
            profile_name: None,
            profile_snapshot_id: None,
            title: String::new(),
            company_name: String::new(),
            detail: String::new(),
            salary: String::new(),
            location: None,
            is_reply: false,
            is_send_resume: false,
            created_at: String::new(),
            resume_sent_at: None,
            updated_at: String::new(),
        }
    }

    #[test]
    fn persisted_snapshot_has_priority_over_current_profile() {
        let mut base = default_app_config();
        let resolved = resolve_job_profile(&base, None).unwrap();
        let snapshot = JobProfileSnapshot::from_resolved(&resolved.config).unwrap();
        let old_query = snapshot.job_filter_config.query.clone();
        base.job_profiles[0].job_filter_config.query = Some("后来编辑的方向".into());

        let (config, source) = resolve_from_sources(
            &base,
            Some(&legacy_job(Some(&snapshot.profile_id))),
            Some(&snapshot),
        )
        .unwrap();

        assert_eq!(source, ResolutionSource::Snapshot);
        assert_eq!(config.job_filter_config.query, old_query);
    }

    #[test]
    fn old_job_without_profile_falls_back_to_default() {
        let base = default_app_config();
        let (config, source) = resolve_from_sources(&base, Some(&legacy_job(None)), None).unwrap();

        assert_eq!(source, ResolutionSource::DefaultProfile);
        assert_eq!(
            config
                .active_job_profile
                .as_ref()
                .map(|profile| profile.id.as_str()),
            Some(base.default_job_profile_id.as_str())
        );
    }
}
