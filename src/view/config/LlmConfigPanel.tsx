import { useEffect, useMemo, useState } from "react";
import { Alert, AutoComplete, Button, Card, Collapse, Divider, Form, Input, Modal, Select, Space, Switch, Tag, Typography } from "antd";
import { ArrowDownOutlined, ArrowUpOutlined, DeleteOutlined, PlusOutlined } from "@ant-design/icons";
import {
  MAX_NETWORK_RETRY_ATTEMPTS,
  MAX_RETRY_BASE_DELAY_MS,
  MIN_RETRY_BASE_DELAY_MS,
  PRIMARY_LLM_ENTRY_ID,
  type LlmConfig,
  type LlmProviderEntry,
  type LlmProviderPreset,
  type LlmRetryConfig,
} from "@/types/app-config";
import { NumberField } from "@/components/NumberField";
import {
  clearLlmApiKey,
  clearLlmApiKeyFor,
  getLlmCredentialStatus,
  listLlmCredentialStatus,
  listLlmModels,
  listLlmModelsFor,
  setLlmApiKey,
  setLlmApiKeyFor,
  swapLlmCredentials,
  testLlmConnection,
  testLlmEntryConnection,
} from "@/lib/llmConfig";
import type { CommandError } from "@/types/command";
import type { LlmCredentialStatus } from "@/types/llm";

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

/** 一条降级链条目在界面上需要的最小字段，主用服务与备用服务共用同一套编辑控件 */
type LlmServiceFields = Pick<LlmConfig, "provider" | "base_url" | "model">;

const resultError = (error: CommandError | null, fallback: string) => error ? `[${error.code}] ${error.message}` : fallback;
export const isValidLlmConfig = (value: LlmServiceFields | null) => Boolean(value?.base_url.trim() && value.model.trim());
export const shouldFetchLlmModels = (
  config: LlmServiceFields | null,
  credentialConfigured: boolean,
  userRequested: boolean,
) => Boolean(
  userRequested &&
    config?.base_url.trim() &&
    (!LLM_PRESETS[config.provider].requiresKey || credentialConfigured),
);

/**
 * 条目标识会被拼进系统凭据库的条目名，Rust 侧只接受字母、数字、下划线和连字符，
 * 且不允许等于主用服务的保留标识。randomUUID 满足该约束，缺失时退回同样安全的字符集。
 */
