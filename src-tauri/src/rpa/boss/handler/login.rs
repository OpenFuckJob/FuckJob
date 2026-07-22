use std::time::Duration;

use crate::{browser, rpa::boss::BOSS_LOGIN_PAGE_URL};
use anyhow::{anyhow, Context, Ok};
use rust_drission::utils::sleep_random_ms;
use serde_json::Value;

// 获取登录二维码
pub async fn login() -> Result<String, anyhow::Error> {
    browser::with_browser(|page| {
        Box::pin(async move {
            page.get(BOSS_LOGIN_PAGE_URL)?;
            page.wait(".btn-sign-switch", Duration::from_secs(10))?;
            let ewm_switch_ele = page.ele(".btn-sign-switch.ewm-switch")?;
            if ewm_switch_ele.is_some() {
                page.click(".ewm-switch")?;
                sleep_random_ms(500, 1000);
            }

            qr_base64(page)
        })
    })
    .await
}

fn qr_base64(page: &rust_drission::ChromiumPage) -> Result<String, anyhow::Error> {
    let img = page
        .ele(".qr-img-box img")?
        .ok_or_else(|| anyhow!("未找到二维码图片元素"))?;

    let src = img.attr("src")?;
    let _ = src;

    let script = format!(
        r#"
        (async () => {{
            const img = document.querySelector('.qr-img-box img');
            if (!img) {{
                throw new Error('QR image not found');
            }}

            const src = img.getAttribute('src');
            if (!src) {{
                throw new Error('QR image src missing');
            }}

            const response = await fetch({src:?});
            const blob = await response.blob();

            return await new Promise((resolve, reject) => {{
                const reader = new FileReader();
                reader.onloadend = () => resolve(reader.result);
                reader.onerror = () => reject(new Error('failed to read blob as data url'));
                reader.readAsDataURL(blob);
            }});
        }})()
        "#
    );

    let data_url = page
        .run_js_await(&script)
        .context("浏览器执行二维码导出脚本失败")?;

    qr_base64_from_data_url(&data_url)
}

fn qr_base64_from_data_url(value: &Value) -> Result<String, anyhow::Error> {
    let data_url = extract_js_string(value)?;
    let prefix = "data:";
    let base64_marker = ";base64,";

    let body = data_url
        .strip_prefix(prefix)
        .ok_or_else(|| anyhow!("二维码导出结果不是 data URL"))?;
    let (_content_type, encoded) = body
        .split_once(base64_marker)
        .ok_or_else(|| anyhow!("二维码导出结果不是 base64 data URL"))?;

    Ok(encoded.to_string())
}

fn extract_js_string(value: &Value) -> Result<&str, anyhow::Error> {
    if let Some(text) = value.as_str() {
        return Ok(text);
    }

    if let Some(text) = value.get("value").and_then(Value::as_str) {
        return Ok(text);
    }

    if value.get("subtype").and_then(Value::as_str) == Some("error") {
        let message = value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("浏览器脚本执行失败");
        return Err(anyhow!("浏览器脚本执行失败: {message}"));
    }

    value
        .get("description")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("二维码导出结果不是字符串"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_base64_from_data_url() {
        let encoded =
            qr_base64_from_data_url(&Value::String("data:image/png;base64,aGVsbG8=".to_string()))
                .unwrap();

        assert_eq!(encoded, "aGVsbG8=");
    }

    #[test]
    fn rejects_non_base64_data_url() {
        let error = qr_base64_from_data_url(&Value::String("data:image/png,hello".to_string()))
            .unwrap_err();

        assert_eq!(error.to_string(), "二维码导出结果不是 base64 data URL");
    }
}
