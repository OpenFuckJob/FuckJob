import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Alert, Button, Card, Space, Typography } from "antd";
import type { AppRuntimeConfig } from "@/types/app-config";
import type { BrowserEnvStatus } from "@/types/rpa";
import type { CommandResult } from "@/types/command";
import { LlmConfigPanel } from "@/view/config/LlmConfigPanel";

export function Onboarding({ config, onFinish }: { config: AppRuntimeConfig; onFinish: (config: AppRuntimeConfig) => Promise<boolean> }) {
  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState(config);
  const [browser, setBrowser] = useState<BrowserEnvStatus | null>(null);
  const [browserError, setBrowserError] = useState("");

  useEffect(() => {
    if (step !== 1) return;
    invoke<CommandResult<BrowserEnvStatus>>("check_browser_env").then((result) => {
      if (result.success && result.data) setBrowser(result.data); else setBrowserError(result.error?.message ?? "浏览器检测失败");
    }).catch(() => setBrowserError("浏览器检测失败，可稍后在配置中心处理"));
  }, [step]);

  const finish = (llm = draft.llm_config) => onFinish({ ...draft, onboarding_completed: true, llm_config: llm });
  return <main className="app-loading"><Card style={{ width: 720, maxWidth: "94vw" }}>
    <Typography.Text type="secondary">步骤 {step + 1} / 3 · {['隐私', '浏览器', '可选 AI'][step]}</Typography.Text>
    {step === 0 && <Space direction="vertical" size="large" style={{ width: "100%", marginTop: 32 }}>
      <Typography.Title level={2}>欢迎使用 OfferFlow</Typography.Title>
      <Alert type="info" showIcon message="数据默认保存在本机" description="招聘平台访问只在你主动执行任务时发生；大模型网络请求完全可选，并发送到你选择的服务。API Key 存入系统凭据库。" />
      <Space><Button type="primary" onClick={() => setStep(1)}>继续</Button><Button onClick={() => void finish(null)}>跳过 AI，进入应用</Button></Space>
    </Space>}
    {step === 1 && <Space direction="vertical" size="large" style={{ width: "100%", marginTop: 32 }}>
      <Typography.Title level={3}>浏览器环境</Typography.Title>
      {browser ? <Alert type={browser.browser_found && browser.user_data_dir_ok ? "success" : "warning"} message={browser.browser_found ? `已检测到 ${browser.browser_name ?? "浏览器"}` : "未自动检测到 Chrome 或 Edge"} description={browser.browser_path ?? "可稍后在配置中心手动指定路径"} /> : browserError ? <Alert type="warning" message={browserError} /> : <Typography.Text>正在检测浏览器…</Typography.Text>}
      <Button type="primary" onClick={() => setStep(2)}>继续配置</Button>
    </Space>}
    {step === 2 && <Space direction="vertical" size="large" style={{ width: "100%", marginTop: 32 }}>
      <Alert type="info" message="AI 配置可随时跳过" description="选择六种预设之一，填写模型并完成真实连接测试；失败不会阻止使用本地功能。" />
      <LlmConfigPanel config={draft.llm_config} onChange={(llm_config) => setDraft((v) => ({ ...v, llm_config }))} onPersist={async (llm_config) => onFinish({ ...draft, onboarding_completed: false, llm_config })} compact />
      <Space><Button type="primary" disabled={!draft.llm_config} onClick={() => void finish()}>完成并进入应用</Button><Button onClick={() => void finish(null)}>跳过 AI，进入应用</Button></Space>
    </Space>}
  </Card></main>;
}
