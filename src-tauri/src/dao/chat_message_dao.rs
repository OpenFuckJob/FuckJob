use crate::dao::model::ChatMessageRecord;
use crate::dao::store::JsonStore;
use crate::rpa::common::ChatMessage;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

static STORE: OnceLock<JsonStore<ChatMessageRecord>> = OnceLock::new();

/// 旧数据的默认平台。猎聘此前从未落过库，存量记录必然来自 BOSS
const LEGACY_PLATFORM: &str = "boss";

pub fn init(data_dir: &Path) -> Result<()> {
    let store = JsonStore::new(data_dir, "chat_messages.json")?;
    STORE
        .set(store)
        .map_err(|_| anyhow::anyhow!("ChatMessageDao 已经初始化"))?;
    migrate_legacy_records()?;
    Ok(())
}

fn store() -> &'static JsonStore<ChatMessageRecord> {
    STORE.get().expect("ChatMessageDao 未初始化")
}

/// 把 `{job_id}:{mid}` 时代的记录补齐平台与会话标识。
///
/// 放在启动时一次做完，而不是每次查询时兜底：兜底逻辑会长期留在读路径上，
/// 且新旧两种主键并存时，增量写入会把同一条消息重复插入。
fn migrate_legacy_records() -> Result<()> {
    let migrated = store().transaction(|all| {
        let mut migrated = 0usize;
        for record in all.iter_mut() {
            if !record.platform.is_empty() && !record.conversation_id.is_empty() {
                continue;
            }
            *record = normalize(record.clone());
            migrated += 1;
        }
        Ok((migrated, migrated > 0))
    })?;

    if migrated > 0 {
        let _ = crate::logger::info(format!("聊天记录已迁移到会话主键，共 {migrated} 条"));
    }
    Ok(())
}

pub fn find_by_job_id(job_id: &str) -> Result<Vec<ChatMessageRecord>> {
    let job_id = job_id.to_string();
    store().query(|m| m.job_id == job_id)
}

/// 按会话取历史，按时间升序返回。
///
/// 自动回复要用的是这个而不是 [`find_by_job_id`]：猎聘会话可能没有岗位归属，
/// 而没有归属不代表没有历史。
pub fn find_by_conversation(platform: &str, conversation_id: &str) -> Result<Vec<ChatMessage>> {
    let platform = platform.to_string();
    let conversation_id = conversation_id.to_string();
    let mut records =
        store().query(|m| m.platform == platform && m.conversation_id == conversation_id)?;
    records.sort_by_key(|record| (record.time, record.mid));

    Ok(records
        .into_iter()
        .map(|record| ChatMessage {
            mid: record.mid,
            received: record.received,
            text: record.text,
            time: record.time,
            from_name: record.from_name,
        })
        .collect())
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

/// 会话归属信息。`job_id` 允许为空——猎聘映射不到岗位时仍要存下消息
#[derive(Debug, Clone, Copy)]
pub struct ConversationKey<'a> {
    pub platform: &'a str,
    pub conversation_id: &'a str,
    pub job_id: &'a str,
}

