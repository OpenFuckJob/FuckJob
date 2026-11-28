//! 可选的实时 Webhook 通知模块（飞书 / 企业微信 / 钉钉 / Bark / 自定义 JSON）。
//!
//! 隐私边界：默认完全关闭；仅在用户主动启用并填写目标 Webhook URL 后，
//! 才向用户显式配置的地址发送轻量通知（待人工处理 / 风控中断 / 收到 HR 回复）。

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::config::{NotificationConfig, NotificationProvider};

/// 通知事件类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationEvent {
    ManualReview,
    RiskAlert,
    HrReply,
    Test,
}

impl NotificationEvent {
    fn tag(self) -> &'static str {
        match self {
            Self::ManualReview => "待人工处理",
            Self::RiskAlert => "风控/异常提醒",
            Self::HrReply => "收到 HR 回复",
            Self::Test => "连接测试",
        }
    }
}

/// 按提供商拼装 Webhook 请求体。纯函数，便于单测验证各平台格式。
pub fn build_webhook_payload(
    provider: NotificationProvider,
    event: NotificationEvent,
    title: &str,
    body: &str,
) -> Value {
    let headline = format!("【OfferFlow · {}】{}", event.tag(), title.trim());
    let content = if body.trim().is_empty() {
        headline.clone()
    } else {
        format!("{headline}\n{}", body.trim())
    };

    match provider {
        NotificationProvider::Feishu => json!({
            "msg_type": "text",
            "content": { "text": content }
        }),
        NotificationProvider::Wecom | NotificationProvider::Dingtalk => json!({
            "msgtype": "text",
            "text": { "content": content }
        }),
        NotificationProvider::Bark => {
            let trimmed_body = body.trim();
            json!({
                "title": headline,
                "body": if trimmed_body.is_empty() { headline.clone() } else { trimmed_body.to_string() },
                "group": "OfferFlow"
            })
        }
        NotificationProvider::Generic => json!({
            "source": "OfferFlow",
            "event": event.tag(),
            "title": title.trim(),
            "body": body.trim(),
            "text": content
        }),
    }
}

/// 判断该事件在当前通知配置下是否应当发送。
pub fn should_send(config: &NotificationConfig, event: NotificationEvent) -> bool {
    if !config.enabled || config.webhook_url.trim().is_empty() {
        return false;
    }
    match event {
        NotificationEvent::ManualReview => config.notify_manual_review,
        NotificationEvent::RiskAlert => config.notify_risk_alert,
        NotificationEvent::HrReply => config.notify_hr_reply,
        NotificationEvent::Test => true,
    }
}

/// 校验 Webhook URL 合法性（只允许 http/https，拒绝无效地址）。
pub fn validate_webhook_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("请先填写 Webhook 地址".to_string());
    }
    let url = reqwest::Url::parse(trimmed).map_err(|_| "Webhook 地址格式无效".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Webhook 地址仅支持 http:// 或 https://".to_string());
    }
    Ok(trimmed.to_string())
}

/// 校验并按提供商规整 Webhook URL（Bark 去除非根路径末尾 `/` 以兼容严格反向代理）。
pub(crate) fn normalize_webhook_url(
    provider: NotificationProvider,
    raw: &str,
) -> Result<String, String> {
    let validated = validate_webhook_url(raw)?;
    if provider == NotificationProvider::Bark {
        if let Ok(mut parsed) = reqwest::Url::parse(&validated) {
            let path = parsed.path().to_string();
            if path.len() > 1 && path.ends_with('/') {
                let trimmed_path = path.trim_end_matches('/');
                parsed.set_path(if trimmed_path.is_empty() {
                    "/"
                } else {
                    trimmed_path
                });
                return Ok(parsed.into());
            }
        }
    }
    Ok(validated)
}

/// 校验 HTTP 2xx 响应中的平台业务状态码（飞书 / 企业微信 / 钉钉 / Bark 常用 200 包裹业务错误）。
pub(crate) fn check_provider_response(
    provider: NotificationProvider,
    body: &str,
) -> Result<(), String> {
    if provider == NotificationProvider::Generic {
        return Ok(());
    }
    let Ok(json) = serde_json::from_str::<Value>(body) else {
        return Ok(());
    };
    let Some(obj) = json.as_object() else {
        return Ok(());
    };

    match provider {
        NotificationProvider::Feishu => {
            if let Some(code) = obj.get("code").and_then(Value::as_i64) {
                if code != 0 {
                    let detail = obj
                        .get("msg")
                        .or_else(|| obj.get("StatusMessage"))
                        .and_then(Value::as_str)
                        .unwrap_or("未知错误");
                    return Err(format!("飞书 Webhook 返回业务错误（code={code}）：{detail}"));
                }
            }
            if let Some(status_code) = obj.get("StatusCode").and_then(Value::as_i64) {
                if status_code != 0 {
                    let detail = obj
                        .get("StatusMessage")
                        .or_else(|| obj.get("msg"))
                        .and_then(Value::as_str)
                        .unwrap_or("未知错误");
                    return Err(format!(
                        "飞书 Webhook 返回业务错误（StatusCode={status_code}）：{detail}"
                    ));
                }
            }
        }
        NotificationProvider::Wecom | NotificationProvider::Dingtalk => {
            if let Some(errcode) = obj.get("errcode").and_then(Value::as_i64) {
                if errcode != 0 {
                    let label = match provider {
                        NotificationProvider::Wecom => "企业微信",
                        NotificationProvider::Dingtalk => "钉钉",
                        _ => "Webhook",
                    };
                    let detail = obj
                        .get("errmsg")
                        .and_then(Value::as_str)
                        .unwrap_or("未知错误");
                    return Err(format!(
                        "{label} Webhook 返回业务错误（errcode={errcode}）：{detail}"
                    ));
                }
            }
        }
        NotificationProvider::Bark => {
            if let Some(code) = obj.get("code").and_then(Value::as_i64) {
                if code != 200 {
                    let detail = obj
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("未知错误");
                    return Err(format!("Bark 返回业务错误（code={code}）：{detail}"));
                }
            }
        }
        NotificationProvider::Generic => {}
    }

    Ok(())
}

