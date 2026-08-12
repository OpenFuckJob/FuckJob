use std::path::Path;

use anyhow::anyhow;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rust_drission::Page;
use serde::{Deserialize, Serialize};

use super::run_flow::PlatformKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpaJob {
    pub platform: PlatformKind,
    pub platform_job_id: String,
    pub title: String,
    pub company_name: String,
    pub detail: String,
    pub salary: String,
    pub location: Option<String>,
    pub detail_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub mid: i64,
    pub received: bool,
    pub text: String,
    pub time: i64,
    pub from_name: String,
}

/// 图片投递到上传控件的结果。
#[derive(Debug, Clone, Default)]
pub struct ImageUploadOutcome {
    pub success: bool,
    pub message: String,
    /// 实际命中的选择器，便于排查各平台 DOM 变化
    pub matched_selector: String,
    /// 命中 input 的 accept 属性原文
    pub accept: String,
    /// 前端上传组件是否确实接管了 change 事件（input 被重建或 files 被清空）
    pub handled: bool,
}

/// 把本地图片写入页面的 file input 并派发 change 事件。
///
/// 各平台上传控件的属性差异很大：Boss 是 `accept="image/*"`，猎聘是
/// `accept="jpg, jpeg, png, bmp"`（不含 image 字样），所以选择器不能写死，
/// 由调用方按优先级传入，命中第一个可用的即停止。
pub fn upload_image_to_file_input(
    page: &Page,
    image_path: &Path,
    input_selectors: &[&str],
) -> Result<ImageUploadOutcome, anyhow::Error> {
    let file_data = std::fs::read(image_path)
        .map_err(|e| anyhow!("读取图片失败 {}: {}", image_path.display(), e))?;
    let base64_data = STANDARD.encode(&file_data);

    let extension = image_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let mime_type = match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "image/png",
    };
    let filename = image_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload.png");

    let script = build_upload_image_script(
        input_selectors,
        &format!("data:{};base64,{}", mime_type, base64_data),
        filename,
    );
    let value = page.run_js_await(&script)?;
    let result = value.get("value").cloned().unwrap_or(value);

    Ok(ImageUploadOutcome {
        success: result
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        message: result
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        matched_selector: result
            .get("matchedSelector")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        accept: result
            .get("accept")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        handled: result
            .get("handled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

fn build_upload_image_script(
    input_selectors: &[&str],
    data_url: &str,
    filename: &str,
) -> String {
    let selectors_json = serde_json::to_string(input_selectors).unwrap_or_else(|_| "[]".to_string());
    let filename_json = serde_json::to_string(filename).unwrap_or_else(|_| "\"upload.png\"".to_string());

    format!(
        r#"
        (async () => {{
            const selectors = {selectors_json};
            const dataUrl = "{data_url}";
            const filename = {filename_json};
            const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
            const usable = (el) => el instanceof HTMLInputElement && el.type === "file" && !el.disabled;

            let input = null;
            let matchedSelector = "";
            for (const selector of selectors) {{
                const found = Array.from(document.querySelectorAll(selector)).find(usable);
                if (found) {{
                    input = found;
                    matchedSelector = selector;
                    break;
                }}
            }}
            if (!input) {{
                return {{ success: false, message: "未找到图片上传输入框", matchedSelector: "" }};
            }}

            const accept = input.getAttribute("accept") || "";
            const parts = dataUrl.split(",");
            const mime = (parts[0].match(/:(.*?);/) || [])[1] || "image/png";
            const binary = atob(parts[1]);
            const bytes = new Uint8Array(binary.length);
            for (let i = 0; i < binary.length; i += 1) {{
                bytes[i] = binary.charCodeAt(i);
            }}

            const transfer = new DataTransfer();
            transfer.items.add(new File([bytes], filename, {{ type: mime }}));
            input.files = transfer.files;
            // file input 不受 React value tracker 影响，原生 change 即可触发 onChange
            input.dispatchEvent(new Event("change", {{ bubbles: true }}));
            await sleep(600);

            // rc-upload 接管后会重建 input（uid 变化）或清空已选文件，以此判断事件是否被消费
            const handled = !document.contains(input) || input.files.length === 0 || !input.value;
            return {{ success: true, message: "已投递到上传控件", matchedSelector, accept, handled }};
        }})()
        "#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_script_builds_file_from_data_url_and_dispatches_change() {
        let script = build_upload_image_script(
            &[".im-ui-upload-container input[type='file']"],
            "data:image/png;base64,AAAA",
            "hello.png",
        );

        assert!(script.contains(".im-ui-upload-container input[type='file']"));
        assert!(script.contains("new DataTransfer()"));
        assert!(script.contains("input.files = transfer.files"));
        assert!(script.contains("new Event(\"change\", { bubbles: true })"));
        assert!(script.contains("未找到图片上传输入框"));
        assert!(script.contains("const handled ="));
    }

    #[test]
    fn upload_script_escapes_filename_with_quotes() {
        let script = build_upload_image_script(&["input[type='file']"], "data:image/png;base64,AA", "a\"b.png");

        assert!(script.contains(r#"const filename = "a\"b.png""#));
    }
}
