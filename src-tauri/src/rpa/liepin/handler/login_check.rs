use crate::{
    browser, logger,
    rpa::liepin::{LIEPIN_USER_HOME_URL, LIEPIN_USER_PROPERTY_API},
};
use anyhow::anyhow;
use rust_drission::{utils::sleep_random_ms, ChromiumPage};
use serde_json::{json, Value};

/// 页面就绪判定的正文长度下限。猎聘页面渲染完成后正文远超这个量级，
/// 低于它基本可以认定 DOM 还没出来。
const MIN_READY_TEXT_LENGTH: i64 = 80;
const API_PROBE_MAX_ATTEMPTS: usize = 3;

pub async fn login_check() -> Result<Value, anyhow::Error> {
    let verify_result =
        browser::with_browser(|page| Box::pin(async move { verify_login(page) })).await;
    Ok(build_login_check_output(verify_result.map_err(|e| {
        anyhow!("登录状态异常:{}", summarize_error(&e))
    })))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginState {
    LoggedIn,
    LoggedOut,
    Unknown,
}

/// 用户信息接口的判定结果。
///
/// 只在拿到强证据时下结论：接口确认身份即已登录，401/403 即未登录，
/// 其余（CORS 被拦、断网、5xx、返回体看不懂）一律 Unknown 交给页面特征兜底，
/// 否则一次网络抖动就会把已登录的会话踢去扫码。
#[derive(Debug, Clone, PartialEq, Eq)]
enum ApiVerdict {
    LoggedIn(String),
    LoggedOut(String),
    Unknown(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ApiResponse {
    status: i64,
    body: String,
    error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct LoginProbe {
    ready: bool,
    text_length: i64,
    on_login_page: bool,
    has_login_entry: bool,
    has_user_signal: bool,
}

impl LoginProbe {
    fn state(&self) -> LoginState {
        // 页面没渲染完时所有信号都是空的，这时判“未登录”会把正在刷新的已登录会话
        // 直接拽去扫码页，所以必须先归为未知。
        if !self.ready || self.text_length < MIN_READY_TEXT_LENGTH {
            return LoginState::Unknown;
        }
        if self.on_login_page || self.has_login_entry {
            return LoginState::LoggedOut;
        }
        if self.has_user_signal {
            return LoginState::LoggedIn;
        }
        LoginState::Unknown
    }
}

fn verify_login(page: &ChromiumPage) -> Result<(), anyhow::Error> {
    // 接口的 Access-Control-Allow-Origin 精确等于 https://c.liepin.com，
    // 站在 www.liepin.com 上发请求会被浏览器拦掉，所以先落到候选人端主页。
    if !page.url()?.contains("c.liepin.com") {
        page.get(LIEPIN_USER_HOME_URL)?;
    }
    // 不论有没有发生导航都要等：页面刷新时 URL 已经是 liepin.com 但 DOM 还是空的
    let ready = wait_until_page_ready(page)?;

    let mut last_reason = String::new();
    for attempt in 0..API_PROBE_MAX_ATTEMPTS {
        match probe_via_user_api(page)? {
            ApiVerdict::LoggedIn(user_name) => {
                // 日志写失败不能影响登录判定，否则又多一条误判路径
                let _ = logger::info(format!("猎聘登录态已确认（当前账号：{}）", user_name));
                return Ok(());
            }
            ApiVerdict::LoggedOut(reason) => {
                return Err(anyhow!("猎聘登录态已失效（{}），需要重新扫码", reason));
            }
            ApiVerdict::Unknown(reason) => {
                // 重试过程不逐次记录，只有最终没结论时才汇报一次
                last_reason = reason;
                if attempt + 1 < API_PROBE_MAX_ATTEMPTS {
                    sleep_random_ms(1000, 1600);
                }
            }
        }
    }

    // 接口不可用（改版、被风控、离线）时降级到页面特征，不至于整个功能瘫掉
    let _ = logger::warning(format!(
        "猎聘用户接口无法确认登录态（{}），改用页面特征判断",
        last_reason
    ));
    let probe = probe_login_state(page)?;
    match probe.state() {
        LoginState::LoggedIn => Ok(()),
        LoginState::LoggedOut => Err(anyhow!("页面停留在猎聘登录入口，需要重新扫码")),
        LoginState::Unknown => Err(anyhow!(
            "无法确认登录态（接口：{}；页面就绪={} 正文长度={}）",
            last_reason,
            ready && probe.ready,
            probe.text_length
        )),
    }
}

fn probe_via_user_api(page: &ChromiumPage) -> Result<ApiVerdict, anyhow::Error> {
    let value = page.run_js_await(&build_user_api_script())?;
    let result = value.get("value").cloned().unwrap_or(value);

    Ok(classify_api_response(&ApiResponse {
        status: result
            .get("status")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        body: result
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        error: result
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }))
}

fn classify_api_response(response: &ApiResponse) -> ApiVerdict {
    // fetch 抛异常（CORS 被拦、断网）时拿不到状态码
    if response.status == 0 {
        return ApiVerdict::Unknown(if response.error.is_empty() {
            "请求未拿到响应".to_string()
        } else {
            format!("请求失败：{}", response.error)
        });
    }
    if response.status == 401 || response.status == 403 {
        return ApiVerdict::LoggedOut(format!("接口返回 {}", response.status));
    }
    if !(200..300).contains(&response.status) {
        return ApiVerdict::Unknown(format!("接口返回 HTTP {}", response.status));
    }

    let Ok(body) = serde_json::from_str::<Value>(&response.body) else {
        return ApiVerdict::Unknown("接口响应不是合法 JSON".to_string());
    };

    let user_name = body
        .pointer("/data/userName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if body.get("flag").and_then(Value::as_i64) == Some(1) && !user_name.is_empty() {
        return ApiVerdict::LoggedIn(user_name);
    }

    // flag 不为 1 也可能是接口改版或业务异常，只有出现明确的登录关键字才判失效
    let serialized = response.body.to_lowercase();
    if serialized.contains("not login")
        || serialized.contains("unlogin")
        || serialized.contains("not_login")
        || response.body.contains("未登录")
        || response.body.contains("重新登录")
    {
        return ApiVerdict::LoggedOut("接口提示登录态失效".to_string());
    }

    ApiVerdict::Unknown("接口未返回可识别的用户信息".to_string())
}

fn build_user_api_script() -> String {
    format!(
        r#"
        (async () => {{
            const readCookie = (name) => {{
                const matched = document.cookie.match(new RegExp("(^|;\\s*)" + name + "=([^;]*)"));
                return matched ? decodeURIComponent(matched[2]) : "";
            }};
            // XSRF-TOKEN 每个会话都不同，必须现读现用
            const xsrfToken = readCookie("XSRF-TOKEN");
            const traceId = (window.crypto && crypto.randomUUID)
                ? crypto.randomUUID()
                : "trace-" + performance.now().toString(36);

            const headers = {{
                "Accept": "application/json, text/plain, */*",
                "Content-Type": "application/x-www-form-urlencoded",
                "X-Client-Type": "web",
                "X-Requested-With": "XMLHttpRequest",
                "X-Fscp-Version": "1.1",
                "X-Fscp-Std-Info": '{{"client_id": "40106"}}',
                "X-Fscp-Trace-Id": traceId,
                "X-Fscp-Bi-Stat": JSON.stringify({{ location: location.href }})
            }};
            if (xsrfToken) {{
                headers["X-XSRF-TOKEN"] = xsrfToken;
            }}

            try {{
                const response = await fetch("{LIEPIN_USER_PROPERTY_API}", {{
                    method: "POST",
                    credentials: "include",
                    headers,
                    body: ""
                }});
                const body = await response.text();
                return {{ status: response.status, body, error: "" }};
            }} catch (error) {{
                return {{ status: 0, body: "", error: String((error && error.message) || error) }};
            }}
        }})()
        "#
    )
}

/// 等到 DOM 真正渲染出来，最多约 12 秒。返回是否等到就绪。
fn wait_until_page_ready(page: &ChromiumPage) -> Result<bool, anyhow::Error> {
    let value = page.run_js_await(&build_wait_page_ready_script())?;
    let result = value.get("value").cloned().unwrap_or(value);
    Ok(result
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

fn build_wait_page_ready_script() -> String {
    format!(
        r#"
        (async () => {{
            const minTextLength = {MIN_READY_TEXT_LENGTH};
            const stepMs = 300;
            const maxAttempts = 40;
            const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

            for (let attempt = 0; attempt < maxAttempts; attempt += 1) {{
                const text = document.body ? (document.body.innerText || "") : "";
                if (document.readyState === "complete" && text.trim().length >= minTextLength) {{
                    return {{ ready: true, waitedMs: attempt * stepMs }};
                }}
                await sleep(stepMs);
            }}
            return {{ ready: false, waitedMs: maxAttempts * stepMs }};
        }})()
        "#
    )
}

fn probe_login_state(page: &ChromiumPage) -> Result<LoginProbe, anyhow::Error> {
    let value = page.run_js_await(PROBE_LOGIN_STATE_SCRIPT)?;
    let result = value.get("value").cloned().unwrap_or(value);

    Ok(LoginProbe {
        ready: result
            .get("ready")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        text_length: result
            .get("textLength")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        on_login_page: result
            .get("onLoginPage")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        has_login_entry: result
            .get("hasLoginEntry")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        has_user_signal: result
            .get("hasUserSignal")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

const PROBE_LOGIN_STATE_SCRIPT: &str = r#"
    (() => {
        const text = document.body ? (document.body.innerText || "") : "";
        const href = location.href || "";
        const visible = (el) => {
            if (!el) return false;
            const rect = el.getBoundingClientRect();
            const style = window.getComputedStyle(el);
            return rect.width > 0 && rect.height > 0
                && style.visibility !== "hidden"
                && style.display !== "none";
        };

        // 登录页有独立 URL（/simple-login/），比在正文里搜“登录”二字可靠得多：
        // 已登录首页的页脚、帮助入口同样会出现“登录”。
        const onLoginPage = /\/(simple-)?login|\/passport|\/account\/login/i.test(href);
        const hasLoginEntry = Boolean(
            document.querySelector("input[type='password']")
            || document.querySelector("img[class*='qrcode-img']")
            || document.querySelector("img[src*='qrcode']:not([src*='qrcode-btn'])")
            || Array
                .from(document.querySelectorAll("a[href*='login'], [class*='login-btn'], [class*='btn-login']"))
                .some(visible)
            || /扫码登录|手机号登录|验证码登录|密码登录/.test(text)
        );
        const hasUserSignal = [
            ".user-info",
            ".user-name",
            ".header-user",
            ".personal-center",
            "a[href*='resume']",
            "a[href*='message']"
        ].some((selector) => document.querySelector(selector));

        return {
            ready: document.readyState === "complete",
            textLength: text.trim().length,
            onLoginPage,
            hasLoginEntry,
            hasUserSignal
        };
    })()
"#;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn api(status: i64, body: &str) -> ApiResponse {
        ApiResponse {
            status,
            body: body.to_string(),
            error: String::new(),
        }
    }

    fn probe(ready: bool, text_length: i64) -> LoginProbe {
        LoginProbe {
            ready,
            text_length,
            ..LoginProbe::default()
        }
    }

    #[test]
    fn real_user_property_response_is_recognized_as_logged_in() {
        let body = r#"{"flag":1,"data":{"userName":"李炳桦","sexName":"男","resId":"9b27608d8321R47b876959abe","userShowName":"李先生"}}"#;

        assert_eq!(
            classify_api_response(&api(200, body)),
            ApiVerdict::LoggedIn("李炳桦".to_string())
        );
    }

    #[test]
    fn unauthorized_status_means_logged_out() {
        assert!(matches!(
            classify_api_response(&api(401, "")),
            ApiVerdict::LoggedOut(_)
        ));
        assert!(matches!(
            classify_api_response(&api(403, "")),
            ApiVerdict::LoggedOut(_)
        ));
    }

    #[test]
    fn cors_or_network_failure_is_unknown_not_logged_out() {
        // 站错域名时浏览器直接拦掉 fetch，这种情况绝不能判成未登录
        let blocked = ApiResponse {
            status: 0,
            body: String::new(),
            error: "Failed to fetch".to_string(),
        };

        assert!(matches!(
            classify_api_response(&blocked),
            ApiVerdict::Unknown(_)
        ));
    }

    #[test]
    fn server_error_and_unparsable_body_are_unknown() {
        assert!(matches!(
            classify_api_response(&api(502, "<html>bad gateway</html>")),
            ApiVerdict::Unknown(_)
        ));
        assert!(matches!(
            classify_api_response(&api(200, "not json")),
            ApiVerdict::Unknown(_)
        ));
    }

    #[test]
    fn flag_success_without_user_name_is_unknown() {
        // 接口改版可能 flag 仍为 1 但结构变了，此时不下结论、交给页面特征
        assert!(matches!(
            classify_api_response(&api(200, r#"{"flag":1,"data":{}}"#)),
            ApiVerdict::Unknown(_)
        ));
        assert!(matches!(
            classify_api_response(&api(200, r#"{"flag":1,"data":null}"#)),
            ApiVerdict::Unknown(_)
        ));
    }

    #[test]
    fn explicit_login_hint_in_body_means_logged_out() {
        assert!(matches!(
            classify_api_response(&api(200, r#"{"flag":0,"message":"用户未登录"}"#)),
            ApiVerdict::LoggedOut(_)
        ));
        assert!(matches!(
            classify_api_response(&api(200, r#"{"flag":0,"code":"NOT_LOGIN"}"#)),
            ApiVerdict::LoggedOut(_)
        ));
    }

    #[test]
    fn unrecognized_business_failure_is_unknown() {
        // 单纯 flag != 1 不足以判定未登录，可能只是接口自身异常
        assert!(matches!(
            classify_api_response(&api(200, r#"{"flag":0,"message":"系统繁忙"}"#)),
            ApiVerdict::Unknown(_)
        ));
    }

    #[test]
    fn api_script_targets_candidate_api_with_credentials_and_fresh_xsrf_token() {
        let script = build_user_api_script();

        assert!(script.contains(LIEPIN_USER_PROPERTY_API));
        assert!(script.contains("credentials: \"include\""));
        assert!(script.contains("readCookie(\"XSRF-TOKEN\")"));
        assert!(script.contains("\"X-Client-Type\": \"web\""));
        assert!(script.contains(r#"'{"client_id": "40106"}'"#));
        assert!(script.contains("method: \"POST\""));
    }

    #[test]
    fn page_still_loading_is_unknown_instead_of_logged_out() {
        // 刷新中：URL 已经是 liepin.com 但 DOM 为空，此时不能判未登录
        assert_eq!(probe(false, 0).state(), LoginState::Unknown);
        assert_eq!(probe(true, 0).state(), LoginState::Unknown);
        assert_eq!(
            probe(true, MIN_READY_TEXT_LENGTH - 1).state(),
            LoginState::Unknown
        );
    }

    #[test]
    fn ready_page_without_any_signal_is_unknown_not_logged_out() {
        assert_eq!(probe(true, 2000).state(), LoginState::Unknown);
    }

    #[test]
    fn login_page_or_visible_login_entry_means_logged_out() {
        let on_login_page = LoginProbe {
            ready: true,
            text_length: 2000,
            on_login_page: true,
            ..LoginProbe::default()
        };
        let with_entry = LoginProbe {
            ready: true,
            text_length: 2000,
            has_login_entry: true,
            ..LoginProbe::default()
        };

        assert_eq!(on_login_page.state(), LoginState::LoggedOut);
        assert_eq!(with_entry.state(), LoginState::LoggedOut);
    }

    #[test]
    fn user_signal_on_ready_page_means_logged_in() {
        let logged_in = LoginProbe {
            ready: true,
            text_length: 2000,
            has_user_signal: true,
            ..LoginProbe::default()
        };

        assert_eq!(logged_in.state(), LoginState::LoggedIn);
    }

    #[test]
    fn login_page_wins_over_user_signal() {
        let conflicting = LoginProbe {
            ready: true,
            text_length: 2000,
            on_login_page: true,
            has_user_signal: true,
            ..LoginProbe::default()
        };

        assert_eq!(conflicting.state(), LoginState::LoggedOut);
    }

    #[test]
    fn probe_script_detects_login_page_by_url_not_by_body_keyword() {
        assert!(PROBE_LOGIN_STATE_SCRIPT.contains("simple-login"));
        assert!(PROBE_LOGIN_STATE_SCRIPT.contains("location.href"));
        assert!(!PROBE_LOGIN_STATE_SCRIPT.contains("/登录|扫码|验证码/"));
    }

    #[test]
    fn wait_script_polls_until_dom_actually_rendered() {
        let script = build_wait_page_ready_script();

        assert!(script.contains("document.readyState === \"complete\""));
        assert!(script.contains("const minTextLength = 80"));
        assert!(script.contains("maxAttempts = 40"));
    }
}
