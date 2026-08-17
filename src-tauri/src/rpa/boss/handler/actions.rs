//! BOSS 直聘的会话动作实现。
//!
//! 这里存在的意义是把「读状态」和「做动作」分开。改造前的做法是不看状态直接
//! `page.click(".toolbar-btn")` 点工具栏第一个按钮——而 `.toolbar-btn` 是
//! 「发简历 / 换电话 / 换微信」共用的类名，点中哪个取决于 DOM 顺序，
//! 等于随机把手机号或微信推给素未谋面的招聘方。

use rust_drission::Page;

use crate::rpa::boss::handler::send_message::send_text_message;
use crate::rpa::boss::handler::send_resume::send_resume;
use crate::rpa::conversation::{ConversationActions, ResumeState};

pub struct BossActions;

impl ConversationActions for BossActions {
    /// 读简历入口状态。
    ///
    /// 判定顺序是有讲究的：先看有没有待我确认的索要请求，再看工具栏能不能主动投递。
    /// 反过来的话，对方正在索要时会被识别成「可主动投递」，于是我们主动发一次、
    /// 对方的请求还挂着，等于同一份简历走了两条路。
    fn resume_state(&self, page: &Page) -> Result<ResumeState, anyhow::Error> {
        let value = page.run_js_await(RESUME_STATE_SCRIPT)?;
        let raw = value.get("value").cloned().unwrap_or(value);

        Ok(match raw.as_str().unwrap_or_default() {
            "requested" => ResumeState::RequestedByPeer,
            "sendable" => ResumeState::Sendable,
            "unavailable" => ResumeState::Unavailable,
            // 认不出来就认不出来。宁可这一轮不投递，也不要靠猜去点按钮
            _ => ResumeState::Unknown,
        })
    }

    fn send_text(&self, page: &Page, text: &str) -> Result<bool, anyhow::Error> {
        send_text_message(page, text)
    }

    fn send_resume(&self, page: &Page) -> Result<bool, anyhow::Error> {
        send_resume(page)
    }

    fn accept_resume_request(&self, page: &Page) -> Result<bool, anyhow::Error> {
        let value = page.run_js_await(MARK_ACCEPT_BUTTON_SCRIPT)?;
        let raw = value.get("value").cloned().unwrap_or(value);
        if !raw.as_bool().unwrap_or(false) {
            return Ok(false);
        }

        // 先在页面里打标记再用真实点击，比在 JS 里直接 click 更接近人的操作
        page.click("[data-fj-accept-resume='1']")?;
        rust_drission::utils::sleep_random_ms(600, 1000);

        // BOSS 同意后通常还有一层确认面板；没有就说明这次不需要，不必等满超时
        if page.ele(".panel-resume")?.is_some() {
            confirm_resume_panel(page)?;
        }
        Ok(true)
    }
}

/// 点掉简历确认面板上的「确定」。
///
/// 只认文案，不认位置：面板里同时有「取消」和「确定」，按下标取会随版本漂移。
pub(crate) fn confirm_resume_panel(page: &Page) -> Result<bool, anyhow::Error> {
    for candidate in page.elements(".panel-resume .btns span, .panel-resume .btns button")? {
        if candidate.text_content()?.trim() == "确定" {
            candidate.click()?;
            rust_drission::utils::sleep_random_ms(500, 800);
            return Ok(true);
        }
    }
    Ok(false)
}