export const createLlmEntryId = (): string => {
  // UUID 的形状注定不会等于 "primary"，无需再排除保留标识
  const uuid = globalThis.crypto?.randomUUID?.();
  if (uuid) return uuid;
  return `llm-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
};

export const createFallbackEntry = (provider: LlmProviderPreset): LlmProviderEntry => ({
  id: createLlmEntryId(),
  label: null,
  provider,
  base_url: LLM_PRESETS[provider].baseUrl,
  model: "",
  enabled: true,
});

/** 备用服务之间的重排：纯数组移动，不涉及任何密钥归属变化 */
export const moveFallback = <T,>(items: T[], from: number, to: number): T[] => {
  if (from === to || from < 0 || to < 0 || from >= items.length || to >= items.length) return items;
  const next = [...items];
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
};

/**
 * 把某个备用服务提升为主用服务。
 *
 * 被降级的原主用服务沿用被提升条目的标识，两个服务恰好构成一次位置对调：
 * 主用槽位与该备用槽位互换内容。密钥同样按这两个标识对调（由后端的
 * swap_llm_credentials 完成），因此不存在密钥错配，也不需要用户重填。
 */
export const promoteFallbackToPrimary = (
  primary: LlmConfig,
  fallbacks: LlmProviderEntry[],
  index: number,
): { primary: LlmConfig; fallbacks: LlmProviderEntry[]; swappedId: string } | null => {
  const target = fallbacks[index];
  if (!target) return null;
  const demoted: LlmProviderEntry = {
    id: target.id,
    label: null,
    provider: primary.provider,
    base_url: primary.base_url,
    model: primary.model,
    enabled: true,
  };
  return {
    primary: { provider: target.provider, base_url: target.base_url, model: target.model },
    fallbacks: fallbacks.map((item, i) => (i === index ? demoted : item)),
    swappedId: target.id,
  };
};

export const clampRetryAttempts = (value: number | null | undefined) =>
  Math.min(Math.max(Math.round(value ?? 0), 0), MAX_NETWORK_RETRY_ATTEMPTS);
export const clampRetryBaseDelay = (value: number | null | undefined) =>
  Math.min(Math.max(Math.round(value ?? MIN_RETRY_BASE_DELAY_MS), MIN_RETRY_BASE_DELAY_MS), MAX_RETRY_BASE_DELAY_MS);

const fallbackTitle = (entry: LlmProviderEntry, index: number) =>
  entry.label?.trim() || entry.model.trim() || `备用 ${index + 1}`;

const credentialSourceText: Record<LlmCredentialStatus["source"], string> = {
  keychain: "系统凭据库",
  environment: "环境变量",
  none: "未配置",
};

export interface LlmConfigPanelProps {
  config: LlmConfig | null;
  onChange: (config: LlmConfig | null) => void;
  onPersist?: (config: LlmConfig | null) => Promise<boolean>;
  onPendingApiKeyChange?: (apiKey: string) => void;
  compact?: boolean;
  /** 降级链中的备用服务，按数组顺序生效 */
  fallbacks?: LlmProviderEntry[];
  onFallbacksChange?: (next: LlmProviderEntry[]) => void;
  retryConfig?: LlmRetryConfig;
  onRetryConfigChange?: (next: LlmRetryConfig) => void;
  /** 整份配置是否存在未保存的改动，用于阻止对着草稿做连接测试 */
  dirty?: boolean;
  /** 保存整份配置，测试备用服务前必须先落盘 */
  onPersistAll?: () => Promise<boolean>;
}

export function LlmConfigPanel({
  config,
  onChange,
  onPersist,
  onPendingApiKeyChange,
  compact,
  fallbacks,
  onFallbacksChange,
  retryConfig,
  onRetryConfigChange,
  dirty,
  onPersistAll,
}: LlmConfigPanelProps) {
  const [credentials, setCredentials] = useState<Record<string, LlmCredentialStatus>>({});
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});
  const [models, setModels] = useState<Record<string, string[]>>({});
  const [modelsLoadingId, setModelsLoadingId] = useState<string | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [activeKeys, setActiveKeys] = useState<string[]>([]);
  const [feedback, setFeedback] = useState<{ type: "success" | "error" | "info"; text: string } | null>(null);

  const chain = useMemo(() => fallbacks ?? [], [fallbacks]);
  const chainEnabled = Boolean(config && onFallbacksChange && !compact);
  const entryIdsKey = useMemo(
    () => [PRIMARY_LLM_ENTRY_ID, ...(chainEnabled ? chain.map((entry) => entry.id) : [])].join("\n"),
    [chain, chainEnabled],
  );

  useEffect(() => {
    let cancelled = false;
    const fallbackToPrimaryOnly = async () => {
      try {
        const single = await getLlmCredentialStatus();
        if (cancelled) return;
        if (single.success && single.data) setCredentials({ [PRIMARY_LLM_ENTRY_ID]: single.data });
        else setFeedback({ type: "info", text: "无法读取凭据状态；可继续配置本地服务。" });
      } catch {
        if (!cancelled) setFeedback({ type: "info", text: "无法读取凭据状态；可继续配置本地服务。" });
      }
    };
    // 一次性拉取整条链的凭据状态，避免每个服务各发一次请求
    listLlmCredentialStatus(entryIdsKey.split("\n"))
      .then((result) => {
        if (cancelled) return;
        if (!result.success || !Array.isArray(result.data)) return fallbackToPrimaryOnly();
        const next: Record<string, LlmCredentialStatus> = {};
        for (const item of result.data) next[item.entry_id] = { configured: item.configured, source: item.source };
        setCredentials(next);
      })
      .catch(() => (cancelled ? undefined : fallbackToPrimaryOnly()));
    return () => { cancelled = true; };
  }, [entryIdsKey]);

  const options = useMemo(() => Object.entries(LLM_PRESETS).map(([value, item]) => ({ value, label: item.label })), []);
  const serviceOf = (entryId: string): LlmServiceFields | null =>
    entryId === PRIMARY_LLM_ENTRY_ID ? config : chain.find((entry) => entry.id === entryId) ?? null;

  const clearModels = (entryId: string) => setModels((current) => ({ ...current, [entryId]: [] }));
  const patchEntry = (entryId: string, next: Partial<LlmServiceFields>) => {
    if (next.provider || next.base_url !== undefined) clearModels(entryId);
    if (entryId === PRIMARY_LLM_ENTRY_ID) {
      if (config) onChange({ ...config, ...next });
      return;
    }
    onFallbacksChange?.(chain.map((entry) => (entry.id === entryId ? { ...entry, ...next } : entry)));
  };
  const chooseProvider = (entryId: string, provider: LlmProviderPreset) =>
    patchEntry(entryId, { provider, base_url: LLM_PRESETS[provider].baseUrl, model: "" });

  const changeApiKey = (entryId: string, value: string) => {
    setApiKeys((current) => ({ ...current, [entryId]: value }));
    if (entryId === PRIMARY_LLM_ENTRY_ID) onPendingApiKeyChange?.(value);
  };

  const storeKey = async (entryId: string) => {
    const draft = (apiKeys[entryId] ?? "").trim();
    if (!draft) { setFeedback({ type: "error", text: "请输入 API Key" }); return; }
    const result = entryId === PRIMARY_LLM_ENTRY_ID ? await setLlmApiKey(draft) : await setLlmApiKeyFor(entryId, draft);
    const status = result.data;
    if (result.success && status) {
      setCredentials((current) => ({ ...current, [entryId]: status }));
      changeApiKey(entryId, "");
      setFeedback({ type: "success", text: "凭据已安全保存，可展开模型列表获取模型" });
    } else setFeedback({ type: "error", text: resultError(result.error, "保存凭据失败") });
  };

  const clearKey = async (entryId: string) => {
    const result = entryId === PRIMARY_LLM_ENTRY_ID ? await clearLlmApiKey() : await clearLlmApiKeyFor(entryId);
    const status = result.data;
    if (result.success && status) {
      setCredentials((current) => ({ ...current, [entryId]: status }));
      changeApiKey(entryId, "");
      setFeedback({ type: "success", text: "凭据已清除" });
    } else setFeedback({ type: "error", text: resultError(result.error, "清除凭据失败") });
  };

  const fetchModels = async (entryId: string, showSuccess = false) => {
    const service = serviceOf(entryId);
    if (!service) return;
    if (!service.base_url.trim()) { setFeedback({ type: "error", text: "请先填写服务地址" }); return; }
    if (!shouldFetchLlmModels(service, Boolean(credentials[entryId]?.configured), true)) {
      setFeedback({ type: "info", text: "请先保存 API Key，再展开模型列表获取模型" });
      return;
    }
    // 备用服务的模型列表读的是已落盘的配置，草稿状态下拉取到的会是旧内容
    if (entryId !== PRIMARY_LLM_ENTRY_ID && dirty) {
      setFeedback({ type: "info", text: "备用服务的模型列表读取的是已保存的配置，请等待自动保存完成后再获取" });
      return;
    }
    setModelsLoadingId(entryId);
    try {
      const result = entryId === PRIMARY_LLM_ENTRY_ID
        ? await listLlmModels({ provider: service.provider, base_url: service.base_url.trim() })
        : await listLlmModelsFor(entryId);
      if (result.success && result.data) {
        const list = result.data;
        setModels((current) => ({ ...current, [entryId]: list }));
        if (!service.model.trim() && list[0]) patchEntry(entryId, { model: list[0] });
        if (showSuccess) setFeedback({ type: "success", text: `已获取 ${list.length} 个模型` });
      } else {
        clearModels(entryId);
        setFeedback({ type: "error", text: resultError(result.error, "获取模型列表失败") });
      }
    } catch (error) {
      clearModels(entryId);
      setFeedback({ type: "error", text: error instanceof Error ? error.message : "获取模型列表失败" });
    } finally {
      setModelsLoadingId(null);
    }
  };

  const test = async (entryId: string) => {
    const service = serviceOf(entryId);
    if (!service) return;
    setTestingId(entryId);
    try {
      if (!isValidLlmConfig(service)) { setFeedback({ type: "error", text: "请检查服务地址和模型名称" }); return; }
      if (LLM_PRESETS[service.provider].requiresKey && !credentials[entryId]?.configured) {
        setFeedback({ type: "error", text: "该服务需要先配置 API Key" });
        return;
      }
      // 后端测试的是已落盘的配置，先保证磁盘上的内容与界面一致，避免测出误导性的结果
      if (entryId === PRIMARY_LLM_ENTRY_ID) {
        if (onPersist && !await onPersist(config)) return;
      } else if (dirty) {
        if (!onPersistAll) { setFeedback({ type: "error", text: "请先保存配置后再测试" }); return; }
        if (!await onPersistAll()) { setFeedback({ type: "error", text: "配置保存失败，请修正后再测试" }); return; }
      }
      const result = entryId === PRIMARY_LLM_ENTRY_ID ? await testLlmConnection() : await testLlmEntryConnection(entryId);
      if (result.success && result.data) setFeedback({ type: "success", text: `短连接测试成功 · 模型 ${result.data.model}${result.data.latency_ms ? ` · ${result.data.latency_ms}ms` : ""} · 响应: ${result.data.response}` });
      else setFeedback({ type: "error", text: resultError(result.error, "连接测试失败") });
    } catch (error) {
      setFeedback({ type: "error", text: error instanceof Error ? error.message : "连接测试失败" });
    } finally {
      setTestingId(null);
    }
  };

  const addFallback = () => {
    const entry = createFallbackEntry(config?.provider ?? "openai");
    onFallbacksChange?.([...chain, entry]);
    setActiveKeys((keys) => [...keys, entry.id]);
  };

  const removeFallback = (index: number) => {
    const entry = chain[index];
    if (!entry) return;
    Modal.confirm({
      title: `删除「${fallbackTitle(entry, index)}」？`,
      content: "该服务在系统凭据库里保存的 API Key 会一并清除，避免留下无人使用的密钥。",
      okText: "删除",
      okButtonProps: { danger: true },
      cancelText: "取消",
      onOk: async () => {
        await clearLlmApiKeyFor(entry.id).catch(() => undefined);
        onFallbacksChange?.(chain.filter((_, i) => i !== index));
        setFeedback({ type: "success", text: `已删除「${fallbackTitle(entry, index)}」及其 API Key` });
      },
    });
  };

  const reorderFallback = (from: number, to: number) => {
    const next = moveFallback(chain, from, to);
    if (next !== chain) onFallbacksChange?.(next);
  };

  const promoteFallback = (index: number) => {
    const entry = chain[index];
    if (!entry || !config) return;
    Modal.confirm({
      title: `将「${fallbackTitle(entry, index)}」设为主用服务？`,
      width: 520,
      content: (
        <Space direction="vertical" size="small">
          <Typography.Text>它会移到第 1 位成为主用服务，当前的主用服务降级为备用 {index + 1}。</Typography.Text>
          <Typography.Text>
            两个服务已保存的 API Key 会跟随各自的服务一起对调，不需要重新填写。
          </Typography.Text>
          <Typography.Text type="secondary">
            若原主用服务的密钥来自环境变量，该变量只对主用服务生效，交换后会继续作用于新的主用服务，降级下来的服务需要单独填写密钥。
          </Typography.Text>
        </Space>
      ),
      okText: "交换",
      cancelText: "取消",
      onOk: async () => {
        // 先算出交换结果，确认可行再动密钥；反过来一旦交换失败，
        // 密钥已经被搬走，配置却没变，两边就对不上了。
        const swapped = promoteFallbackToPrimary(config, chain, index);
        if (!swapped) return;
        // 密钥明文不能进入前端，对调只能整体交给后端完成。
        const statuses = await swapLlmCredentials(PRIMARY_LLM_ENTRY_ID, swapped.swappedId)
          .then((result) => (result.success && Array.isArray(result.data) ? result.data : null))
          .catch(() => null);
        if (!statuses) {
          setFeedback({ type: "error", text: "交换 API Key 失败，已取消本次调整，配置保持不变。" });
          return;
        }
        onChange(swapped.primary);
        onFallbacksChange?.(swapped.fallbacks);
        setCredentials((current) => ({
          ...current,
          ...Object.fromEntries(
            statuses.map((status) => [
              status.entry_id,
              { configured: status.configured, source: status.source },
            ]),
          ),
        }));
        setModels({});
        setFeedback({ type: "success", text: "已交换主用与备用服务，API Key 已随服务一并对调。" });
      },
    });
  };

  const renderServiceFields = (entryId: string, service: LlmServiceFields) => {
    const credential = credentials[entryId] ?? null;
    const entryModels = models[entryId] ?? [];
    const loadingModels = modelsLoadingId === entryId;
    return (
      <>
        <Form.Item label="服务地址" validateStatus={service.base_url.trim() ? undefined : "error"}>
          <Input
            aria-label={`${entryId} 服务地址`}
            value={service.base_url}
            placeholder="服务 API 地址，可按需自定义 BaseURL"
            onChange={(e) => patchEntry(entryId, { base_url: e.target.value })}
          />
        </Form.Item>
        <Form.Item label="模型" validateStatus={service.model.trim() ? undefined : "error"}>
          <AutoComplete
            aria-label={`${entryId} 模型`}
            value={service.model}
            options={entryModels.map((model) => ({ value: model, label: model }))}
            placeholder="自动获取或手动输入模型名称"
            onChange={(model) => patchEntry(entryId, { model })}
            onOpenChange={(open) => {
              if (open && !loadingModels && entryModels.length === 0) void fetchModels(entryId);
            }}
            filterOption={(input, option) => String(option?.value ?? "").toLowerCase().includes(input.toLowerCase())}
            style={{ width: "100%" }}
          />
          <Button style={{ marginTop: 8 }} loading={loadingModels} onClick={() => void fetchModels(entryId, true)}>刷新模型列表</Button>
        </Form.Item>
        <Typography.Text type="secondary">
          凭据状态：{credential?.configured ? `已配置（${credentialSourceText[credential.source]}）` : "未配置"}。应用不会读取或显示明文。
        </Typography.Text>
        <Space.Compact block style={{ marginTop: 12 }}>
          <Input.Password
            aria-label={`${entryId} API Key`}
            value={apiKeys[entryId] ?? ""}
            placeholder="设置或替换 API Key"
            onChange={(e) => changeApiKey(entryId, e.target.value)}
          />
          <Button onClick={() => void storeKey(entryId)}>保存凭据</Button>
          <Button danger disabled={!credential?.configured} onClick={() => void clearKey(entryId)}>清除</Button>
        </Space.Compact>
      </>
    );
  };

  return <Card title="大模型配置">
    <Form layout="vertical">
      <Form.Item label={<Space size={4}>服务预设{chainEnabled && chain.length > 0 && <Tag color="blue">主用</Tag>}</Space>}>
        <Select aria-label="服务预设" value={config?.provider} placeholder="选择服务后开始配置" options={options} onChange={(provider: LlmProviderPreset) => (config ? chooseProvider(PRIMARY_LLM_ENTRY_ID, provider) : onChange({ provider, base_url: LLM_PRESETS[provider].baseUrl, model: "" }))} />
      </Form.Item>
      {config && <>
        {renderServiceFields(PRIMARY_LLM_ENTRY_ID, config)}
        <Space wrap style={{ marginTop: 16 }}>
          <Button loading={testingId === PRIMARY_LLM_ENTRY_ID} type="primary" onClick={() => void test(PRIMARY_LLM_ENTRY_ID)}>短连接测试</Button>
          <Button danger onClick={() => onChange(null)}>停用 AI</Button>
        </Space>

        {chainEnabled && <>
          <Divider style={{ marginTop: 24 }} />
          <Typography.Title level={5} style={{ marginTop: 0 }}>降级链</Typography.Title>
          <Typography.Paragraph type="secondary" style={{ marginBottom: 12 }}>
            主用服务不可用时，会按下面的顺序自动降级到备用服务，可用于应对网络波动或额度用尽。
          </Typography.Paragraph>
          {chain.length > 0 && (
            <Collapse
              style={{ marginBottom: 12 }}
              activeKey={activeKeys}
              onChange={(keys) => setActiveKeys(Array.isArray(keys) ? keys : [keys])}
              items={chain.map((entry, index) => ({
                key: entry.id,
                label: (
                  <Space size={8} wrap>
                    <Tag>备用 {index + 1}</Tag>
                    <Typography.Text strong>{fallbackTitle(entry, index)}</Typography.Text>
                    {!entry.enabled && <Tag>已停用</Tag>}
                  </Space>
                ),
                extra: (
                  <Space size={4} onClick={(event) => event.stopPropagation()}>
                    <Button size="small" aria-label={`上移备用 ${index + 1}`} icon={<ArrowUpOutlined />} disabled={index === 0} onClick={() => reorderFallback(index, index - 1)} />
                    <Button size="small" aria-label={`下移备用 ${index + 1}`} icon={<ArrowDownOutlined />} disabled={index === chain.length - 1} onClick={() => reorderFallback(index, index + 1)} />
                    <Button size="small" onClick={() => promoteFallback(index)}>设为主用</Button>
                    <Button size="small" danger aria-label={`删除备用 ${index + 1}`} icon={<DeleteOutlined />} onClick={() => removeFallback(index)} />
                  </Space>
                ),
                children: (
                  <>
                    <Form.Item label="名称（选填）">
                      <Input
                        aria-label={`备用 ${index + 1} 名称`}
                        value={entry.label ?? ""}
                        placeholder="留空时显示模型名"
                        onChange={(e) => onFallbacksChange?.(chain.map((item) => item.id === entry.id ? { ...item, label: e.target.value || null } : item))}
                      />
                    </Form.Item>
                    <Form.Item label="服务预设">
                      <Select aria-label={`备用 ${index + 1} 服务预设`} value={entry.provider} options={options} onChange={(provider: LlmProviderPreset) => chooseProvider(entry.id, provider)} />
                    </Form.Item>
                    {renderServiceFields(entry.id, entry)}
                    <Space wrap style={{ marginTop: 16 }}>
                      <Button loading={testingId === entry.id} onClick={() => void test(entry.id)}>短连接测试</Button>
                      <Space size={8}>
                        <Switch
                          aria-label={`启用备用 ${index + 1}`}
                          checked={entry.enabled}
                          onChange={(enabled) => onFallbacksChange?.(chain.map((item) => item.id === entry.id ? { ...item, enabled } : item))}
                        />
                        <Typography.Text type="secondary">参与降级链</Typography.Text>
                      </Space>
                    </Space>
                  </>
                ),
              }))}
            />
          )}
          <Button icon={<PlusOutlined />} onClick={addFallback}>添加备用服务</Button>
          {chain.some((entry) => !entry.base_url.trim() || !entry.model.trim()) && (
            <Alert
              style={{ marginTop: 12 }}
              type="warning"
              showIcon
              message="有备用服务尚未填写完整"
              description="服务地址和模型名称都填写后配置才能保存，未填完期间自动保存会一直提示失败。"
            />
          )}
        </>}

        {retryConfig && onRetryConfigChange && !compact && <>
          <Divider style={{ marginTop: 24 }} />
          <Typography.Title level={5} style={{ marginTop: 0 }}>重试策略</Typography.Title>
          <Form.Item label="网络故障重试次数" extra="仅对网络连接失败重试；请求超时和密钥错误不会重试。重试次数用尽后才会降级到下一个服务。">
            <NumberField
              aria-label="网络故障重试次数"
              min={0}
              max={MAX_NETWORK_RETRY_ATTEMPTS}
              precision={0}
              fallback={0}
              value={retryConfig.network_retry_attempts}
              onChange={(value) => onRetryConfigChange({ ...retryConfig, network_retry_attempts: clampRetryAttempts(value) })}
            />
          </Form.Item>
          <Form.Item label="首次重试等待" extra="之后按指数退避，第二次等待翻倍。">
            <NumberField
              aria-label="首次重试等待"
              min={MIN_RETRY_BASE_DELAY_MS}
              max={MAX_RETRY_BASE_DELAY_MS}
              step={100}
              precision={0}
              addonAfter="毫秒"
              fallback={MIN_RETRY_BASE_DELAY_MS}
              value={retryConfig.retry_base_delay_ms}
              onChange={(value) => onRetryConfigChange({ ...retryConfig, retry_base_delay_ms: clampRetryBaseDelay(value) })}
            />
          </Form.Item>
        </>}
      </>}
      {feedback && <Alert style={{ marginTop: 16 }} type={feedback.type} showIcon message={feedback.text} />}
    </Form>
  </Card>;
}
