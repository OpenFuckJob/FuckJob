use std::time::Duration;

use crate::{browser, rpa::liepin::LIEPIN_LOGIN_PAGE_URL};
use anyhow::{anyhow, Context};
use rust_drission::utils::sleep_random_ms;
use serde_json::Value;

pub async fn login() -> Result<String, anyhow::Error> {
    browser::with_browser(|page| {
        Box::pin(async move {
            page.get(LIEPIN_LOGIN_PAGE_URL)?;
            sleep_random_ms(800, 1200);
            show_qr_login(page)?;
            sleep_random_ms(500, 900);
            wait_for_qr(page)?;
            qr_base64(page)
        })
    })
    .await
}

fn show_qr_login(page: &rust_drission::ChromiumPage) -> Result<(), anyhow::Error> {
    let value = page
        .run_js_await(build_show_qr_login_script())
        .context("浏览器执行猎聘二维码入口点击脚本失败")?;
    let clicked_qr_button = value
        .get("value")
        .and_then(Value::as_bool)
        .or_else(|| value.as_bool())
        .unwrap_or(false);

    if clicked_qr_button {
        Ok(())
    } else {
        Err(anyhow!("未找到猎聘二维码登录入口"))
    }
}

fn build_show_qr_login_script() -> &'static str {
    r#"
        (async () => {
            const click = (element) => {
                if (!element) return false;
                element.click();
                return true;
            };

            const existingAccountLink = Array
                .from(document.querySelectorAll("a.button, a"))
                .find((element) => (element.innerText || "").trim().includes("我已有账号，直接登录"));
            click(existingAccountLink);
            await new Promise((resolve) => setTimeout(resolve, 500));

            const qrcodeButton = Array
                .from(document.querySelectorAll("img"))
                .find((img) => {
                    const src = img.getAttribute("src") || "";
                    const className = img.getAttribute("class") || "";
                    return (src.includes("concat.lietou-static.com")
                        && src.includes("qrcode-btn"))
                        || className.includes("qrcode-btn");
                });

            return click(qrcodeButton);
        })()
    "#
}

fn wait_for_qr(page: &rust_drission::ChromiumPage) -> Result<(), anyhow::Error> {
    for selector in [
        "img[src*='qr']:not([src*='qrcode-btn'])",
        "img[src*='qrcode']:not([src*='qrcode-btn'])",
        "img.ScanQrCode-qrcode-img",
        "img[class*='qrcode-img']",
        ".qrcode img:not([src*='qrcode-btn'])",
        ".qr-code img:not([src*='qrcode-btn'])",
        ".scan-code img:not([src*='qrcode-btn'])",
        "canvas",
    ] {
        if page.wait(selector, Duration::from_secs(3)).is_ok() && page.ele(selector)?.is_some() {
            return Ok(());
        }
    }

    Err(anyhow!("未找到猎聘登录二维码"))
}

fn qr_base64(page: &rust_drission::ChromiumPage) -> Result<String, anyhow::Error> {
    let data_url = page
        .run_js_await(build_qr_base64_script())
        .context("浏览器执行猎聘二维码导出脚本失败")?;

    qr_base64_from_data_url(&data_url)
}

fn build_qr_base64_script() -> &'static str {
    r#"
        (async () => {
            const selectors = [
                "img[src*='qr']:not([src*='qrcode-btn'])",
                "img[src*='qrcode']:not([src*='qrcode-btn'])",
                "img.ScanQrCode-qrcode-img",
                "img[class*='qrcode-img']",
                ".qrcode img:not([src*='qrcode-btn'])",
                ".qr-code img:not([src*='qrcode-btn'])",
                ".scan-code img:not([src*='qrcode-btn'])"
            ];
            const img = selectors.map((selector) => document.querySelector(selector)).find(Boolean);
            if (img && img.getAttribute("src")) {
                const src = img.getAttribute("src");
                if (src.startsWith("data:")) {
                    return src;
                }
                const response = await fetch(src);
                const blob = await response.blob();
                return await new Promise((resolve, reject) => {
                    const reader = new FileReader();
                    reader.onloadend = () => resolve(reader.result);
                    reader.onerror = () => reject(new Error("failed to read blob as data url"));
                    reader.readAsDataURL(blob);
                });
            }

            const canvas = document.querySelector("canvas");
            if (canvas) {
                return canvas.toDataURL("image/png");
            }

            throw new Error("QR image not found");
        })()
    "#
}

fn qr_base64_from_data_url(value: &Value) -> Result<String, anyhow::Error> {
    let data_url = extract_js_string(value)?;
    let body = data_url
        .strip_prefix("data:")
        .ok_or_else(|| anyhow!("二维码导出结果不是 data URL"))?;
    let (_content_type, encoded) = body
        .split_once(";base64,")
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
    fn qr_login_script_targets_existing_account_and_qrcode_button() {
        let script = build_show_qr_login_script();

        assert!(script.contains("我已有账号，直接登录"));
        assert!(script.contains("qrcode-btn"));
        assert!(script.contains("concat.lietou-static.com"));
    }

    #[test]
    fn qr_export_script_excludes_qrcode_button_icon() {
        let script = build_qr_base64_script();

        assert!(script.contains(":not([src*='qrcode-btn'])"));
    }

    #[test]
    fn qr_export_script_targets_liepin_scan_qrcode_image_class() {
        let script = build_qr_base64_script();

        assert!(script.contains("ScanQrCode-qrcode-img"));
        assert!(script.contains("img[class*='qrcode-img']"));
    }
}
