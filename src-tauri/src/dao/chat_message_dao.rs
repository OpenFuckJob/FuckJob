use crate::dao::model::ChatMessageRecord;
use crate::dao::store::JsonStore;
use crate::rpa::boss::model::ChatMessage;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
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

pub fn list() -> Result<Vec<ChatMessageRecord>> {
    store().load_all()
}

pub fn create(record: ChatMessageRecord) -> Result<()> {
    store().insert(record)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ChatMessageSaveResult {
    pub inserted: usize,
    pub updated: usize,
}

/// 按 job_id + mid 增量写入；已存在的消息内容发生变化时同步更新。
pub fn upsert_incremental(job_id: &str, messages: &[ChatMessage]) -> Result<ChatMessageSaveResult> {
    let mut all = store().load_all()?;
    let mut indexes: HashMap<i64, usize> = all
        .iter()
        .enumerate()
        .filter(|(_, record)| record.job_id == job_id)
        .map(|(index, record)| (record.mid, index))
        .collect();
    let mut result = ChatMessageSaveResult::default();

    for msg in messages {
        let record = ChatMessageRecord {
            id: format!("{}:{}", job_id, msg.mid),
            job_id: job_id.to_string(),
            mid: msg.mid,
            received: msg.received,
            text: msg.text.clone(),
            time: msg.time,
            from_name: msg.from_name.clone(),
        };

        if let Some(index) = indexes.get(&msg.mid).copied() {
            let existing = &all[index];
            if existing.received != record.received
                || existing.text != record.text
                || existing.time != record.time
                || existing.from_name != record.from_name
            {
                all[index] = record;
                result.updated += 1;
            }
        } else {
            indexes.insert(msg.mid, all.len());
            all.push(record);
            result.inserted += 1;
        }
    }

    if result.inserted > 0 || result.updated > 0 {
        store().replace_all(all)?;
    }
    Ok(result)
}

/// 兼容既有调用方，只返回新增条数。
pub fn save_incremental(job_id: &str, messages: &[ChatMessage]) -> Result<usize> {
    Ok(upsert_incremental(job_id, messages)?.inserted)
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

pub fn batch_insert_new(items: Vec<ChatMessageRecord>) -> Result<usize> {
    store().batch_insert_new(items)
}

pub fn replace_all(items: Vec<ChatMessageRecord>) -> Result<()> {
    store().replace_all(items)
}
