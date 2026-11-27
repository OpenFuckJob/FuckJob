use crate::command::base::CommandResult;
use crate::config::NotificationProvider;
use crate::notification::{send_webhook, NotificationEvent};

#[tauri::command]
pub async fn test_notification_webhook(
    provider: NotificationProvider,
    webhook_url: String,
) -> CommandResult<()> {
    match send_webhook(
        provider,
        &webhook_url,
        NotificationEvent::Test,
        "OfferFlow 通知连通性测试",
        "如果你看到这条消息，说明实时推送配置已生效。后续在「待你处理」、风控提醒或收到 HR 回复时会自动通知你。",
    )
    .await
    {
        Ok(()) => CommandResult::ok(()),
        Err(error) => CommandResult::err(error),
    }
}
