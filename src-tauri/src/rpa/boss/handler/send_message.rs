use anyhow::anyhow;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rust_drission::{utils::sleep_random_ms, Page};
use std::{path::Path, time::Duration};

use crate::{
    config::{ReplayResourceType, ReplyResource},
    logger,
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

// 发送图片
pub fn send_image(page: &Page, image_path: &Path) -> Result<bool, anyhow::Error> {
    let file_data = std::fs::read(image_path)?;
    let base64_data = STANDARD.encode(&file_data);

    let ext = image_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let mime_type = match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    };

    let filename = image_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload.png");

    let base64_str = format!("data:{};base64,{}", mime_type, base64_data);

    let js = format!(
        r#"
(() => {{
  const base64 = "{}";
  const input = document.querySelector('input[type="file"][accept*="image"]');
  if (!input) {{
    console.error("没找到上传 input");
    return;
  }}
  function base64ToFile(base64, filename) {{
    const arr = base64.split(',');
    const mime = arr[0].match(/:(.*?);/)[1];
    const bstr = atob(arr[1]);
    let n = bstr.length;
    const u8arr = new Uint8Array(n);
    while (n--) {{
      u8arr[n] = bstr.charCodeAt(n);
    }}
    return new File([u8arr], filename, {{ type: mime }});
  }}
  const file = base64ToFile(base64, "{}");
  const dt = new DataTransfer();
  dt.items.add(file);
  input.files = dt.files;
  input.dispatchEvent(new Event("change", {{ bubbles: true }}));
  console.log("图片已自动上传");
}})();
"#,
        base64_str, filename
    );

    page.run_js_await(&js)?;
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
