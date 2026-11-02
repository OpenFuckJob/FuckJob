//! 猎聘会话状态与文本发送；附件发送由 resume_delivery 统一处理。
use anyhow::Context;
use rust_drission::Page;
use crate::{config::{ReplayResourceType, ReplyResource}, rpa::{
    conversation::{ConversationActions, ResumeState},
    liepin::handler::position_say_hello::send_resources,
}};
pub struct LiepinActions;

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
    /// 识别请求卡片和聊天工具栏；实际附件选择与提交由统一发送流程执行。
    fn resume_state(&self, page: &Page) -> Result<ResumeState, anyhow::Error> {
        if find_pending_resume_request(page, false)? {
            return Ok(ResumeState::RequestedByPeer);
        }
        Ok(if mark_resume_button(page)? {
            ResumeState::Sendable
        } else {
            ResumeState::Unavailable
        })
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

}
fn mark_resume_button(page: &Page) -> Result<bool, anyhow::Error> {
    Ok(crate::rpa::resume_delivery::ui(page, "markResumeUi(document, 'liepin', 'open', null)")?.as_bool().unwrap_or(false))
}
