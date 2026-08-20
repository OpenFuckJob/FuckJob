import { useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { LlmConfig, LlmProviderEntry, LlmRetryConfig } from "@/types/app-config";
import {
  DEFAULT_LLM_REQUEST_TIMEOUT_SECONDS,
  MAX_NETWORK_RETRY_ATTEMPTS,
  MAX_LLM_REQUEST_TIMEOUT_SECONDS,
  MAX_RETRY_BASE_DELAY_MS,
  MIN_LLM_REQUEST_TIMEOUT_SECONDS,
  MIN_RETRY_BASE_DELAY_MS,
  PRIMARY_LLM_ENTRY_ID,
} from "@/types/app-config";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((command: string) =>
    Promise.resolve(
      command === "list_llm_credential_status"
        ? { success: true, data: [], error: null }
        : { success: true, data: { configured: false, source: "none" }, error: null },
    ),
  ),
}));

import {
  LLM_PRESETS,
  LlmConfigPanel,
  clampRetryAttempts,
  clampRetryBaseDelay,
  clampRequestTimeout,
  createFallbackEntry,
  createLlmEntryId,
  isValidLlmConfig,
  moveFallback,
  promoteFallbackToPrimary,
} from "./LlmConfigPanel";

describe("LLM presets", () => {
  it("keeps supported provider endpoints and key expectations deterministic", () => {
    expect(LLM_PRESETS).toEqual({
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
    });
  });
});

describe("LLM config validation", () => {
  const openAiConfig = {
    provider: "openai" as const,
    base_url: "https://llm.example.test/v1",
    model: "custom-model",
  };

  it("accepts a service address and model without advanced parameters", () => {
    expect(isValidLlmConfig(openAiConfig)).toBe(true);
  });

  it("rejects blank service addresses or model names", () => {
    expect(isValidLlmConfig({ ...openAiConfig, base_url: "  " })).toBe(false);
    expect(isValidLlmConfig({ ...openAiConfig, model: "  " })).toBe(false);
    expect(isValidLlmConfig(null)).toBe(false);
  });
});

describe("降级链条目标识", () => {
  it("生成的标识非空、不等于主用保留标识，且只含 Rust 侧允许的字符", () => {
    for (let i = 0; i < 20; i += 1) {
      const id = createLlmEntryId();
      expect(id).not.toBe("");
      expect(id).not.toBe(PRIMARY_LLM_ENTRY_ID);
      expect(id).toMatch(/^[A-Za-z0-9_-]+$/);
    }
  });

  it("randomUUID 不可用时仍生成合法标识", () => {
    const original = globalThis.crypto.randomUUID;
    Object.defineProperty(globalThis.crypto, "randomUUID", { value: undefined, configurable: true });
    try {
      const id = createLlmEntryId();
      expect(id).toMatch(/^[A-Za-z0-9_-]+$/);
      expect(id).not.toBe(PRIMARY_LLM_ENTRY_ID);
    } finally {
      Object.defineProperty(globalThis.crypto, "randomUUID", { value: original, configurable: true });
    }
  });

  it("新建备用服务带上预设地址且默认启用", () => {
    const entry = createFallbackEntry("deepseek");
    expect(entry.base_url).toBe(LLM_PRESETS.deepseek.baseUrl);
    expect(entry.model).toBe("");
    expect(entry.enabled).toBe(true);
    expect(entry.label).toBeNull();
  });
});

describe("降级链排序", () => {
  const entries = ["a", "b", "c"].map((id) => ({ ...createFallbackEntry("openai"), id }));

  it("上移与下移只调整数组顺序，标识保持不变", () => {
    expect(moveFallback(entries, 2, 1).map((entry) => entry.id)).toEqual(["a", "c", "b"]);
    expect(moveFallback(entries, 0, 1).map((entry) => entry.id)).toEqual(["b", "a", "c"]);
  });

  it("越界移动返回原数组", () => {
    expect(moveFallback(entries, 0, -1)).toBe(entries);
    expect(moveFallback(entries, 2, 3)).toBe(entries);
    expect(moveFallback(entries, 1, 1)).toBe(entries);
  });
});

