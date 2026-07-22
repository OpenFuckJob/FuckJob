use crate::{
    browser,
    rpa::boss::{BOSS_ACCOUNT_VERIFY_API, BOSS_LOGIN_PAGE_URL},
};
use anyhow::{anyhow, Context};
use rust_drission::utils::sleep_random_ms;
use serde_json::{json, Value};

// 检查登录
pub async fn login_check() -> Result<Value, anyhow::Error> {
    let verify_result =
        browser::with_browser(|page| Box::pin(async move { verify_login(page) })).await;
    let output = build_login_check_output(
        verify_result.map_err(|e| anyhow!("登录状态异常:{}", summarize_error(&e))),
    );
    Ok(output)
}

fn verify_login(page: &rust_drission::ChromiumPage) -> Result<Value, anyhow::Error> {
    if !page.url()?.contains("zhipin.com") {
        page.get(BOSS_LOGIN_PAGE_URL)?;
        sleep_random_ms(1200, 2000);
    }
    let body_text = fetch_via_page_js(page, BOSS_ACCOUNT_VERIFY_API)?;
    if body_text.starts_with("ERR:") {
        return Err(anyhow!("token校验请求失败: {}", body_text));
    }

    parse_verify_response(&body_text)
}

fn fetch_via_page_js(
    page: &rust_drission::ChromiumPage,
    url: &str,
) -> Result<String, anyhow::Error> {
    let script = build_fetch_script(url);
    let result = page.run_js_await(&script)?;
    result
        .get("value")
        .and_then(serde_json::Value::as_str)
        .map(|s| s.to_string())
        .context("页面 fetch 返回值非字符串（缺少 result.value）")
}

fn build_fetch_script(url: &str) -> String {
    format!(
        r#"
        (async () => {{
            try {{
                const response = await fetch({:?}, {{
                    method: 'GET',
                    credentials: 'include'
                }});
                const text = await response.text();
                if (!response.ok) {{
                    return "ERR: HTTP " + response.status + " " + response.statusText + " body=" + text;
                }}
                return text;
            }} catch (e) {{
                return "ERR: " + (e.message || String(e));
            }}
        }})()
        "#,
        url
    )
}

fn parse_verify_response(body_text: &str) -> Result<Value, anyhow::Error> {
    let root: serde_json::Value =
        serde_json::from_str(body_text).context("解析 token 校验 JSON 失败")?;

    let code = root.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(anyhow!(
            "接口业务失败: code={}, message={}",
            code,
            root.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        ));
    }

    let zp_data = root.get("zpData").context("响应缺少 zpData")?;
    if zp_data.is_null() {
        return Err(anyhow!("响应中的 zpData 为空"));
    }

    Ok(root)
}

fn build_login_check_output(verify_result: Result<Value, anyhow::Error>) -> serde_json::Value {
    match verify_result {
        Ok(data) => json!({
            "success": true,
            "message": "登录成功",
            "data": data,
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
