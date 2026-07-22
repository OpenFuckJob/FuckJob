import { useEffect, useMemo, useState } from "react";
import { Alert, Button, Card, Form, Input, Select, Space, Typography } from "antd";
import type { LlmConfig, LlmProviderPreset } from "@/types/app-config";
import { clearLlmApiKey, getLlmCredentialStatus, setLlmApiKey, testLlmConnection } from "@/lib/llmConfig";
import type { CommandError, CommandResult } from "@/types/command";
import type { LlmConnectionReport, LlmCredentialStatus } from "@/types/llm";

export const LLM_PRESETS: Record<LlmProviderPreset, { label: string; baseUrl: string; requiresKey: boolean }> = {
  ollama: { label: "Ollama", baseUrl: "http://127.0.0.1:11434/v1", requiresKey: false },
  lm_studio: { label: "LM Studio", baseUrl: "http://127.0.0.1:1234/v1", requiresKey: false },
  openai: { label: "OpenAI", baseUrl: "https://api.openai.com/v1", requiresKey: true },
  deepseek: { label: "DeepSeek", baseUrl: "https://api.deepseek.com", requiresKey: true },
  dashscope: { label: "DashScope", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", requiresKey: true },
  custom: { label: "自定义", baseUrl: "", requiresKey: false },
};

const resultError = (error: CommandError | null, fallback: string) => error ? `[${error.code}] ${error.message}` : fallback;
export const isValidLlmConfig = (value: LlmConfig | null) => Boolean(value?.base_url.trim() && value.model.trim());

export interface LlmConfigPanelProps {
  config: LlmConfig | null;
  onChange: (config: LlmConfig | null) => void;
  onPersist?: (config: LlmConfig | null) => Promise<boolean>;
  compact?: boolean;
}

export function LlmConfigPanel({ config, onChange, onPersist }: LlmConfigPanelProps) {
  const [credential, setCredential] = useState<LlmCredentialStatus | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [feedback, setFeedback] = useState<{ type: "success" | "error" | "info"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const preset = config ? LLM_PRESETS[config.provider] : null;

  useEffect(() => { getLlmCredentialStatus().then((r) => r.success && r.data && setCredential(r.data)).catch(() => setFeedback({ type: "info", text: "无法读取凭据状态；可继续配置本地服务。" })); }, []);

  const options = useMemo(() => Object.entries(LLM_PRESETS).map(([value, item]) => ({ value, label: item.label })), []);
  const choose = (provider: LlmProviderPreset) => onChange({ provider, base_url: LLM_PRESETS[provider].baseUrl, model: "" });
  const patch = (next: Partial<LlmConfig>) => config && onChange({ ...config, ...next });

  const storeKey = async () => {
    if (!apiKey.trim()) return setFeedback({ type: "error", text: "请输入 API Key" });
    const result = await setLlmApiKey(apiKey.trim());
    if (result.success && result.data) { setCredential(result.data); setApiKey(""); setFeedback({ type: "success", text: "凭据已安全保存" }); }
    else setFeedback({ type: "error", text: resultError(result.error, "保存凭据失败") });
  };
  const clearKey = async () => {
    const result = await clearLlmApiKey();
    if (result.success && result.data) { setCredential(result.data); setFeedback({ type: "success", text: "凭据已清除" }); }
    else setFeedback({ type: "error", text: resultError(result.error, "清除凭据失败") });
  };

  const prepare = async () => {
    if (!isValidLlmConfig(config)) { setFeedback({ type: "error", text: "请检查服务地址和模型名称" }); return false; }
    if (preset?.requiresKey && !credential?.configured) { setFeedback({ type: "error", text: "该服务需要先配置 API Key" }); return false; }
    return onPersist ? onPersist(config) : true;
  };

  const test = async () => {
    setBusy(true);
    try {
      if (!await prepare()) return;
      const result: CommandResult<LlmConnectionReport> = await testLlmConnection();
      if (result.success && result.data) setFeedback({ type: "success", text: `短连接测试成功 · 模型 ${result.data.model}${result.data.latency_ms ? ` · ${result.data.latency_ms}ms` : ""} · 响应: ${result.data.response}` });
      else setFeedback({ type: "error", text: resultError(result.error, "连接测试失败") });
    } catch (error) { setFeedback({ type: "error", text: error instanceof Error ? error.message : "连接测试失败" }); } finally { setBusy(false); }
  };

  return <Card title="大模型配置">
    <Form layout="vertical">
      <Form.Item label="服务预设">
        <Select aria-label="服务预设" value={config?.provider} placeholder="选择服务后开始配置" options={options} onChange={choose} />
      </Form.Item>
      {config && <>
        <Form.Item label="服务地址" validateStatus={config.base_url.trim() ? undefined : "error"}>
          <Input value={config.base_url} placeholder="OpenAI 兼容 API 地址" onChange={(e) => patch({ base_url: e.target.value })} />
        </Form.Item>
        <Form.Item label="模型">
          <Input value={config.model} placeholder="手动输入模型名称" onChange={(e) => patch({ model: e.target.value })} />
        </Form.Item>
        <Typography.Text type="secondary">凭据状态：{credential?.configured ? `已配置（${credential.source}）` : "未配置"}。应用不会读取或显示明文。</Typography.Text>
        {config.provider === "custom" && !credential?.configured && (
          <Alert type="info" showIcon style={{ marginTop: 8 }} message="自定义 API 端点通常需要 API Key。如果连接测试返回 401 或鉴权失败，请先在下方设置凭据。" />
        )}
        <Space.Compact block style={{ marginTop: 12 }}>
          <Input.Password value={apiKey} placeholder="设置或替换 API Key" onChange={(e) => setApiKey(e.target.value)} />
          <Button onClick={() => void storeKey()}>保存凭据</Button>
          <Button danger disabled={!credential?.configured} onClick={() => void clearKey()}>清除</Button>
        </Space.Compact>
        <Space wrap style={{ marginTop: 16 }}>
          <Button loading={busy} type="primary" onClick={() => void test()}>短连接测试</Button>
          <Button danger onClick={() => onChange(null)}>停用 AI</Button>
        </Space>
      </>}
      {feedback && <Alert style={{ marginTop: 16 }} type={feedback.type} showIcon message={feedback.text} />}
    </Form>
  </Card>;
}
