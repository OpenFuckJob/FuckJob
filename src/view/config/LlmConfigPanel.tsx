import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { Alert, AutoComplete, Button, Card, Collapse, Form, Input, Modal, Select, Space, Switch, Tag, Typography } from "antd";
import {
  ApiOutlined,
  ArrowDownOutlined,
  ArrowUpOutlined,
  CheckCircleFilled,
  ClockCircleOutlined,
  DeleteOutlined,
  FieldTimeOutlined,
  PlusOutlined,
  ReloadOutlined,
  SafetyCertificateOutlined,
} from "@ant-design/icons";
import {
  DEFAULT_LLM_REQUEST_TIMEOUT_SECONDS,
  MAX_NETWORK_RETRY_ATTEMPTS,
  MAX_LLM_REQUEST_TIMEOUT_SECONDS,
  MAX_RETRY_BASE_DELAY_MS,
  MIN_RETRY_BASE_DELAY_MS,
  MIN_LLM_REQUEST_TIMEOUT_SECONDS,
  PRIMARY_LLM_ENTRY_ID,
  isLlmServiceUsable,
  type LlmConfig,
  type LlmProviderEntry,
  type LlmProviderPreset,
  type LlmRetryConfig,
} from "@/types/app-config";
import { SettingGroup, SettingSlider } from "@/components/SettingField";
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
export const isValidLlmConfig = (value: LlmServiceFields | null) => isLlmServiceUsable(value);

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
export const clampRequestTimeout = (value: number | null | undefined) =>
  Math.min(
    Math.max(Math.round(value ?? DEFAULT_LLM_REQUEST_TIMEOUT_SECONDS), MIN_LLM_REQUEST_TIMEOUT_SECONDS),
    MAX_LLM_REQUEST_TIMEOUT_SECONDS,
  );

const fallbackTitle = (entry: LlmProviderEntry, index: number) =>
  entry.label?.trim() || entry.model.trim() || `备用 ${index + 1}`;

const credentialSourceText: Record<LlmCredentialStatus["source"], string> = {
  keychain: "系统凭据库",
  environment: "环境变量",
  none: "未配置",
};

