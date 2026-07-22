use crate::dao::model::ChatMessageRecord;
use crate::dao::store::JsonStore;
use crate::rpa::boss::model::ChatMessage;
use anyhow::Result;
use std::path::Path;
use std::sync::OnceLock;

static STORE: OnceLock<JsonStore<ChatMessageRecord>> = OnceLock::new();

pub fn init(data_dir: &Path) -> Result<()> {
    let store = JsonStore::new(data_dir, "chat_messages.json")?;
    STORE
        .set(store)
        .map_err(|_| anyhow::anyhow!("ChatMessageDao 已经初始化"))?;
    Ok(())
}

fn store() -> &'static JsonStore<ChatMessageRecord> {
    STORE.get().expect("ChatMessageDao 未初始化")
}

pub fn find_by_job_id(job_id: &str) -> Result<Vec<ChatMessageRecord>> {
    let job_id = job_id.to_string();
    store().query(|m| m.job_id == job_id)
}

/// 增量保存：按 mid 去重，只插入新消息
pub fn save_incremental(job_id: &str, messages: &[ChatMessage]) -> Result<usize> {
    let existing = find_by_job_id(job_id)?;
    let existing_mids: std::collections::HashSet<i64> = existing.iter().map(|m| m.mid).collect();

    let mut inserted = 0;
    for msg in messages {
        if existing_mids.contains(&msg.mid) {
            continue;
        }
        let record = ChatMessageRecord {
            id: format!("{}:{}", job_id, msg.mid),
            job_id: job_id.to_string(),
            mid: msg.mid,
            received: msg.received,
            text: msg.text.clone(),
            time: msg.time,
            from_name: msg.from_name.clone(),
        };
        store().insert(record)?;
        inserted += 1;
    }
    Ok(inserted)
}

pub fn delete_by_job_id(job_id: &str) -> Result<bool> {
    let existing = find_by_job_id(job_id)?;
    if existing.is_empty() {
        return Ok(false);
    }
    let all = store().load_all()?;
    let remaining: Vec<ChatMessageRecord> =
        all.into_iter().filter(|m| m.job_id != job_id).collect();
    store().replace_all(remaining)?;
    Ok(true)
}
