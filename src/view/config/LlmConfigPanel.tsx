import { useEffect, useMemo, useState } from "react";
import { Alert, AutoComplete, Button, Card, Form, Input, Select, Space, Typography } from "antd";
import type { LlmConfig, LlmProviderPreset } from "@/types/app-config";
import { clearLlmApiKey, getLlmCredentialStatus, listLlmModels, setLlmApiKey, testLlmConnection } from "@/lib/llmConfig";
import type { CommandError, CommandResult } from "@/types/command";
import type { LlmConnectionReport, LlmCredentialStatus } from "@/types/llm";

export const LLM_PRESETS: Record<LlmProviderPreset, { label: string; baseUrl: string; requiresKey: boolean }> = {
  anthropic: { label: "Anthropic", baseUrl: "https://api.anthropic.com", requiresKey: true },
  deepseek: { label: "DeepSeek", baseUrl: "https://api.deepseek.com", requiresKey: true },
  openai: { label: "OpenAI (Completions)", baseUrl: "https://api.openai.com/v1", requiresKey: true },
  openai_responses: { label: "OpenAI (Responses)", baseUrl: "https://api.openai.com/v1", requiresKey: true },
  minimax: { label: "MiniMax", baseUrl: "https://api.minimax.io/v1", requiresKey: true },
  moonshot: { label: "Moonshot", baseUrl: "https://api.moonshot.ai/v1", requiresKey: true },
  ollama: { label: "Ollama", baseUrl: "http://127.0.0.1:11434", requiresKey: false },
  openrouter: { label: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", requiresKey: true },
  xiaomi_mimo: { label: "Xiaomi MiMo", baseUrl: "https://api.xiaomimimo.com/v1", requiresKey: true },
  zai: { label: "Z.ai", baseUrl: "https://api.z.ai/api/paas/v4", requiresKey: true },
};

const resultError = (error: CommandError | null, fallback: string) => error ? `[${error.code}] ${error.message}` : fallback;
export const isValidLlmConfig = (value: LlmConfig | null) => Boolean(value?.base_url.trim() && value.model.trim());
export const shouldFetchLlmModels = (
  config: LlmConfig | null,
  credentialConfigured: boolean,
  userRequested: boolean,
) => Boolean(
  userRequested &&
    config?.base_url.trim() &&
    (!LLM_PRESETS[config.provider].requiresKey || credentialConfigured),
);

export interface LlmConfigPanelProps {
  config: LlmConfig | null;
  onChange: (config: LlmConfig | null) => void;
  onPersist?: (config: LlmConfig | null) => Promise<boolean>;
  onPendingApiKeyChange?: (apiKey: string) => void;
  compact?: boolean;
}

export function LlmConfigPanel({ config, onChange, onPersist, onPendingApiKeyChange }: LlmConfigPanelProps) {
  const [credential, setCredential] = useState<LlmCredentialStatus | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [feedback, setFeedback] = useState<{ type: "success" | "error" | "info"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const preset = config ? LLM_PRESETS[config.provider] : null;

  useEffect(() => { getLlmCredentialStatus().then((r) => r.success && r.data && setCredential(r.data)).catch(() => setFeedback({ type: "info", text: "无法读取凭据状态；可继续配置本地服务。" })); }, []);

  const options = useMemo(() => Object.entries(LLM_PRESETS).map(([value, item]) => ({ value, label: item.label })), []);
  const choose = (provider: LlmProviderPreset) => onChange({ provider, base_url: LLM_PRESETS[provider].baseUrl, model: "" });
  const patch = (next: Partial<LlmConfig>) => config && onChange({ ...config, ...next });
  const changeApiKey = (value: string) => {
    setApiKey(value);
    onPendingApiKeyChange?.(value);
  };

  const fetchModels = async (showSuccess = false) => {
    if (!config?.base_url.trim()) {
      setFeedback({ type: "error", text: "请先填写服务地址" });
      return;
    }
    if (!shouldFetchLlmModels(config, Boolean(credential?.configured), true)) {
      setFeedback({ type: "info", text: "请先保存 API Key，再展开模型列表获取模型" });
      return;
    }
    setModelsLoading(true);
    try {
      const result = await listLlmModels({
        provider: config.provider,
        base_url: config.base_url.trim(),
      });
      if (result.success && result.data) {
        setModels(result.data);
        if (!config.model.trim() && result.data[0]) patch({ model: result.data[0] });
        if (showSuccess) setFeedback({ type: "success", text: `已获取 ${result.data.length} 个模型` });
      } else {
        setModels([]);
        setFeedback({ type: "error", text: resultError(result.error, "获取模型列表失败") });
      }
    } catch (error) {
      setModels([]);
      setFeedback({ type: "error", text: error instanceof Error ? error.message : "获取模型列表失败" });
    } finally {
      setModelsLoading(false);
    }
  };

  useEffect(() => {
    setModels([]);
  }, [config?.base_url, config?.provider]);

  const storeKey = async () => {
    if (!apiKey.trim()) return setFeedback({ type: "error", text: "请输入 API Key" });
    const result = await setLlmApiKey(apiKey.trim());
    if (result.success && result.data) { setCredential(result.data); changeApiKey(""); setFeedback({ type: "success", text: "凭据已安全保存，可展开模型列表获取模型" }); }
    else setFeedback({ type: "error", text: resultError(result.error, "保存凭据失败") });
  };
  const clearKey = async () => {
    const result = await clearLlmApiKey();
    if (result.success && result.data) { setCredential(result.data); changeApiKey(""); setFeedback({ type: "success", text: "凭据已清除" }); }
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
          <Input value={config.base_url} placeholder="服务 API 地址，可按需自定义 BaseURL" onChange={(e) => patch({ base_url: e.target.value })} />
        </Form.Item>
        <Form.Item label="模型">
          <AutoComplete
            value={config.model}
            options={models.map((model) => ({ value: model, label: model }))}
            placeholder="自动获取或手动输入模型名称"
            onChange={(model) => patch({ model })}
            onOpenChange={(open) => {
              if (open && !modelsLoading && models.length === 0) void fetchModels();
            }}
            filterOption={(input, option) => String(option?.value ?? "").toLowerCase().includes(input.toLowerCase())}
            style={{ width: "100%" }}
          />
          <Button style={{ marginTop: 8 }} loading={modelsLoading} onClick={() => void fetchModels(true)}>刷新模型列表</Button>
        </Form.Item>
        <Typography.Text type="secondary">凭据状态：{credential?.configured ? `已配置（${credential.source}）` : "未配置"}。应用不会读取或显示明文。</Typography.Text>
        <Space.Compact block style={{ marginTop: 12 }}>
          <Input.Password value={apiKey} placeholder="设置或替换 API Key" onChange={(e) => changeApiKey(e.target.value)} />
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
