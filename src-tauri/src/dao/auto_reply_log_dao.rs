//! 自动发送流水。时间窗节流和「你是否已接手」的判定都读这里。

use std::path::Path;
use std::sync::OnceLock;

use anyhow::Result;

use crate::dao::model::{AutoReplyAction, AutoReplyLogRecord};
use crate::dao::store::JsonStore;

static STORE: OnceLock<JsonStore<AutoReplyLogRecord>> = OnceLock::new();

/// 超过这个天数的流水在启动时清掉。
///
/// 节流只看最近 24 小时上下，「你是否已接手」只看待办创建之后，
/// 两者都用不到更早的记录，留着只是让每次查询多扫一遍。
const RETENTION_DAYS: i64 = 30;

pub fn init(data_dir: &Path) -> Result<()> {
    let store = JsonStore::new(data_dir, "auto_reply_log.json")?;
    STORE
        .set(store)
        .map_err(|_| anyhow::anyhow!("AutoReplyLogDao 已经初始化"))?;
    prune(RETENTION_DAYS * 24 * 3600 * 1000)?;
    Ok(())
}

fn store() -> &'static JsonStore<AutoReplyLogRecord> {
    STORE.get().expect("AutoReplyLogDao 未初始化")
}

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

/// 记一次自动发送。
///
/// 主键含毫秒时间戳，同一会话连续发送会各记一条；同一毫秒内的重复发送
/// 会被主键去重，而那本就不是两次独立的发送。
pub fn record(
    platform: &str,
    conversation_id: &str,
    job_id: &str,
    action: AutoReplyAction,
    chars: usize,
) -> Result<()> {
    let sent_at = now_ms();
    let record = AutoReplyLogRecord {
        id: AutoReplyLogRecord::build_id(platform, conversation_id, sent_at),
        platform: platform.to_string(),
        conversation_id: conversation_id.to_string(),
        job_id: job_id.to_string(),
        action,
        sent_at,
        chars,
    };
    store().batch_upsert(vec![record], |_, _| false)?;
    Ok(())
}

/// 数指定会话在最近 `window_hours` 小时内的模型自动回复条数。
///
/// 只数 [`AutoReplyAction::Reply`]：模板话术是用户写死的固定应答，
/// 简历投递是既定策略，把它们算进额度会让真正的对话轮次被无谓挤掉。
pub fn count_replies_within(
    platform: &str,
    conversation_id: &str,
    window_hours: u64,
) -> Result<usize> {
    let since = window_start(now_ms(), window_hours);
    let platform = platform.to_string();
    let conversation_id = conversation_id.to_string();
    Ok(store()
        .query(move |record| {
            record.platform == platform
                && record.conversation_id == conversation_id
                && record.action == AutoReplyAction::Reply
                && record.sent_at >= since
        })?
        .len())
}

/// 指定会话最近一次自动发送的时刻。判断「你是否已手工接手」时用作分界点。
pub fn last_sent_at(platform: &str, conversation_id: &str) -> Result<Option<i64>> {
    let platform = platform.to_string();
    let conversation_id = conversation_id.to_string();
    Ok(store()
        .query(move |record| {
            record.platform == platform && record.conversation_id == conversation_id
        })?
        .into_iter()
        .map(|record| record.sent_at)
        .max())
}

pub fn list() -> Result<Vec<AutoReplyLogRecord>> {
    store().load_all()
}

pub fn delete_by_conversation(platform: &str, conversation_id: &str) -> Result<bool> {
    store().transaction(|all| {
        let original = all.len();
        all.retain(|record| {
            !(record.platform == platform && record.conversation_id == conversation_id)
        });
        let changed = all.len() < original;
        Ok((changed, changed))
    })
}

fn prune(max_age_ms: i64) -> Result<usize> {
    let cutoff = now_ms() - max_age_ms;
    store().transaction(|all| {
        let original = all.len();
        all.retain(|record| record.sent_at >= cutoff);
        let removed = original - all.len();
        Ok((removed, removed > 0))
    })
}

/// 滚动窗口的起点。抽出来是为了能在不碰时钟的前提下验证边界语义。
///
/// 转换用 `try_from` 而不是 `as`：`as` 对超出 i64 的窗口值会回绕成负数，
/// 把起点推到未来去，于是节流计数恒为 0——上限静默失效，谁也不会发现
fn window_start(now: i64, window_hours: u64) -> i64 {
    let span = i64::try_from(window_hours)
        .unwrap_or(i64::MAX)
        .saturating_mul(3600 * 1000);
    now.saturating_sub(span)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_start_is_exclusive_of_older_records() {
        let now = 1_000_000_000_000;
        assert_eq!(window_start(now, 24), now - 24 * 3600 * 1000);
    }

    /// 窗口为 0 时起点等于当下，等价于「不允许任何历史记录计入」，
    /// 而不是回绕成一个远古时间点把所有记录都算进来
    #[test]
    fn zero_window_does_not_wrap_into_the_past() {
        let now = 1_000_000_000_000;
        assert_eq!(window_start(now, 0), now);
    }

    /// 大到离谱的窗口值必须仍然落在过去。回绕成未来时刻会让计数恒为 0，
    /// 上限静默失效而没有任何报错
    #[test]
    fn absurd_window_stays_in_the_past_instead_of_wrapping() {
        let now = 1_000_000_000_000;
        assert!(window_start(now, u64::MAX) < now);
        assert!(window_start(now, u64::MAX) < 0);
    }
}