/// 异步发送一条 Webhook 通知并返回可读结果（测试按钮用）。
pub async fn send_webhook(
    provider: NotificationProvider,
    webhook_url: &str,
    event: NotificationEvent,
    title: &str,
    body: &str,
) -> Result<(), String> {
    let url = normalize_webhook_url(provider, webhook_url)?;
    let payload = build_webhook_payload(provider, event, title, body);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| format!("初始化网络客户端失败：{error}"))?;
    let response = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("发送通知失败：{error}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        let snippet: String = detail.chars().take(120).collect();
        return Err(format!("通知接口返回错误状态 {status}：{snippet}"));
    }
    let response_body = response.text().await.unwrap_or_default();
    check_provider_response(provider, &response_body)?;
    Ok(())
}

/// 在后台线程中非阻塞地触发通知；失败只打警告日志，绝不阻断求职主流程。
pub fn notify_async(
    config: &NotificationConfig,
    event: NotificationEvent,
    title: impl Into<String>,
    body: impl Into<String>,
) {
    if !should_send(config, event) {
        return;
    }
    let provider = config.provider;
    let url = config.webhook_url.clone();
    let title = title.into();
    let body = body.into();
    let log_ctx = crate::logger::current_context();

    std::thread::spawn(move || {
        let _log_guard = crate::logger::scoped_context(log_ctx.task_id, log_ctx.platform);
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        if let Err(error) = runtime.block_on(send_webhook(provider, &url, event, &title, &body)) {
            let _ = crate::logger::warning(format!("推送实时通知失败：{error}"));
        }
    });
}

thread_local! {
    static CURRENT_NOTIFICATION: std::cell::RefCell<Option<NotificationConfig>> =
        const { std::cell::RefCell::new(None) };
    static LAST_RISK_ALERT_AT: std::cell::Cell<Option<Instant>> =
        const { std::cell::Cell::new(None) };
}

pub struct NotificationGuard {
    previous: Option<NotificationConfig>,
}

impl Drop for NotificationGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT_NOTIFICATION.with(|slot| *slot.borrow_mut() = previous);
        LAST_RISK_ALERT_AT.with(|slot| slot.set(None));
    }
}

/// 在当前任务工作线程上安装实时通知配置，离开作用域后自动摘除（与 logger/humanize 一致）。
#[must_use]
pub fn scoped_notification(config: NotificationConfig) -> NotificationGuard {
    let previous = CURRENT_NOTIFICATION.with(|slot| slot.replace(Some(config)));
    LAST_RISK_ALERT_AT.with(|slot| slot.set(None));
    NotificationGuard { previous }
}

/// 检查当前工作线程是否在最近 `window` 内已经推送过风控告警，避免同一中断事件重复触发两次 Webhook。
pub(crate) fn risk_alert_sent_recently(window: Duration) -> bool {
    LAST_RISK_ALERT_AT.with(|slot| {
        slot.get()
            .is_some_and(|at| at.elapsed() <= window)
    })
}