describe("设为主用", () => {
  const primary: LlmConfig = { provider: "openai", base_url: "https://primary.test/v1", model: "primary-model" };
  const fallbacks: LlmProviderEntry[] = [
    { id: "backup-a", label: "备用甲", provider: "deepseek", base_url: "https://a.test", model: "model-a", enabled: true },
    { id: "backup-b", label: null, provider: "moonshot", base_url: "https://b.test", model: "model-b", enabled: false },
  ];

  it("主用槽位与被提升的备用槽位对调内容，标识随槽位保持不变", () => {
    const result = promoteFallbackToPrimary(primary, fallbacks, 0);

    expect(result?.primary).toEqual({ provider: "deepseek", base_url: "https://a.test", model: "model-a" });
    expect(result?.fallbacks[0]).toEqual({
      id: "backup-a",
      label: null,
      provider: "openai",
      base_url: "https://primary.test/v1",
      model: "primary-model",
      enabled: true,
    });
    // 密钥由后端按这两个标识一并对调，因此被降级的服务沿用原标识即可，
    // 既不会读到别人的密钥，也不需要用户重新填写
    expect(result?.swappedId).toBe("backup-a");
    expect(result?.fallbacks[1]).toBe(fallbacks[1]);
  });

  it("未被选中的备用服务不受影响", () => {
    const result = promoteFallbackToPrimary(primary, fallbacks, 1);

    expect(result?.swappedId).toBe("backup-b");
    expect(result?.fallbacks[0]).toBe(fallbacks[0]);
    expect(result?.fallbacks[1].model).toBe("primary-model");
  });

  it("索引越界时不做任何交换", () => {
    expect(promoteFallbackToPrimary(primary, fallbacks, 5)).toBeNull();
  });
});

describe("重试策略取值区间", () => {
  it("重试次数被限制在 0 到 5", () => {
    expect(clampRetryAttempts(-3)).toBe(0);
    expect(clampRetryAttempts(99)).toBe(5);
    expect(clampRetryAttempts(null)).toBe(0);
    expect(clampRetryAttempts(3)).toBe(3);
  });

  it("首次重试等待被限制在 100 到 10000 毫秒", () => {
    expect(clampRetryBaseDelay(1)).toBe(100);
    expect(clampRetryBaseDelay(999999)).toBe(10000);
    expect(clampRetryBaseDelay(null)).toBe(100);
    expect(clampRetryBaseDelay(750)).toBe(750);
  });

  it("模型请求超时被限制在 1 到 600 秒", () => {
    expect(clampRequestTimeout(0)).toBe(MIN_LLM_REQUEST_TIMEOUT_SECONDS);
    expect(clampRequestTimeout(999999)).toBe(MAX_LLM_REQUEST_TIMEOUT_SECONDS);
    expect(clampRequestTimeout(null)).toBe(DEFAULT_LLM_REQUEST_TIMEOUT_SECONDS);
    expect(clampRequestTimeout(240)).toBe(240);
  });
});

const primaryConfig: LlmConfig = {
  provider: "openai",
  base_url: "https://api.openai.com/v1",
  model: "gpt-test",
};

function Harness({
  initialConfig = primaryConfig,
  initialFallbacks = [],
  onFallbacks,
  onRetry,
  dirty,
}: {
  initialConfig?: LlmConfig;
  initialFallbacks?: LlmProviderEntry[];
  onFallbacks?: (next: LlmProviderEntry[]) => void;
  onRetry?: (next: LlmRetryConfig) => void;
  dirty?: boolean;
}) {
  const [config, setConfig] = useState<LlmConfig | null>(initialConfig);
  const [enabled, setEnabled] = useState(true);
  const [fallbacks, setFallbacks] = useState<LlmProviderEntry[]>(initialFallbacks);
  const [retry, setRetry] = useState<LlmRetryConfig>({
    network_retry_attempts: 2,
    retry_base_delay_ms: 500,
    request_timeout_seconds: DEFAULT_LLM_REQUEST_TIMEOUT_SECONDS,
  });
  return (
    <LlmConfigPanel
      config={config}
      enabled={enabled}
      onChange={setConfig}
      onEnabledChange={setEnabled}
      fallbacks={fallbacks}
      onFallbacksChange={(next) => {
        onFallbacks?.(next);
        setFallbacks(next);
      }}
      retryConfig={retry}
      onRetryConfigChange={(next) => {
        onRetry?.(next);
        setRetry(next);
      }}
      dirty={dirty}
    />
  );
}

const fallbackEntry = (id: string, model: string): LlmProviderEntry => ({
  id,
  label: null,
  provider: "deepseek",
  base_url: "https://api.deepseek.com",
  model,
  enabled: true,
});