/// 按会话 + mid 增量写入；已存在的消息内容发生变化时同步更新。
///
/// 岗位归属是后补的：猎聘先按会话存消息，之后某轮识别出岗位时再回填 `job_id`，
/// 所以这里对已存在记录的空 `job_id` 也做更新。
pub fn upsert_incremental(
    key: ConversationKey<'_>,
    messages: &[ChatMessage],
) -> Result<ChatMessageSaveResult> {
    store().transaction(|all| {
        let mut indexes: HashMap<i64, usize> = all
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record.platform == key.platform && record.conversation_id == key.conversation_id
            })
            .map(|(index, record)| (record.mid, index))
            .collect();
        let mut result = ChatMessageSaveResult::default();

        for msg in messages {
            let record = ChatMessageRecord {
                id: ChatMessageRecord::build_id(key.platform, key.conversation_id, msg.mid),
                job_id: key.job_id.to_string(),
                platform: key.platform.to_string(),
                conversation_id: key.conversation_id.to_string(),
                mid: msg.mid,
                received: msg.received,
                text: msg.text.clone(),
                time: msg.time,
                from_name: msg.from_name.clone(),
            };

            if let Some(index) = indexes.get(&msg.mid).copied() {
                let existing = &all[index];
                // 岗位归属只补不删：这一轮没识别出岗位，不该把上一轮识别到的抹掉
                let job_id_changed = !key.job_id.is_empty() && existing.job_id != key.job_id;
                if existing.received != record.received
                    || existing.text != record.text
                    || existing.time != record.time
                    || existing.from_name != record.from_name
                    || job_id_changed
                {
                    let job_id = if key.job_id.is_empty() {
                        existing.job_id.clone()
                    } else {
                        record.job_id.clone()
                    };
                    all[index] = ChatMessageRecord { job_id, ..record };
                    result.updated += 1;
                }
            } else {
                indexes.insert(msg.mid, all.len());
                all.push(record);
                result.inserted += 1;
            }
        }

        let changed = result.inserted > 0 || result.updated > 0;
        Ok((result, changed))
    })
}

pub fn delete_by_job_id(job_id: &str) -> Result<bool> {
    store().transaction(|all| {
        let original_len = all.len();
        all.retain(|message| message.job_id != job_id);
        let changed = all.len() < original_len;
        Ok((changed, changed))
    })
}

pub fn batch_insert_new(items: Vec<ChatMessageRecord>) -> Result<usize> {
    store().batch_insert_new(normalize_all(items))
}

pub fn replace_all(items: Vec<ChatMessageRecord>) -> Result<()> {
    store().replace_all(normalize_all(items))
}

/// 备份导入走的是这里而不是启动迁移，所以旧格式备份必须在入库前补齐，
/// 否则导入的记录会以空平台存进来，之后既查不到也会被增量写入重复插入。
fn normalize_all(items: Vec<ChatMessageRecord>) -> Vec<ChatMessageRecord> {
    items.into_iter().map(normalize).collect()
}

fn normalize(mut record: ChatMessageRecord) -> ChatMessageRecord {
    if record.platform.is_empty() {
        record.platform = LEGACY_PLATFORM.to_string();
    }
    if record.conversation_id.is_empty() {
        record.conversation_id = record.job_id.clone();
    }
    record.id = ChatMessageRecord::build_id(&record.platform, &record.conversation_id, record.mid);
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy(job_id: &str, mid: i64) -> ChatMessageRecord {
        ChatMessageRecord {
            id: format!("{job_id}:{mid}"),
            job_id: job_id.to_string(),
            platform: String::new(),
            conversation_id: String::new(),
            mid,
            received: true,
            text: "在吗".to_string(),
            time: 1,
            from_name: String::new(),
        }
    }

    #[test]
    fn legacy_records_are_backfilled_as_boss_conversations() {
        let record = normalize(legacy("encrypted-job-1", 7));

        assert_eq!(record.platform, "boss");
        assert_eq!(record.conversation_id, "encrypted-job-1");
        assert_eq!(record.id, "boss:encrypted-job-1:7");
    }

    /// 猎聘会话没有岗位归属，会话标识不能从 job_id 兜底出来
    #[test]
    fn normalizing_keeps_an_explicit_conversation_id() {
        let mut record = legacy("", 9);
        record.platform = "liepin".to_string();
        record.conversation_id = "imid-abc".to_string();

        let record = normalize(record);

        assert_eq!(record.id, "liepin:imid-abc:9");
        assert!(record.job_id.is_empty());
    }

    /// 同一条消息在两种主键格式下各存一份，是迁移必须在写入前完成的原因
    #[test]
    fn migration_makes_the_key_stable_across_formats() {
        let first = normalize(legacy("job-1", 3));
        let mut already_migrated = legacy("job-1", 3);
        already_migrated.platform = "boss".to_string();
        already_migrated.conversation_id = "job-1".to_string();

        assert_eq!(first.id, normalize(already_migrated).id);
    }
}
