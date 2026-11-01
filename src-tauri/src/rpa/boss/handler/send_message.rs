use anyhow::anyhow;
use rust_drission::{utils::sleep_random_ms, Page};
use serde_json::Value;
use std::{
    path::Path,
    time::{Duration, Instant},
};

use crate::{
    config::{ReplayResourceType, ReplyResource},
    logger,
    rpa::common::upload_image_to_file_input,
    rpa::human_input,
    rpa::run_flow::is_job_task_stop_requested,
};

const CHAT_INPUT_SELECTOR: &str = "#chat-input";
const SEND_BUTTON_SELECTOR: &str = ".chat-op .btn-send";
const BOSS_SEND_STATE_SCRIPT: &str = r#"
(() => {
    const input = document.querySelector('#chat-input');
    const button = document.querySelector('.chat-op .btn-send');
    const messages = Array.from(document.querySelectorAll('.message-item.item-myself'))
        .filter(node => !node.querySelector('.status-error, .status-fail, .send-fail'))
        .flatMap(node => Array.from(node.querySelectorAll('.text-content')).map(text => text.textContent || ''));
    return {
        inputExists: Boolean(input),
        inputText: input ? (input.value ?? input.textContent ?? '') : '',
        sendEnabled: Boolean(button && !button.disabled && !button.classList.contains('disabled') && button.getAttribute('aria-disabled') !== 'true'),
        messages
    };
})()
"#;

