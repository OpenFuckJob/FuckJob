use anyhow::anyhow;
use rust_drission::{utils::sleep_random_ms, Page};
use std::{path::Path, time::Duration};

use crate::{
    config::{ReplayResourceType, ReplyResource},
    logger,
    rpa::common::upload_image_to_file_input,
};

// 发送文本消息
pub fn send_text_message(page: &Page, greeting: &str) -> Result<bool, anyhow::Error> {
    let greeting_js = serde_json::to_string(greeting).map_err(|e| anyhow!("{}", e))?;
    page.wait(".chat-op .btn-send", Duration::from_secs(10))?;
    page.run_js(&format!(
        "document.querySelector('#chat-input').textContent = {};",
        greeting_js
    ))?;
    // input-area
    sleep_random_ms(900, 1500);
    let send_btn_selector = ".chat-op .btn-send";
    let send_btn_ele = page.ele(send_btn_selector)?;
    if let Some(send_btn_ele) = send_btn_ele {
        send_btn_ele.click()?;
        return Ok(true);
    }
    Ok(false)
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
