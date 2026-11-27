//! 可选的实时 Webhook 通知模块（飞书 / 企业微信 / 钉钉 / Bark / 自定义 JSON）。
//!
//! 隐私边界：默认完全关闭；仅在用户主动启用并填写目标 Webhook URL 后，
//! 才向用户显式配置的地址发送轻量通知（待人工处理 / 风控中断 / 收到 HR 回复）。

use std::time::Duration;

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
        NotificationProvider::Bark => json!({
            "title": headline,
            "body": body.trim(),
            "group": "OfferFlow"
        }),
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

/// 异步发送一条 Webhook 通知并返回可读结果（测试按钮用）。
pub async fn send_webhook(
    provider: NotificationProvider,
    webhook_url: &str,
    event: NotificationEvent,
    title: &str,
    body: &str,
) -> Result<(), String> {
    let url = validate_webhook_url(webhook_url)?;
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

    std::thread::spawn(move || {
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
}

pub struct NotificationGuard {
    previous: Option<NotificationConfig>,
}

impl Drop for NotificationGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT_NOTIFICATION.with(|slot| *slot.borrow_mut() = previous);
    }
}

/// 在当前任务工作线程上安装实时通知配置，离开作用域后自动摘除（与 logger/humanize 一致）。
#[must_use]
pub fn scoped_notification(config: NotificationConfig) -> NotificationGuard {
    let previous = CURRENT_NOTIFICATION.with(|slot| slot.replace(Some(config)));
    NotificationGuard { previous }
}

/// 使用当前线程绑定的通知配置发送通知；未绑定时（如单测环境）静默跳过。
pub fn notify_current(
    event: NotificationEvent,
    title: impl Into<String>,
    body: impl Into<String>,
) {
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
}