/// 使用当前线程绑定的通知配置发送通知；未绑定时（如单测环境）静默跳过。
pub fn notify_current(
    event: NotificationEvent,
    title: impl Into<String>,
    body: impl Into<String>,
) {
    if event == NotificationEvent::RiskAlert {
        LAST_RISK_ALERT_AT.with(|slot| slot.set(Some(Instant::now())));
    }
    let maybe_config = CURRENT_NOTIFICATION.with(|slot| slot.borrow().clone());
    if let Some(config) = maybe_config {
        notify_async(&config, event, title, body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_or_blank_config_never_sends() {
        let mut config = NotificationConfig::default();
        assert!(!should_send(&config, NotificationEvent::ManualReview));

        config.enabled = true;
        assert!(!should_send(&config, NotificationEvent::ManualReview));

        config.webhook_url = "https://open.feishu.cn/open-apis/bot/v2/hook/test".to_string();
        assert!(should_send(&config, NotificationEvent::ManualReview));
        config.notify_manual_review = false;
        assert!(!should_send(&config, NotificationEvent::ManualReview));
        assert!(should_send(&config, NotificationEvent::RiskAlert));
    }

    #[test]
    fn builds_provider_specific_webhook_payloads() {
        let feishu = build_webhook_payload(
            NotificationProvider::Feishu,
            NotificationEvent::ManualReview,
            "Rust 后端 · 示例公司",
            "对方询问期望薪资",
        );
        assert_eq!(feishu["msg_type"], "text");
        assert!(feishu["content"]["text"]
            .as_str()
            .unwrap()
            .contains("待人工处理"));

        let wecom = build_webhook_payload(
            NotificationProvider::Wecom,
            NotificationEvent::RiskAlert,
            "BOSS 任务中断",
            "检测到安全验证",
        );
        assert_eq!(wecom["msgtype"], "text");
        assert!(wecom["text"]["content"]
            .as_str()
            .unwrap()
            .contains("检测到安全验证"));

        let bark = build_webhook_payload(
            NotificationProvider::Bark,
            NotificationEvent::HrReply,
            "收到新消息",
            "约个时间面试",
        );
        assert_eq!(bark["group"], "OfferFlow");
        assert_eq!(bark["body"], "约个时间面试");
    }

    #[test]
    fn rejects_non_http_webhook_urls() {
        assert!(validate_webhook_url("").is_err());
        assert!(validate_webhook_url("file:///etc/passwd").is_err());
        assert!(validate_webhook_url("https://example.com/hook").is_ok());
    }

    #[test]
    fn bark_falls_back_to_headline_when_body_is_empty() {
        let bark = build_webhook_payload(
            NotificationProvider::Bark,
            NotificationEvent::RiskAlert,
            "BOSS 检测到安全验证",
            "   ",
        );
        assert_eq!(bark["title"], "【OfferFlow · 风控/异常提醒】BOSS 检测到安全验证");
        assert_eq!(bark["body"], bark["title"]);
    }

    #[test]
    fn bark_trims_trailing_slash_on_non_root_path() {
        assert_eq!(
            normalize_webhook_url(NotificationProvider::Bark, "https://api.day.app/KEY/").unwrap(),
            "https://api.day.app/KEY"
        );
        assert_eq!(
            normalize_webhook_url(NotificationProvider::Bark, "https://api.day.app/KEY///?sound=alarm")
                .unwrap(),
            "https://api.day.app/KEY?sound=alarm"
        );
        assert_eq!(
            normalize_webhook_url(NotificationProvider::Bark, "https://api.day.app/").unwrap(),
            "https://api.day.app/"
        );
        assert_eq!(
            normalize_webhook_url(NotificationProvider::Feishu, "https://open.feishu.cn/hook/")
                .unwrap(),
            "https://open.feishu.cn/hook/"
        );
    }

    #[test]
    fn check_provider_response_rejects_http_200_business_errors_and_accepts_success() {
        // Feishu
        assert!(check_provider_response(
            NotificationProvider::Feishu,
            r#"{"code":0,"msg":"success"}"#
        )
        .is_ok());
        assert!(check_provider_response(
            NotificationProvider::Feishu,
            r#"{"StatusCode":0,"StatusMessage":"success"}"#
        )
        .is_ok());
        let feishu_err = check_provider_response(
            NotificationProvider::Feishu,
            r#"{"code":19024,"msg":"Key Words Not Found"}"#,
        )
        .unwrap_err();
        assert!(feishu_err.contains("19024"));
        assert!(feishu_err.contains("Key Words Not Found"));
        let feishu_status_err = check_provider_response(
            NotificationProvider::Feishu,
            r#"{"StatusCode":19021,"StatusMessage":"sign match fail"}"#,
        )
        .unwrap_err();
        assert!(feishu_status_err.contains("19021"));

        // WeCom
        assert!(check_provider_response(
            NotificationProvider::Wecom,
            r#"{"errcode":0,"errmsg":"ok"}"#
        )
        .is_ok());
        let wecom_err = check_provider_response(
            NotificationProvider::Wecom,
            r#"{"errcode":93000,"errmsg":"invalid webhook url"}"#,
        )
        .unwrap_err();
        assert!(wecom_err.contains("93000"));

        // DingTalk
        assert!(check_provider_response(
            NotificationProvider::Dingtalk,
            r#"{"errcode":0,"errmsg":"ok"}"#
        )
        .is_ok());
        let dingtalk_err = check_provider_response(
            NotificationProvider::Dingtalk,
            r#"{"errcode":310000,"errmsg":"keywords not in content"}"#,
        )
        .unwrap_err();
        assert!(dingtalk_err.contains("310000"));

        // Bark
        assert!(check_provider_response(
            NotificationProvider::Bark,
            r#"{"code":200,"message":"success","timestamp":1700000000}"#
        )
        .is_ok());
        let bark_err = check_provider_response(
            NotificationProvider::Bark,
            r#"{"code":400,"message":"failed to get device token"}"#,
        )
        .unwrap_err();
        assert!(bark_err.contains("400"));

        // Generic always accepts 2xx
        assert!(check_provider_response(
            NotificationProvider::Generic,
            r#"{"code":500,"errcode":99}"#
        )
        .is_ok());
    }
}
