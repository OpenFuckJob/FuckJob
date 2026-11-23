use crate::dao::store::{Identifiable, JsonStore};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path, sync::OnceLock};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProcessedJobId {
    pub id: String,
}

impl Identifiable for ProcessedJobId {
    fn id(&self) -> &str {
        &self.id
    }
}

static STORE: OnceLock<JsonStore<ProcessedJobId>> = OnceLock::new();

pub fn init(data_dir: &Path) -> Result<()> {
    let store = JsonStore::<ProcessedJobId>::new(data_dir, "processed_job_ids.json")?;
    STORE
        .set(store)
        .map_err(|_| anyhow::anyhow!("ProcessedJobDao 已经初始化"))?;
    Ok(())
}

fn store() -> &'static JsonStore<ProcessedJobId> {
    STORE.get().expect("ProcessedJobDao 未初始化")
}

pub fn ids() -> Result<HashSet<String>> {
    Ok(store().load_all()?.into_iter().map(|job| job.id).collect())
}

pub fn record(job_id: &str) -> Result<()> {
    store().transaction(|ids| {
        if ids.iter().any(|job| job.id == job_id) {
            return Ok(((), false));
        }
        ids.push(ProcessedJobId {
            id: job_id.to_string(),
        });
        Ok(((), true))
    })
}