// 发送文本消息
pub fn send_text_message(page: &Page, greeting: &str) -> Result<bool, anyhow::Error> {
    if greeting.trim().is_empty() {
        return Ok(false);
    }
    page.wait(SEND_BUTTON_SELECTOR, Duration::from_secs(10))?;
    let before = read_send_state(page)?;
    if !type_greeting(page, greeting)? {
        return Ok(false);
    }
    // 逐字输入一条长招呼语要几十秒，这中间用户完全可能点了停止。
    // 内容已经填进输入框了，但按不按发送是另一回事
    if is_job_task_stop_requested() {
        logger::info("任务已停止，本条消息未发送")?;
        return Ok(false);
    }
    // input-area
    sleep_random_ms(900, 1500);
    if is_job_task_stop_requested() {
        return Ok(false);
    }
    let ready = read_send_state(page)?;
    if ready.get("sendEnabled").and_then(Value::as_bool) != Some(true) {
        return Ok(false);
    }
    let send_btn_ele = page.ele(SEND_BUTTON_SELECTOR)?;
    if let Some(send_btn_ele) = send_btn_ele {
        if is_job_task_stop_requested() {
            return Ok(false);
        }
        send_btn_ele.click()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if text_send_confirmed(&before, &read_send_state(page)?, greeting) {
                logger::info("BOSS 文本发送已确认：出现新的发送气泡，输入框已清空")?;
                return Ok(true);
            }
            if Instant::now() >= deadline || is_job_task_stop_requested() {
                logger::warning("BOSS 消息发送状态未确认，保留未投递状态")?;
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(false)
}

/// 把招呼语填进聊天输入框。
///
/// 拟人化开着时逐字符敲进去——CDP 的 `Input.insertText` 触发的是真实的
/// `beforeinput` / `input`，而直接给 `textContent` 赋值这一路，站点侧看到的是
/// 内容凭空出现、没有任何输入事件。
///
/// 打字这条路只要没走通就整段回退到赋值：残缺的半句话发给 HR 比机器特征糟得多，
/// 所以回退前先把输入框清空，绝不在已有内容上接着写
fn type_greeting(page: &Page, greeting: &str) -> Result<bool, anyhow::Error> {
    clear_chat_input(page)?;
    if let Some(input) = page.ele(CHAT_INPUT_SELECTOR)? {
        match human_input::type_text(page, &input, greeting) {
            Ok(true) => return Ok(true),
            Ok(false) => clear_chat_input(page)?,
            Err(error) => {
                logger::warning(format!("逐字输入失败，改用整段填入：{error}"))?;
                clear_chat_input(page)?;
            }
        }
    }

    let greeting_js = serde_json::to_string(greeting).map_err(|e| anyhow!("{}", e))?;
    let value = page.run_js_await(&format!(
        "(() => {{ const input = document.querySelector('{CHAT_INPUT_SELECTOR}'); if (!input) return false; input.textContent = {greeting_js}; input.dispatchEvent(new InputEvent('input', {{ bubbles: true, inputType: 'insertText', data: {greeting_js} }})); input.dispatchEvent(new Event('change', {{ bubbles: true }})); return true; }})()"
    ))?;
    Ok(value.get("value").unwrap_or(&value).as_bool() == Some(true))
}

fn read_send_state(page: &Page) -> Result<Value, anyhow::Error> {
    let result = page.run_js_await(BOSS_SEND_STATE_SCRIPT)?;
    if result.get("subtype").and_then(Value::as_str) == Some("error") {
        return Err(anyhow!("读取 BOSS 发送状态失败：{result}"));
    }
    Ok(result.get("value").cloned().unwrap_or(result))
}

fn text_send_confirmed(before: &Value, after: &Value, greeting: &str) -> bool {
    let normalize = |text: &str| text.split_whitespace().collect::<String>();
    let expected = normalize(greeting);
    let count_matches = |state: &Value| {
        state
            .get("messages")
            .and_then(Value::as_array)
            .map_or(0, |messages| {
                messages
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|text| normalize(text) == expected)
                    .count()
            })
    };
    after.get("inputExists").and_then(Value::as_bool) == Some(true)
        && after
            .get("inputText")
            .and_then(Value::as_str)
            .is_some_and(|text| text.trim().is_empty())
        && count_matches(after) > count_matches(before)
}

fn clear_chat_input(page: &Page) -> Result<(), anyhow::Error> {
    page.run_js(&format!(
        "(() => {{ const el = document.querySelector('{CHAT_INPUT_SELECTOR}'); if (el) el.textContent = ''; }})();"
    ))?;
    Ok(())
}

// 不加 `input[type=file]` 裸兜底：Boss 聊天页还有简历上传框，误命中会把图片塞错地方
const BOSS_IMAGE_INPUT_SELECTORS: &[&str] = &["input[type='file'][accept*='image']"];

// 发送图片
pub fn send_image(page: &Page, image_path: &Path) -> Result<bool, anyhow::Error> {
    let outcome = upload_image_to_file_input(page, image_path, BOSS_IMAGE_INPUT_SELECTORS)?;
    if !outcome.success {
        logger::warning(format!("Boss 图片上传失败：{}", outcome.message))?;
        return Ok(false);
    }

    logger::info(format!(
        "Boss 图片已投递到上传控件（selector={} handled={}）",
        outcome.matched_selector, outcome.handled
    ))?;
    Ok(true)
}

/// 给定回复列表资源 依次执行
pub fn send_messages(page: &Page, resources: Vec<ReplyResource>) -> Result<bool, anyhow::Error> {
    for resource in resources {
        if resource.content.trim().is_empty() {
            continue;
        }
        let sent = match resource.resource_type {
            ReplayResourceType::Text | ReplayResourceType::LLM => {
                send_text_message(page, &resource.content)?
            }
            ReplayResourceType::Image => {
                let res = send_image(page, Path::new(&resource.content))?;
                sleep_random_ms(800, 1000);
                res
            }
        };

        if !sent {
            logger::warning(format!("发送消息失败:{:?}", resource.resource_type))?;
            return Ok(false);
        }

        sleep_random_ms(500, 1000);
    }

    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sending_requires_a_new_matching_outgoing_bubble_and_a_cleared_composer() {
        let before = serde_json::json!({"messages": ["你好"], "inputExists": true, "inputText": ""});
        let new_message = serde_json::json!({"messages": ["你好", "你好"], "inputExists": true, "inputText": ""});
        assert!(text_send_confirmed(&before, &new_message, "你好"));
        assert!(!text_send_confirmed(&before, &before, "你好"));
        let retained_draft = serde_json::json!({"messages": ["你好", "你好"], "inputExists": true, "inputText": "你好"});
        assert!(!text_send_confirmed(&before, &retained_draft, "你好"));
        let missing_input = serde_json::json!({"messages": ["你好", "你好"], "inputExists": false, "inputText": ""});
        assert!(!text_send_confirmed(&before, &missing_input, "你好"));
    }

    #[test]
    fn send_message_module_has_no_async_llm_send_path() {
        let source = include_str!("send_message.rs");
        let async_api_name = ["send_messages", "with_llm"].join("_");
        let pending_llm_variant = ["PendingText", "Llm"].join("::");
        let spawn_call = ["tauri::async_runtime::spawn", "async move"].join("(");

        assert!(!source.contains(&async_api_name));
        assert!(!source.contains(&pending_llm_variant));
        assert!(!source.contains(&spawn_call));
    }
}