/// 读简历入口状态。
///
/// 工具栏按钮靠**文案**定位而不是位置：`.toolbar-btn` 是同类按钮共用的类名，
/// 按下标取等于把「换微信」当成「发简历」。
///
/// 对方索要简历的卡片没有稳定类名，因此要求同时满足三条才认：
/// 按钮文案是「同意」「接受」，祖先节点提到「简历」，且不在工具栏里。
/// 宁可认不出来（这一轮不处理），也不要认错（点到不该点的按钮）。
const RESUME_STATE_SCRIPT: &str = r#"
(() => {
    const text = (el) => ((el && (el.innerText || el.textContent)) || "").trim();
    const disabled = (el) => {
        const className = el.getAttribute("class") || "";
        return Boolean(el.disabled)
            || el.getAttribute("aria-disabled") === "true"
            || className.split(/\s+/).includes("unable")
            || className.includes("disabled");
    };

    const acceptable = Array.from(
        document.querySelectorAll("button, span[role='button'], div[role='button'], a")
    ).find((el) => {
        const label = text(el);
        if (label !== "同意" && label !== "接受" && label !== "同意并发送") return false;
        if (disabled(el)) return false;
        if (el.closest(".toolbar, .toolbar-btn, .chat-op")) return false;
        // 往上找几层看有没有提到简历，避免把「同意查看联系方式」之类误认
        let scope = el;
        for (let depth = 0; depth < 4 && scope; depth += 1) {
            if (text(scope).includes("简历")) return true;
            scope = scope.parentElement;
        }
        return false;
    });
    if (acceptable) return "requested";

    const resumeButton = Array.from(document.querySelectorAll(".toolbar-btn"))
        .find((el) => text(el) === "发简历");
    if (!resumeButton) return "unknown";
    return disabled(resumeButton) ? "unavailable" : "sendable";
})()
"#;

/// 给待确认的「同意」按钮打标记，供后续真实点击定位
const MARK_ACCEPT_BUTTON_SCRIPT: &str = r#"
(() => {
    const text = (el) => ((el && (el.innerText || el.textContent)) || "").trim();
    const disabled = (el) => {
        const className = el.getAttribute("class") || "";
        return Boolean(el.disabled)
            || el.getAttribute("aria-disabled") === "true"
            || className.split(/\s+/).includes("unable")
            || className.includes("disabled");
    };

    const target = Array.from(
        document.querySelectorAll("button, span[role='button'], div[role='button'], a")
    ).find((el) => {
        const label = text(el);
        if (label !== "同意" && label !== "接受" && label !== "同意并发送") return false;
        if (disabled(el)) return false;
        if (el.closest(".toolbar, .toolbar-btn, .chat-op")) return false;
        let scope = el;
        for (let depth = 0; depth < 4 && scope; depth += 1) {
            if (text(scope).includes("简历")) return true;
            scope = scope.parentElement;
        }
        return false;
    });
    if (!target) return false;

    target.setAttribute("data-fj-accept-resume", "1");
    return true;
})()
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// 改造前是 `page.click(".toolbar-btn")`——点中哪个全看 DOM 顺序。
    /// 这条测试锁死「按文案定位」这个前提，防止有人改回按类名取第一个
    #[test]
    fn resume_button_is_located_by_label_not_by_position() {
        assert!(RESUME_STATE_SCRIPT.contains(r#"text(el) === "发简历""#));
        assert!(!RESUME_STATE_SCRIPT.contains("querySelector(\".toolbar-btn\")"));
        assert!(!RESUME_STATE_SCRIPT.contains("[0]"));
    }

    /// 「换电话」「换微信」和「发简历」同类名，认错就是把联系方式推给陌生人
    #[test]
    fn accept_scan_never_reaches_the_toolbar() {
        for script in [RESUME_STATE_SCRIPT, MARK_ACCEPT_BUTTON_SCRIPT] {
            assert!(script.contains(".toolbar, .toolbar-btn, .chat-op"));
            assert!(script.contains(r#"includes("简历")"#));
        }
    }

    #[test]
    fn disabled_resume_entry_is_reported_as_unavailable() {
        assert!(
            RESUME_STATE_SCRIPT.contains(r#"disabled(resumeButton) ? "unavailable" : "sendable""#)
        );
        // BOSS 用 unable 这个类名表示「等待对方回复」，不能漏
        assert!(RESUME_STATE_SCRIPT.contains(r#"includes("unable")"#));
    }

    #[test]
    fn unrecognised_markup_falls_back_to_unknown_instead_of_guessing() {
        assert!(RESUME_STATE_SCRIPT.contains(r#"if (!resumeButton) return "unknown""#));
    }
}