export interface LlmConfigPanelProps {
  config: LlmConfig | null;
  enabled?: boolean;
  onChange: (config: LlmConfig | null) => void;
  onEnabledChange?: (enabled: boolean) => void;
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
  enabled,
  onChange,
  onEnabledChange,
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
  const [modal, modalContextHolder] = Modal.useModal();
  const [credentials, setCredentials] = useState<Record<string, LlmCredentialStatus>>({});
  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});
  const [models, setModels] = useState<Record<string, string[]>>({});
  const [modelsLoadingId, setModelsLoadingId] = useState<string | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [activeKeys, setActiveKeys] = useState<string[]>([]);
  const [connectionOk, setConnectionOk] = useState<Record<string, boolean>>({});
  // 旧配置没有 llm_enabled 时按启用处理；只有明确写入 false 才代表停用。
  const llmEnabled = enabled !== false;
  const canToggle = Boolean(onEnabledChange && enabled !== undefined);
  const statusLabel = config ? (llmEnabled ? "已启用" : "已停用") : "未配置";

  const showFeedback = useCallback(
    (
      type: "success" | "error" | "info" | "warning",
      title: string,
      content: ReactNode,
      width = 520,
    ) => {
      modal[type]({
        title,
        content,
        centered: true,
        width,
        okText: "知道了",
      });
    },
    [modal],
  );

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
        else showFeedback("info", "凭据状态", "无法读取凭据状态；可继续配置本地服务。");
      } catch {
        if (!cancelled) showFeedback("info", "凭据状态", "无法读取凭据状态；可继续配置本地服务。");
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
  }, [entryIdsKey, showFeedback]);

  const options = useMemo(() => Object.entries(LLM_PRESETS).map(([value, item]) => ({ value, label: item.label })), []);
  const serviceOf = (entryId: string): LlmServiceFields | null =>
    entryId === PRIMARY_LLM_ENTRY_ID ? config : chain.find((entry) => entry.id === entryId) ?? null;

  const clearModels = (entryId: string) => setModels((current) => ({ ...current, [entryId]: [] }));
  const invalidateConnection = (entryId: string) => {
    setConnectionOk((current) => current[entryId] ? { ...current, [entryId]: false } : current);
  };
  const patchEntry = (entryId: string, next: Partial<LlmServiceFields>) => {
    invalidateConnection(entryId);
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
    invalidateConnection(entryId);
    setApiKeys((current) => ({ ...current, [entryId]: value }));
    if (entryId === PRIMARY_LLM_ENTRY_ID) onPendingApiKeyChange?.(value);
  };

  const storeKey = async (entryId: string, showSuccess = true): Promise<boolean> => {
    const draft = (apiKeys[entryId] ?? "").trim();
    if (!draft) {
      showFeedback("error", "保存凭据失败", "请输入 API Key");
      return false;
    }

    try {
      const result = entryId === PRIMARY_LLM_ENTRY_ID
        ? await setLlmApiKey(draft)
        : await setLlmApiKeyFor(entryId, draft);
      const status = result.data;
      if (result.success && status) {
        setCredentials((current) => ({ ...current, [entryId]: status }));
        changeApiKey(entryId, "");
        if (showSuccess) showFeedback("success", "凭据已保存", "凭据已安全保存，可以点「获取模型」拉取模型列表了");
        return true;
      }
      showFeedback("error", "保存凭据失败", resultError(result.error, "保存凭据失败"));
    } catch (error) {
      showFeedback("error", "保存凭据失败", error instanceof Error ? error.message : "保存凭据失败");
    }
    return false;
  };

  const clearKey = async (entryId: string) => {
    const result = entryId === PRIMARY_LLM_ENTRY_ID ? await clearLlmApiKey() : await clearLlmApiKeyFor(entryId);
    const status = result.data;
    if (result.success && status) {
      setCredentials((current) => ({ ...current, [entryId]: status }));
      changeApiKey(entryId, "");
      showFeedback("success", "凭据已清除", "凭据已清除");
    } else showFeedback("error", "清除凭据失败", resultError(result.error, "清除凭据失败"));
  };

  /**
   * 拉取模型列表填充下拉备选项。
   *
   * 只由「获取模型」按钮触发。模型名以手动输入为准，列表只是省去打字的辅助，
   * 所以编辑输入框、展开下拉都不该发请求——那会在用户逐字输入模型名时反复打网络。
   */
  const loadModels = async (entryId: string) => {
    const service = serviceOf(entryId);
    if (!service) return;
    const baseUrl = service.base_url.trim();
    if (!baseUrl) { showFeedback("error", "获取模型列表失败", "请先填写服务地址"); return; }

    // 用户可能刚在输入框里填了 Key 还没点保存。先把这份草稿密钥落盘再取列表，
    // 否则要么取不到，要么悄悄用了上一把旧密钥。这里用返回值判断，不等 state 更新。
    if (LLM_PRESETS[service.provider].requiresKey) {
      const pendingKey = (apiKeys[entryId] ?? "").trim();
      if (pendingKey) {
        if (!await storeKey(entryId, false)) return;
      } else if (!credentials[entryId]?.configured) {
        showFeedback("info", "获取模型列表", "请先填写 API Key，或直接在输入框里手动填写模型名称。");
        return;
      }
    }

    setModelsLoadingId(entryId);
    try {
      // 一律按界面上的当前值取列表，不读已落盘的配置：
      // 用户刚改完地址就想看新服务有哪些模型，这时磁盘上还是旧的那份。
      const draft = { provider: service.provider, base_url: baseUrl };
      const result = entryId === PRIMARY_LLM_ENTRY_ID
        ? await listLlmModels(draft)
        : await listLlmModelsFor(entryId, draft);
      if (result.success && result.data) {
        const list = result.data;
        setModels((current) => ({ ...current, [entryId]: list }));
        if (!service.model.trim() && list[0]) patchEntry(entryId, { model: list[0] });
        showFeedback("success", "模型列表已更新", `已获取 ${list.length} 个模型`);
      } else {
        clearModels(entryId);
        showFeedback("error", "获取模型列表失败", resultError(result.error, "获取模型列表失败"));
      }
    } catch (error) {
      clearModels(entryId);
      showFeedback("error", "获取模型列表失败", error instanceof Error ? error.message : "获取模型列表失败");
    } finally {
      setModelsLoadingId(null);
    }
  };

  const test = async (entryId: string) => {
    const service = serviceOf(entryId);
    if (!service) return;
    setTestingId(entryId);
    try {
      if (!isValidLlmConfig(service)) { showFeedback("error", "短连接测试失败", "请检查服务地址和模型名称"); return; }
      if (LLM_PRESETS[service.provider].requiresKey && !credentials[entryId]?.configured) {
        showFeedback("error", "短连接测试失败", "该服务需要先配置 API Key");
        return;
      }
      // 后端测试的是已落盘的配置，先保证磁盘上的内容与界面一致，避免测出误导性的结果
      if (entryId === PRIMARY_LLM_ENTRY_ID) {
        if (onPersist && !await onPersist(config)) return;
      } else if (dirty) {
        if (!onPersistAll) { showFeedback("error", "短连接测试失败", "请先保存配置后再测试"); return; }
        if (!await onPersistAll()) { showFeedback("error", "短连接测试失败", "配置保存失败，请修正后再测试"); return; }
      }
      const result = entryId === PRIMARY_LLM_ENTRY_ID ? await testLlmConnection() : await testLlmEntryConnection(entryId);
      if (result.success && result.data) {
        setConnectionOk((current) => ({ ...current, [entryId]: true }));
        showFeedback(
          "success",
          "短连接测试成功",
          (
            <Space direction="vertical" size={8}>
              <Typography.Text>
                模型：<Typography.Text strong>{result.data.model}</Typography.Text>
              </Typography.Text>
              {result.data.latency_ms !== undefined && (
                <Typography.Text>
                  耗时：<Typography.Text strong>{result.data.latency_ms}ms</Typography.Text>
                </Typography.Text>
              )}
              <Typography.Paragraph className="mb-0 break-words">
                响应：{result.data.response}
              </Typography.Paragraph>
            </Space>
          ),
          560,
        );
      } else {
        invalidateConnection(entryId);
        showFeedback("error", "连接测试失败", resultError(result.error, "连接测试失败"));
      }
    } catch (error) {
      invalidateConnection(entryId);
      showFeedback("error", "连接测试失败", error instanceof Error ? error.message : "连接测试失败");
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
        showFeedback("success", "备用服务已删除", `已删除「${fallbackTitle(entry, index)}」及其 API Key`);
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
          showFeedback("error", "切换主用服务失败", "交换 API Key 失败，已取消本次调整，配置保持不变。");
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
        setConnectionOk({});
        showFeedback("success", "已切换主用服务", "已交换主用与备用服务，API Key 已随服务一并对调。");
      },
    });
  };

  const renderModelField = (entryId: string, service: LlmServiceFields) => {
    const entryModels = models[entryId] ?? [];
    const loadingModels = modelsLoadingId === entryId;
    return (
      <div className="flex min-w-0 gap-2">
        <AutoComplete
          className="min-w-0 flex-1"
          aria-label={`${entryId} 模型`}
          value={service.model}
          options={entryModels.map((model) => ({ value: model, label: model }))}
          placeholder="直接输入模型名称，或点右侧按钮获取"
          onChange={(model) => patchEntry(entryId, { model })}
          filterOption={(input, option) => String(option?.value ?? "").toLowerCase().includes(input.toLowerCase())}
        />
        <Button
          className="shrink-0"
          aria-label={`刷新${entryId}模型列表`}
          title="从服务获取可用模型列表"
          loading={loadingModels}
          icon={<ReloadOutlined />}
          onClick={() => void loadModels(entryId)}
        >
          获取模型
        </Button>
      </div>
    );
  };

  const renderServiceFields = (entryId: string, service: LlmServiceFields) => (
    <div className="grid grid-cols-1 gap-x-4 md:grid-cols-2">
      <Form.Item label="服务地址" validateStatus={service.base_url.trim() ? undefined : "error"}>
        <Input
          aria-label={`${entryId} 服务地址`}
          value={service.base_url}
          placeholder="服务 API 地址，可按需自定义 BaseURL"
          onChange={(e) => patchEntry(entryId, { base_url: e.target.value })}
        />
      </Form.Item>
      <Form.Item label="模型" validateStatus={service.model.trim() ? undefined : "error"}>
        {renderModelField(entryId, service)}
      </Form.Item>
    </div>
  );

  const renderCredentialFields = (entryId: string) => {
    const credential = credentials[entryId] ?? null;
    const tested = connectionOk[entryId] === true;
    return (
      <>
        <Form.Item label="API Key" className="!mb-3">
          <div className="flex gap-2">
          <Input.Password
            aria-label={`${entryId} API Key`}
            value={apiKeys[entryId] ?? ""}
            placeholder="设置或替换 API Key"
            onChange={(e) => changeApiKey(entryId, e.target.value)}
          />
            <Button
              className="shrink-0"
              loading={testingId === entryId}
              icon={tested ? <CheckCircleFilled className="text-emerald-500" /> : undefined}
              onClick={() => void test(entryId)}
            >
              验证连接
            </Button>
          </div>
        </Form.Item>
        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-slate-100 pt-3">
          <Typography.Text type="secondary" className="text-xs">
            <SafetyCertificateOutlined className="mr-1 text-sky-500" />
            凭据状态：{credential?.configured ? `已配置（${credentialSourceText[credential.source]}）` : "未配置"}。应用不会读取或显示明文。
          </Typography.Text>
          <Space size={8}>
            <Button size="small" onClick={() => void storeKey(entryId)}>保存凭据</Button>
            <Button size="small" danger disabled={!credential?.configured} onClick={() => void clearKey(entryId)}>清除</Button>
          </Space>
        </div>
      </>
    );
  };

  return <Card
    title="大模型配置"
    extra={canToggle ? (
      <Space size={8}>
        <Typography.Text type={config ? undefined : "secondary"}>
          {statusLabel}
        </Typography.Text>
        <Switch
          aria-label="大模型启用开关"
          checked={Boolean(config && llmEnabled)}
          disabled={!config}
          onChange={(next) => {
            if (config) onEnabledChange?.(next);
          }}
        />
      </Space>
    ) : undefined}
  >
    {modalContextHolder}
    {config && !llmEnabled ? (
      <Typography.Text type="secondary">
        配置已保留，启用大模型后展开编辑。
      </Typography.Text>
    ) : (
      <Form layout="vertical">
        {!config ? (
          <section className="rounded-lg border border-slate-200/80 bg-white p-4">
            <Typography.Title level={5} className="!mb-4">基础配置</Typography.Title>
            <Form.Item label="服务预设" className="!mb-0">
              <Select
                aria-label="服务预设"
                value={undefined}
                placeholder="选择服务后开始配置"
                options={options}
                onChange={(provider: LlmProviderPreset) => onChange({
                  provider,
                  base_url: LLM_PRESETS[provider].baseUrl,
                  model: "",
                })}
              />
            </Form.Item>
          </section>
        ) : (
          <>
            <section className="rounded-lg border border-slate-200/80 bg-white p-4 md:p-5">
              <div className="mb-4 flex items-start justify-between gap-4">
                <div>
                  <Typography.Title level={5} className="!mb-1">基础配置</Typography.Title>
                  <Typography.Text type="secondary" className="text-xs">
                    配置主用模型服务，连接失败时会按下方顺序切换备用服务。
                  </Typography.Text>
                </div>
                <Tag color="blue" className="!mr-0">主用</Tag>
              </div>

              <div className="grid grid-cols-1 gap-x-4 md:grid-cols-2">
                <Form.Item label="服务预设">
                  <Select
                    aria-label="服务预设"
                    value={config.provider}
                    options={options}
                    onChange={(provider: LlmProviderPreset) => chooseProvider(PRIMARY_LLM_ENTRY_ID, provider)}
                  />
                </Form.Item>
                <Form.Item label="模型" validateStatus={config.model.trim() ? undefined : "error"}>
                  {renderModelField(PRIMARY_LLM_ENTRY_ID, config)}
                </Form.Item>
              </div>

              <Form.Item label="服务地址" validateStatus={config.base_url.trim() ? undefined : "error"}>
                <Input
                  aria-label="primary 服务地址"
                  value={config.base_url}
                  placeholder="服务 API 地址，可按需自定义 BaseURL"
                  onChange={(e) => patchEntry(PRIMARY_LLM_ENTRY_ID, { base_url: e.target.value })}
                />
              </Form.Item>
              {renderCredentialFields(PRIMARY_LLM_ENTRY_ID)}
              {!isLlmServiceUsable(config) && (
                <Alert
                  className="mt-3"
                  type="info"
                  showIcon
                  message="补齐服务地址和模型后，大模型才会启用"
                  description="当前填写的内容会照常保存，随时可以回来接着配。模型名可以直接输入，也可以点「获取模型」从服务拉取。"
                />
              )}
            </section>

            {chainEnabled && (
              <section className="mt-4 overflow-hidden rounded-lg border border-slate-200/80 bg-white">
                <div className="border-b border-slate-100 px-4 py-4 md:px-5">
                  <Typography.Title level={5} className="!mb-1">可用模型（降级链）</Typography.Title>
                  <Typography.Text type="secondary" className="text-xs">
                    主用服务不可用时，按以下顺序自动切换备用服务，提升可用性与稳定性。
                  </Typography.Text>
                </div>

                <div className="mx-4 my-4 overflow-hidden rounded-lg border border-slate-200/80 md:mx-5">
                  <div className="flex min-h-14 items-center gap-3 border-b border-slate-100 px-3 py-2.5 md:px-4">
                    <span className="w-6 shrink-0 text-center text-xs font-semibold text-slate-400">1</span>
                    <Typography.Text strong className="min-w-0 flex-1 truncate">
                      {config.model.trim() || "未选择模型"}
                    </Typography.Text>
                    <Tag color="green" className="!mr-0">主用</Tag>
                  </div>

                  {chain.length > 0 && (
                    <Collapse
                      bordered={false}
                      className="!rounded-none"
                      activeKey={activeKeys}
                      onChange={(keys) => setActiveKeys(Array.isArray(keys) ? keys : [keys])}
                      items={chain.map((entry, index) => ({
                        key: entry.id,
                        label: (
                          <div className="flex min-w-0 items-center gap-3">
                            <span className="w-6 shrink-0 text-center text-xs font-semibold text-slate-400">{index + 2}</span>
                            <Typography.Text strong className="min-w-0 flex-1 truncate">
                              {fallbackTitle(entry, index)}
                            </Typography.Text>
                            <Tag
                              color={!entry.enabled ? "red" : isLlmServiceUsable(entry) ? undefined : "orange"}
                              className="!mr-0"
                            >
                              {!entry.enabled ? "已停用" : isLlmServiceUsable(entry) ? "备用" : "未填完"}
                            </Tag>
                            <span className="sr-only">备用 {index + 1}</span>
                          </div>
                        ),
                        extra: (
                          <Space size={2} onClick={(event) => event.stopPropagation()}>
                            <Button size="small" type="text" aria-label={`上移备用 ${index + 1}`} icon={<ArrowUpOutlined />} disabled={index === 0} onClick={() => reorderFallback(index, index - 1)} />
                            <Button size="small" type="text" aria-label={`下移备用 ${index + 1}`} icon={<ArrowDownOutlined />} disabled={index === chain.length - 1} onClick={() => reorderFallback(index, index + 1)} />
                            <Button size="small" onClick={() => promoteFallback(index)}>设为主用</Button>
                            <Button size="small" type="text" danger aria-label={`删除备用 ${index + 1}`} icon={<DeleteOutlined />} onClick={() => removeFallback(index)} />
                          </Space>
                        ),
                        children: (
                          <div className="px-1 pb-1 pt-2">
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
                            {renderCredentialFields(entry.id)}
                            <div className="flex items-center gap-2 border-t border-slate-100 pt-3">
                              <Switch
                                aria-label={`启用备用 ${index + 1}`}
                                checked={entry.enabled}
                                onChange={(enabled) => onFallbacksChange?.(chain.map((item) => item.id === entry.id ? { ...item, enabled } : item))}
                              />
                              <Typography.Text type="secondary" className="text-xs">参与降级链</Typography.Text>
                            </div>
                          </div>
                        ),
                      }))}
                    />
                  )}
                </div>

                <div className="px-4 pb-4 md:px-5">
                  <Button block type="dashed" icon={<PlusOutlined />} onClick={addFallback}>添加备用服务</Button>
                  {chain.some((entry) => !isLlmServiceUsable(entry)) && (
                    <Alert
                      className="mt-3"
                      type="warning"
                      showIcon
                      message="有备用服务尚未填写完整"
                      description="缺服务地址或模型名的备用服务会照常保存，但不会参与降级，补齐后自动生效。"
                    />
                  )}
                </div>
              </section>
            )}

            {retryConfig && onRetryConfigChange && !compact && (
              <section className="mt-4 overflow-hidden rounded-lg border border-slate-200/80 bg-white">
                <div className="border-b border-slate-100 px-4 py-4 md:px-5">
                  <Typography.Title level={5} className="!mb-1">重试与超时</Typography.Title>
                  <Typography.Text type="secondary" className="text-xs">
                    控制网络故障时的重试节奏，以及单次模型请求的最长等待时间。
                  </Typography.Text>
                </div>
                <SettingGroup className="rounded-none border-0">
                  <SettingSlider
                    icon={<ApiOutlined />}
                    title="网络故障重试次数"
                    description="仅对网络连接失败重试；请求超时和密钥错误不会重试。重试次数用尽后才会降级到下一个服务"
                    min={0}
                    max={MAX_NETWORK_RETRY_ATTEMPTS}
                    fallback={0}
                    value={retryConfig.network_retry_attempts}
                    unit="次"
                    valueLabel={(value) => (value === 0 ? "不重试" : null)}
                    onChange={(value) => onRetryConfigChange({ ...retryConfig, network_retry_attempts: clampRetryAttempts(value) })}
                  />
                  <SettingSlider
                    icon={<ClockCircleOutlined />}
                    title="首次重试等待"
                    description="之后按指数退避，第二次等待翻倍"
                    min={MIN_RETRY_BASE_DELAY_MS}
                    max={MAX_RETRY_BASE_DELAY_MS}
                    sliderMax={3000}
                    step={100}
                    fallback={MIN_RETRY_BASE_DELAY_MS}
                    value={retryConfig.retry_base_delay_ms}
                    unit="毫秒"
                    onChange={(value) => onRetryConfigChange({ ...retryConfig, retry_base_delay_ms: clampRetryBaseDelay(value) })}
                  />
                  <SettingSlider
                    icon={<FieldTimeOutlined />}
                    title="模型请求超时"
                    description="超过这个时间仍未返回结果，就会判定为超时并终止本次请求"
                    min={MIN_LLM_REQUEST_TIMEOUT_SECONDS}
                    max={MAX_LLM_REQUEST_TIMEOUT_SECONDS}
                    sliderMax={180}
                    step={5}
                    fallback={DEFAULT_LLM_REQUEST_TIMEOUT_SECONDS}
                    value={retryConfig.request_timeout_seconds}
                    unit="秒"
                    onChange={(value) => onRetryConfigChange({
                      ...retryConfig,
                      request_timeout_seconds: clampRequestTimeout(value),
                    })}
                  />
                </SettingGroup>
              </section>
            )}
          </>
        )}
      </Form>
    )}
  </Card>;
}
