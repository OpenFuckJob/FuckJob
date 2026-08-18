//! 待人工处理的会话。
//!
//! 轮询模式下用户不会盯着日志看，而「消息被读掉了却一个字都没回出去」是会
//! 静默吃掉真实机会的。这张表就是那些会话的唯一出口。

use std::path::Path;
use std::sync::OnceLock;

use anyhow::Result;

use crate::dao::model::{ManualReviewReason, ManualReviewRecord};
use crate::dao::store::JsonStore;

static STORE: OnceLock<JsonStore<ManualReviewRecord>> = OnceLock::new();

pub fn init(data_dir: &Path) -> Result<()> {
    let store = JsonStore::new(data_dir, "manual_review.json")?;
    STORE
        .set(store)
        .map_err(|_| anyhow::anyhow!("ManualReviewDao 已经初始化"))?;
    Ok(())
}

fn store() -> &'static JsonStore<ManualReviewRecord> {
    STORE.get().expect("ManualReviewDao 未初始化")
}

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

/// 一次挂起请求。字段都是调用方现场就有的，不需要它再去查库拼装
#[derive(Debug, Clone)]
pub struct ReviewRequest<'a> {
    pub platform: &'a str,
    pub conversation_id: &'a str,
    pub job_id: &'a str,
    pub job_name: &'a str,
    pub company_name: &'a str,
    pub reason: ManualReviewReason,
    pub detail: String,
    pub last_message: String,
}

/// 挂起一个会话，或在它已经挂起时刷新。
///
/// 同一会话重复触发只累加计数并更新原因，不新增记录：HR 连发三条敏感消息
/// 是列表里的一行，不是三行。原因取最新一次——用户要看的是「现在卡在哪」。
pub fn upsert(request: ReviewRequest<'_>) -> Result<()> {
    let id = ManualReviewRecord::build_id(request.platform, request.conversation_id);
    let now = now_ms();

    store().transaction(|all| {
        match all.iter_mut().find(|record| record.id == id) {
            Some(existing) => {
                existing.reason = request.reason;
                existing.detail = request.detail;
                existing.last_message = request.last_message;
                existing.updated_at = now;
                existing.hit_count += 1;
                // 岗位归属只补不删：这一轮没识别出岗位，不该把上一轮识别到的抹掉
                if !request.job_id.is_empty() {
                    existing.job_id = request.job_id.to_string();
                }
                if !request.job_name.is_empty() {
                    existing.job_name = request.job_name.to_string();
                }
                if !request.company_name.is_empty() {
                    existing.company_name = request.company_name.to_string();
                }
            }
            None => all.push(ManualReviewRecord {
                id: id.clone(),
                platform: request.platform.to_string(),
                conversation_id: request.conversation_id.to_string(),
                job_id: request.job_id.to_string(),
                job_name: request.job_name.to_string(),
                company_name: request.company_name.to_string(),
                reason: request.reason,
                detail: request.detail,
                last_message: request.last_message,
                created_at: now,
                updated_at: now,
                hit_count: 1,
            }),
        }
        Ok(((), true))
    })
}

/// 按最近触发时间倒序返回。最新卡住的排在最前面
pub fn list() -> Result<Vec<ManualReviewRecord>> {
    let mut records = store().load_all()?;
    records.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(records)
}

pub fn count() -> Result<usize> {
    store().count()
}

pub fn resolve(platform: &str, conversation_id: &str) -> Result<bool> {
    store().delete_by_id(&ManualReviewRecord::build_id(platform, conversation_id))
}

pub fn clear() -> Result<()> {
    store().replace_all(Vec::new())
}

/// 检查某个会话是否该自动消项。
///
/// 判据是「你在挂起之后手工发过消息」：聊天记录里有一条自己发的消息，
/// 时间晚于挂起时刻，且不是 AI 发的。AI 发的那些在自动发送流水里有据可查，
/// 所以这里只要 `last_auto_sent_at` 就够——它之后的己方消息必然出自人手。
///
/// 纯函数，时间戳全部由调用方给出：这条判据一旦算错，要么把用户还没处理的
/// 会话悄悄消掉，要么让处理过的会话永远赖在列表里，两种都得能单测。
pub fn should_auto_resolve(
    created_at: i64,
    last_own_message_at: Option<i64>,
    last_auto_sent_at: Option<i64>,
) -> bool {
    let Some(own) = last_own_message_at else {
        return false;
    };
    if own <= created_at {
        return false;
    }
    match last_auto_sent_at {
        Some(auto) => own > auto,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_message_after_hold_resolves_the_item() {
        assert!(should_auto_resolve(100, Some(200), None));
        assert!(should_auto_resolve(100, Some(300), Some(200)));
    }

    /// 挂起之前发的消息不算处理，否则每个有过打招呼的会话一挂起就自动消失
    #[test]
    fn messages_predating_the_hold_do_not_resolve_it() {
        assert!(!should_auto_resolve(100, Some(50), None));
        assert!(!should_auto_resolve(100, Some(100), None));
    }

    /// 己方最后一条消息就是 AI 自己发的，不能当成用户已接手
    #[test]
    fn the_agents_own_message_does_not_resolve_the_item() {
        assert!(!should_auto_resolve(100, Some(200), Some(200)));
        assert!(!should_auto_resolve(100, Some(200), Some(300)));
    }

    #[test]
    fn a_conversation_without_own_messages_stays_held() {
        assert!(!should_auto_resolve(100, None, None));
    }
}