describe("LlmConfigPanel 降级链界面", () => {
  beforeEach(() => vi.clearAllMocks());
  afterEach(cleanup);

  it("只有主用服务时不渲染多余的降级链 UI，只留说明和新增入口", async () => {
    render(<Harness />);

    expect(await screen.findByRole("button", { name: /添加备用服务/ })).toBeInTheDocument();
    expect(screen.getByText(/主用服务不可用时/)).toBeInTheDocument();
    expect(screen.queryByText("备用 1")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "设为主用" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "上移备用 1" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "下移备用 1" })).not.toBeInTheDocument();
    // 参考表单始终明确标出主用服务，且降级链中的第一行同步显示主用状态。
    expect(screen.getAllByText("主用").length).toBeGreaterThan(0);
  });

  it("首次获取模型时先保存当前输入的 API Key，再请求模型列表", async () => {
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "list_llm_credential_status") {
        return Promise.resolve({ success: true, data: [], error: null });
      }
      if (command === "set_llm_api_key") {
        return Promise.resolve({ success: true, data: { configured: true, source: "keychain" }, error: null });
      }
      if (command === "list_llm_models") {
        return Promise.resolve({ success: true, data: ["gpt-first"], error: null });
      }
      return Promise.resolve({ success: true, data: { configured: false, source: "none" }, error: null });
    });

    render(<Harness initialConfig={{ ...primaryConfig, model: "" }} />);
    fireEvent.change(await screen.findByLabelText("primary API Key"), { target: { value: "secret-key" } });
    fireEvent.click(screen.getByRole("button", { name: "刷新primary模型列表" }));

    await waitFor(() => expect(screen.getByLabelText("primary 模型")).toHaveValue("gpt-first"));
    const commands = vi.mocked(invoke).mock.calls.map(([command]) => command);
    expect(commands.indexOf("set_llm_api_key")).toBeGreaterThanOrEqual(0);
    expect(commands.indexOf("list_llm_models")).toBeGreaterThan(commands.indexOf("set_llm_api_key"));
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("set_llm_api_key", { apiKey: "secret-key" });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("list_llm_models", {
      provider: "openai",
      baseUrl: "https://api.openai.com/v1",
    });
  });

  it("编辑模型名不触发任何模型列表请求", async () => {
    render(<Harness initialConfig={{ ...primaryConfig, model: "" }} />);

    const model = await screen.findByLabelText("primary 模型");
    // 展开下拉、逐字输入，都不该打网络：模型名以手动输入为准，列表只由按钮拉取
    fireEvent.mouseDown(model);
    fireEvent.focus(model);
    for (const value of ["m", "my", "my-own-model"]) {
      fireEvent.change(model, { target: { value } });
    }

    expect(model).toHaveValue("my-own-model");
    expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).not.toContain("list_llm_models");
    expect(screen.queryByText(/请先填写 API Key/)).not.toBeInTheDocument();
  });

  it("尚未保存的备用服务按界面草稿取模型列表，不再要求先保存配置", async () => {
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "list_llm_credential_status") {
        return Promise.resolve({
          success: true,
          data: [
            { entry_id: PRIMARY_LLM_ENTRY_ID, configured: true, source: "keychain" },
            { entry_id: "backup-a", configured: true, source: "keychain" },
          ],
          error: null,
        });
      }
      if (command === "list_llm_models_for") {
        return Promise.resolve({ success: true, data: ["deepseek-chat"], error: null });
      }
      return Promise.resolve({ success: true, data: { configured: true, source: "keychain" }, error: null });
    });

    // dirty 表示整份配置还没落盘：备用服务模型名为空时它根本保存不了，
    // 这正是「取模型要先保存、保存要先有模型」死锁发生的场景
    render(<Harness dirty initialFallbacks={[fallbackEntry("backup-a", "")]} />);

    // 「备用 1」同时出现在折叠面板标题和无障碍标注里，点标题所在的 header 展开
    const [title] = await screen.findAllByText("备用 1");
    fireEvent.click(title.closest(".ant-collapse-header") as HTMLElement);
    fireEvent.click(await screen.findByRole("button", { name: "刷新backup-a模型列表" }));

    await waitFor(() => expect(screen.getByLabelText("backup-a 模型")).toHaveValue("deepseek-chat"));
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("list_llm_models_for", {
      entryId: "backup-a",
      provider: "deepseek",
      baseUrl: "https://api.deepseek.com",
    });
  });

  it("停用只切换状态，不清除主用配置", async () => {
    render(<Harness />);

    expect(await screen.findByLabelText("大模型启用开关")).toBeChecked();
    fireEvent.click(screen.getByLabelText("大模型启用开关"));
    expect(screen.getByText("已停用")).toBeInTheDocument();
    expect(screen.getByText("配置已保留，启用大模型后展开编辑。")).toBeInTheDocument();
    expect(screen.queryByLabelText("primary 模型")).not.toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("大模型启用开关"));
    expect(screen.getByLabelText("primary 模型")).toHaveValue("gpt-test");
  });

  it("新增备用服务后列表出现新条目，其标识非空且不等于 primary", async () => {
    const onFallbacks = vi.fn();
    render(<Harness onFallbacks={onFallbacks} />);

    fireEvent.click(await screen.findByRole("button", { name: /添加备用服务/ }));

    // 「备用 1」同时出现在折叠面板标题的 Tag 和未填写完整的提示里，用 AllBy 查询
    await waitFor(() => expect(screen.getAllByText("备用 1").length).toBeGreaterThan(0));
    expect(onFallbacks).toHaveBeenCalledTimes(1);
    const [created] = onFallbacks.mock.calls[0][0] as LlmProviderEntry[];
    expect(created.id).toBeTruthy();
    expect(created.id).not.toBe(PRIMARY_LLM_ENTRY_ID);
    expect(created.id).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(screen.getByRole("button", { name: "设为主用" })).toBeInTheDocument();
  });

  it("备用服务之间上移下移后数组顺序正确变化", async () => {
    const onFallbacks = vi.fn();
    render(
      <Harness
        onFallbacks={onFallbacks}
        initialFallbacks={[fallbackEntry("backup-a", "model-a"), fallbackEntry("backup-b", "model-b"), fallbackEntry("backup-c", "model-c")]}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: "下移备用 1" }));
    expect((onFallbacks.mock.calls[0][0] as LlmProviderEntry[]).map((entry) => entry.id)).toEqual(["backup-b", "backup-a", "backup-c"]);

    fireEvent.click(screen.getByRole("button", { name: "上移备用 3" }));
    expect((onFallbacks.mock.calls[1][0] as LlmProviderEntry[]).map((entry) => entry.id)).toEqual(["backup-b", "backup-c", "backup-a"]);

    // 界面顺序同步更新：第 1 位是 model-b
    const first = screen.getByText("备用 1").closest(".ant-collapse-header");
    expect(within(first as HTMLElement).getByText("model-b")).toBeInTheDocument();
  });

  it("第一项禁用上移、最后一项禁用下移", async () => {
    render(<Harness initialFallbacks={[fallbackEntry("backup-a", "model-a"), fallbackEntry("backup-b", "model-b")]} />);

    expect(await screen.findByRole("button", { name: "上移备用 1" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "下移备用 1" })).not.toBeDisabled();
    expect(screen.getByRole("button", { name: "上移备用 2" })).not.toBeDisabled();
    expect(screen.getByRole("button", { name: "下移备用 2" })).toBeDisabled();
  });

  // 越界取值的夹紧行为由 clampRetryAttempts / clampRetryBaseDelay 的纯函数测试覆盖。
  // 这里只验证控件本身声明了正确的取值边界，并且合法输入能正常回传，
  // 不再往 max=5 的 InputNumber 里塞 99——rc-input-number 会直接拒绝该输入，
  // 连 onChange 都不会触发，测出来的失败与夹紧逻辑无关。
  it("重试表单声明了合法取值边界，并把用户输入回传上层", async () => {
    const onRetry = vi.fn();
    render(<Harness onRetry={onRetry} />);

    const lastCallArg = (mock: typeof onRetry) =>
      mock.mock.calls[mock.mock.calls.length - 1]?.[0];

    const attempts = await screen.findByLabelText("网络故障重试次数（输入数值）");
    expect(attempts).toHaveAttribute("aria-valuemin", "0");
    expect(attempts).toHaveAttribute("aria-valuemax", String(MAX_NETWORK_RETRY_ATTEMPTS));
    fireEvent.change(attempts, { target: { value: "4" } });
    await waitFor(() => expect(onRetry).toHaveBeenCalled());
    expect(lastCallArg(onRetry).network_retry_attempts).toBe(4);

    onRetry.mockClear();
    const delay = screen.getByLabelText("首次重试等待（输入数值）");
    expect(delay).toHaveAttribute("aria-valuemin", String(MIN_RETRY_BASE_DELAY_MS));
    expect(delay).toHaveAttribute("aria-valuemax", String(MAX_RETRY_BASE_DELAY_MS));
    fireEvent.change(delay, { target: { value: "800" } });
    await waitFor(() => expect(onRetry).toHaveBeenCalled());
    expect(lastCallArg(onRetry).retry_base_delay_ms).toBe(800);

    onRetry.mockClear();
    const timeout = screen.getByLabelText("模型请求超时（输入数值）");
    expect(timeout).toHaveAttribute("aria-valuemin", String(MIN_LLM_REQUEST_TIMEOUT_SECONDS));
    expect(timeout).toHaveAttribute("aria-valuemax", String(MAX_LLM_REQUEST_TIMEOUT_SECONDS));
    fireEvent.change(timeout, { target: { value: "240" } });
    await waitFor(() => expect(onRetry).toHaveBeenCalled());
    expect(lastCallArg(onRetry).request_timeout_seconds).toBe(240);
  });
});
