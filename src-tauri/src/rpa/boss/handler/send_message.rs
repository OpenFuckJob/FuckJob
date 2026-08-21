use anyhow::anyhow;
use rust_drission::{utils::sleep_random_ms, Page};
use std::{path::Path, time::Duration};

use crate::{
    config::{ReplayResourceType, ReplyResource},
    logger,
    rpa::common::upload_image_to_file_input,
    rpa::human_input,
    rpa::run_flow::is_job_task_stop_requested,
};

const CHAT_INPUT_SELECTOR: &str = "#chat-input";
const SEND_BUTTON_SELECTOR: &str = ".chat-op .btn-send";

// 发送文本消息
pub fn send_text_message(page: &Page, greeting: &str) -> Result<bool, anyhow::Error> {
    page.wait(SEND_BUTTON_SELECTOR, Duration::from_secs(10))?;
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
    let send_btn_ele = page.ele(SEND_BUTTON_SELECTOR)?;
    if let Some(send_btn_ele) = send_btn_ele {
        human_input::click(page, &send_btn_ele)?;
        return Ok(true);
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
    page.run_js(&format!(
        "document.querySelector('{CHAT_INPUT_SELECTOR}').textContent = {greeting_js};"
    ))?;
    Ok(true)
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
