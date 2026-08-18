//! 自动回复过程中两个平台共用的带副作用动作。
//!
//! 和 [`crate::rpa::conversation`] 分开放，是因为那边刻意保持纯判断——它的护栏
//! 逻辑全靠单测兜着，掺进读写库就得先搭一套数据目录才跑得起来。这边则相反，
//! 每个函数都在写库或等时钟，但它们同样是平台无关的：BOSS 和猎聘挂起会话的
//! 字段口径、记流水的时机、消项的判据必须一致，否则待办列表里两个平台的
//! 条目长得不一样，节流额度也会各算各的。

use std::time::Duration;

use crate::config::ReplyPollingConfig;
use crate::dao::model::{AutoReplyAction, ManualReviewReason};
use crate::dao::{auto_reply_log_dao, manual_review_dao};
use crate::logger;
use crate::rpa::common::ChatMessage;
use crate::rpa::conversation::{ConversationContext, ReviewDraft};
use crate::rpa::polling;
use crate::rpa::run_flow::is_job_task_stop_requested;

/// 记一次自动发送。
///
/// 写失败只警告：消息已经发出去了，这时候往上抛错只会让调用方以为没发成功，
/// 进而在下一轮重复发送
pub fn record_auto_send(
    platform: &str,
    conversation_id: &str,
    job_id: &str,
    action: AutoReplyAction,
    chars: usize,
) {
    if let Err(error) =
        auto_reply_log_dao::record(platform, conversation_id, job_id, action, chars)
    {
        let _ = logger::warning(format!("记录自动发送流水失败: {error}"));
    }
}

/// 把会话挂进待人工处理列表。
///
/// DAO 出错不阻断会话处理：挂不进去最多少一条提醒，而抛出去会让本轮剩下的
/// 会话全部处理不到
pub fn hold_for_review(
    platform: &str,
    conversation_id: &str,
    draft: ReviewDraft,
    reason: ManualReviewReason,
    detail: String,
) {
    let request = draft.to_request(platform, conversation_id, reason, detail);
    if let Err(error) = manual_review_dao::upsert(request) {
        let _ = logger::warning(format!("写入待人工处理列表失败: {error}"));
    }
}

/// 用户已经在平台里自己回过了就把待办消掉，不必等他回应用里手工点一次。
///
/// 判据是「最近一次挂起之后有一条己方消息，且不是本工具发的」——本工具发的
/// 那些在自动发送流水里都有记录
pub fn release_if_handled(platform: &str, conversation_id: &str, messages: &[ChatMessage]) {
    let Some(held) = manual_review_dao::get(platform, conversation_id)
        .ok()
        .flatten()
    else {
        return;
    };

    let last_own = messages
        .iter()
        .filter(|message| !message.received)
        .map(|message| message.time)
        .max();
    let last_auto = auto_reply_log_dao::last_sent_at(platform, conversation_id)
        .ok()
        .flatten();

    if manual_review_dao::should_auto_resolve(held.updated_at, last_own, last_auto)
        && manual_review_dao::resolve(platform, conversation_id).unwrap_or(false)
    {
        let _ = logger::info("检测到你已手工回复，该会话已从待人工处理列表移除");
    }
}

/// 发送之前补足「像人在打字」的那段间隔。返回 false 表示等待途中任务被停止。
///
/// 秒回比慢回更像机器人：HR 不指望你三秒回，但会注意到你每次都三秒回。
/// 真正要凑的是「对方发出到我方回复」的总间隔，而轮询周期本身通常已经把它
/// 填满了，所以绝大多数情况下这里一秒都不等
pub async fn wait_before_reply(
    polling_config: &ReplyPollingConfig,
    context: &ConversationContext,
) -> bool {
    let elapsed_ms = context
        .last_received()
        .map(|message| chrono::Local::now().timestamp_millis() - message.time);
    let seconds = polling::humanize_delay_seconds_now(polling_config, elapsed_ms);
    if seconds == 0 {
        return true;
    }

    let _ = logger::info(format!("等待 {seconds} 秒后回复，避免秒回"));
    // 最长可能等两分钟，用户点了停止不该干等着
    for _ in 0..seconds {
        if is_job_task_stop_requested() {
            return false;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    true
}
