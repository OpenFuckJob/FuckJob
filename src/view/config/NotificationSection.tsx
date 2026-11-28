import { useState } from "react";
import { Alert, Button, Checkbox, Input, Select, Space, Typography } from "antd";
import { BellOutlined, SendOutlined } from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import { commandErrorMessage, type CommandResult } from "@/types/command";
import type { NotificationConfig, NotificationProvider } from "@/types/app-config";
import { SettingGroup, SettingToggle } from "@/components/SettingField";

const { Text } = Typography;

const PROVIDER_OPTIONS: Array<{ value: NotificationProvider; label: string; placeholder: string }> = [
  {
    value: "feishu",
    label: "飞书机器人 Webhook",
    placeholder: "https://open.feishu.cn/open-apis/bot/v2/hook/...",
  },
  {
    value: "wecom",
    label: "企业微信机器人 Webhook",
    placeholder: "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=...",
  },
  {
    value: "dingtalk",
    label: "钉钉机器人 Webhook",
    placeholder: "https://oapi.dingtalk.com/robot/send?access_token=...",
  },
  {
    value: "bark",
    label: "Bark (iOS 推送)",
    placeholder: "https://api.day.app/<你的设备Key>/",
  },
  {
    value: "generic",
    label: "自定义 JSON Webhook",
    placeholder: "https://your-server.example.com/webhook",
  },
];

interface Props {
  config: NotificationConfig;
  onChange: (next: Partial<NotificationConfig>) => void;
}

export default function NotificationSection({ config, onChange }: Props) {
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ type: "success" | "error"; text: string } | null>(null);
  const currentProvider = PROVIDER_OPTIONS.find((item) => item.value === config.provider) ?? PROVIDER_OPTIONS[0];

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const result = await invoke<CommandResult<void>>("test_notification_webhook", {
        provider: config.provider,
        webhookUrl: config.webhook_url,
      });
      if (!result.success) {
        setTestResult({
          type: "error",
          text: commandErrorMessage(result.error, "测试通知发送失败"),
        });
        return;
      }
      setTestResult({
        type: "success",
        text: "测试通知已发送，请在对应客户端检查是否收到消息。",
      });
    } catch (error) {
      setTestResult({
        type: "error",
        text: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="space-y-4 rounded-2xl border border-[var(--of-border)] bg-[var(--of-surface)] p-6">
      <div>
        <Text className="block font-bold text-[var(--of-text)]">实时消息提醒（可选 Webhook）</Text>
        <Text className="text-xs text-[var(--of-text-secondary)]">
          挂机投递时不必守在电脑前：遇到待人工接手、风控滑块中断或 HR 回复，直接推送到飞书 / 企业微信 / 钉钉 / Bark。默认完全关闭。
        </Text>
      </div>

      <SettingGroup>
        <SettingToggle
          icon={<BellOutlined />}
          title="启用实时 Webhook 推送"
          description="仅在你开启并填写地址后，才向你指定的 Webhook 发送轻量事件摘要，绝不上传简历或完整聊天记录"
          checked={config.enabled}
          onChange={(enabled) => onChange({ enabled })}
        >
          {config.enabled && (
            <div className="space-y-4 pt-1">
              <div className="grid grid-cols-1 gap-3 md:grid-cols-[220px_minmax(0,1fr)]">
                <div>
                  <Text className="mb-1 block text-xs font-medium text-[var(--of-text-secondary)]">推送渠道</Text>
                  <Select<NotificationProvider>
                    className="w-full"
                    value={config.provider}
                    options={PROVIDER_OPTIONS.map(({ value, label }) => ({ value, label }))}
                    onChange={(provider) => {
                      setTestResult(null);
                      onChange({ provider });
                    }}
                  />
                </div>
                <div>
                  <Text className="mb-1 block text-xs font-medium text-[var(--of-text-secondary)]">Webhook 地址</Text>
                  <Space.Compact className="w-full">
                    <Input
                      value={config.webhook_url}
                      placeholder={currentProvider.placeholder}
                      onChange={(event) => {
                        setTestResult(null);
                        onChange({ webhook_url: event.target.value });
                      }}
                    />
                    <Button
                      icon={<SendOutlined />}
                      loading={testing}
                      disabled={!config.webhook_url.trim()}
                      onClick={() => void handleTest()}
                    >
                      发送测试
                    </Button>
                  </Space.Compact>
                </div>
              </div>

              {config.provider === "dingtalk" && (
                <Text type="secondary" className="block text-xs">
                  提示：钉钉机器人若开启「自定义关键词」安全设置，请添加关键词{" "}
                  <Text code>OfferFlow</Text>（推送消息默认包含该词）。
                </Text>
              )}

              <div>
                <Text className="mb-2 block text-xs font-medium text-[var(--of-text-secondary)]">触发事件</Text>
                <Space wrap size={16}>
                  <Checkbox
                    checked={config.notify_manual_review}
                    onChange={(event) => onChange({ notify_manual_review: event.target.checked })}
                  >
                    会话转人工（如索要微信/问薪资/预约面试）
                  </Checkbox>
                  <Checkbox
                    checked={config.notify_risk_alert}
                    onChange={(event) => onChange({ notify_risk_alert: event.target.checked })}
                  >
                    风控熔断或任务异常中断
                  </Checkbox>
                  <Checkbox
                    checked={config.notify_hr_reply}
                    onChange={(event) => onChange({ notify_hr_reply: event.target.checked })}
                  >
                    收到新的 HR 回复
                  </Checkbox>
                </Space>
              </div>

              {testResult && (
                <Alert
                  type={testResult.type}
                  showIcon
                  message={testResult.text}
                />
              )}
            </div>
          )}
        </SettingToggle>
      </SettingGroup>
    </div>
  );
}
