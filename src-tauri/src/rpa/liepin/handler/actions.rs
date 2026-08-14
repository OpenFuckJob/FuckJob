//! 猎聘侧的会话动作实现。
//!
//! 这里只做「读页面状态」和「点按钮」两件事，任何判断都不放进来 ——
//! 判断全在平台无关的 [`crate::rpa::conversation`] 里，两个平台的行为才不会再次分叉。

use anyhow::Context;
use rust_drission::{utils::sleep_random_ms, Page};

use crate::{
    config::{ReplayResourceType, ReplyResource},
    rpa::{
        conversation::{ConversationActions, ResumeState},
        liepin::handler::position_say_hello::send_resources,
    },
};

pub struct LiepinActions;

/// 找出待处理的简历索要卡片，返回是否找到。
///
/// HR 主动索要简历时会推一张卡片（payload 里 `extType` = 206），带【拒绝】【同意】两个按钮。
///
/// 只认文案里带"简历"的那张卡：优先沟通、职位卡片长得一样，光靠按钮会误点。
/// 已经处理过的卡片不再有可点的【同意】，所以按钮还在就说明这次是待处理的。
///
/// `mark` 为真时顺手给按钮打上标记，交给外层用真实点击触发；只是探状态时不打标记，
/// 免得在页面上留下没用过的属性。
fn find_pending_resume_request(page: &Page, mark: bool) -> Result<bool, anyhow::Error> {
    let script = format!(
        r#"
        (() => {{
            const mark = {mark};
            const cards = Array.from(document.querySelectorAll(".im-ui-common-message-card"));
            // 同一个会话可能来过多次，从最新的一张往回找
            for (const card of cards.reverse()) {{
                if (!(card.innerText || "").includes("简历")) continue;
                const button = card.querySelector("button[btntext='同意']");
                if (!button || button.disabled) continue;
                if (mark) button.setAttribute("data-fj-agree", "1");
                return true;
            }}
            return false;
        }})()
        "#
    );

    let value = page
        .run_js_await(&script)
        .context("查找猎聘简历请求卡片失败")?;
    let raw = value.get("value").cloned().unwrap_or(value);

    Ok(raw.as_bool().unwrap_or(false))
}

impl ConversationActions for LiepinActions {
    /// 猎聘只有「对方索要、我方同意」这一条会话内的简历通路。
    ///
    /// 候选人端的主动投递发生在职位详情页的申请动作里，聊天窗的工具栏根本没有这个入口，
    /// 所以没有待处理卡片时只能是 [`ResumeState::Unavailable`]。这里不能图省事返回
    /// `Sendable`：那会让模型以为可以投，`reconcile` 放行之后执行期才发现无处可点。
    fn resume_state(&self, page: &Page) -> Result<ResumeState, anyhow::Error> {
        if find_pending_resume_request(page, false)? {
            return Ok(ResumeState::RequestedByPeer);
        }

        Ok(ResumeState::Unavailable)
    }

    fn send_text(&self, page: &Page, text: &str) -> Result<bool, anyhow::Error> {
        if text.trim().is_empty() {
            return Ok(false);
        }

        // 复用打招呼那条已经调稳的发送链路（等输入框、等按钮可用、等输入框清空），
        // 另写一套只会让两处的成功判定标准慢慢分家
        send_resources(
            page,
            vec![ReplyResource {
                resource_type: ReplayResourceType::Text,
                content: text.to_string(),
            }],
        )?;

        Ok(true)
    }

    /// 猎聘候选人端没有主动投递简历的入口，所以这里恒为 false。
    ///
    /// 返回 `Ok(false)` 而不是报错：入口不存在是平台的既有形态，不是运行故障，
    /// 上层据此只回消息即可。真正的简历传递靠 [`Self::accept_resume_request`]。
    fn send_resume(&self, _page: &Page) -> Result<bool, anyhow::Error> {
        Ok(false)
    }

    /// 点完就发出去了，没有二次确认 —— 实测过。不要照 Boss 那边补 `.panel-resume` 的等待，
    /// 猎聘这里没有那个弹窗，等只会白等到超时。
    fn accept_resume_request(&self, page: &Page) -> Result<bool, anyhow::Error> {
        if !find_pending_resume_request(page, true)? {
            return Ok(false);
        }

        // 标记出来再用真实点击，比在页面里直接 click 更接近人的操作
        page.click("button[data-fj-agree='1']")?;
        sleep_random_ms(800, 1200);

        Ok(true)
    }
}
