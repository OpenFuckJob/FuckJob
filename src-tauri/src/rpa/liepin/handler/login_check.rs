use crate::{browser, rpa::liepin::LIEPIN_SITE_URL};
use anyhow::anyhow;
use rust_drission::utils::sleep_random_ms;
use serde_json::{json, Value};

pub async fn login_check() -> Result<Value, anyhow::Error> {
    let verify_result =
        browser::with_browser(|page| Box::pin(async move { verify_login(page) })).await;
    Ok(build_login_check_output(verify_result.map_err(|e| {
        anyhow!("登录状态异常:{}", summarize_error(&e))
    })))
}

fn verify_login(page: &rust_drission::ChromiumPage) -> Result<(), anyhow::Error> {
    if !page.url()?.contains("liepin.com") {
        page.get(LIEPIN_SITE_URL)?;
        sleep_random_ms(1200, 2000);
    }

    let login_state = page.run_js_await(
        r#"
        (() => {
            const text = document.body ? document.body.innerText : "";
            const hasLoginText = /登录|扫码|验证码/.test(text);
            const hasUserSignal = [
                ".user-info",
                ".user-name",
                ".header-user",
                ".personal-center",
                "a[href*='resume']",
                "a[href*='message']"
            ].some((selector) => document.querySelector(selector));
            return hasUserSignal && !hasLoginText;
        })()
        "#,
    )?;

    let success = login_state
        .get("value")
        .and_then(Value::as_bool)
        .or_else(|| login_state.as_bool())
        .unwrap_or(false);

    if success {
        Ok(())
    } else {
        Err(anyhow!("未检测到猎聘登录态"))
    }
}

fn build_login_check_output(verify_result: Result<(), anyhow::Error>) -> serde_json::Value {
    match verify_result {
        Ok(()) => json!({
            "success": true,
            "message": "登录成功",
        }),
        Err(error) => json!({
            "success": false,
            "message": "登录校验异常",
            "error": summarize_error(&error),
        }),
    }
}

fn summarize_error(error: &anyhow::Error) -> String {
    let message = error.to_string();

    message
        .rsplit(": ")
        .next()
        .map(str::to_string)
        .unwrap_or(message)
}
