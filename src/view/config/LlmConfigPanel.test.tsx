import { useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LlmConfig, LlmProviderEntry, LlmRetryConfig } from "@/types/app-config";
import {
  MAX_NETWORK_RETRY_ATTEMPTS,
  MAX_RETRY_BASE_DELAY_MS,
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
  createFallbackEntry,
  createLlmEntryId,
  isValidLlmConfig,
  moveFallback,
  promoteFallbackToPrimary,
  shouldFetchLlmModels,
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

describe("LLM model list loading", () => {
  it("requires an explicit user action and saved key for key-based providers", () => {
    const config = {
      provider: "deepseek" as const,
      base_url: "https://api.deepseek.com",
      model: "",
    };

    expect(shouldFetchLlmModels(config, false, false)).toBe(false);
    expect(shouldFetchLlmModels(config, true, false)).toBe(false);
    expect(shouldFetchLlmModels(config, true, true)).toBe(true);
  });

  it("allows local Ollama model loading without a saved key", () => {
    const config = {
      provider: "ollama" as const,
      base_url: "http://127.0.0.1:11434",
      model: "",
    };

    expect(shouldFetchLlmModels(config, false, true)).toBe(true);
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

  it("交换配置内容，并给被降级的主用服务分配全新标识", () => {
    const result = promoteFallbackToPrimary(primary, fallbacks, 0, "fresh-id");

    expect(result?.primary).toEqual({ provider: "deepseek", base_url: "https://a.test", model: "model-a" });
    expect(result?.fallbacks[0]).toEqual({
      id: "fresh-id",
      label: null,
      provider: "openai",
      base_url: "https://primary.test/v1",
      model: "primary-model",
      enabled: true,
    });
    // 被降级的服务绝不能沿用被提升条目的标识，否则会直接读到别人的 API Key
    expect(result?.fallbacks[0].id).not.toBe("backup-a");
    expect(result?.fallbacks[1]).toBe(fallbacks[1]);
  });

  it("索引越界时不做任何交换", () => {
    expect(promoteFallbackToPrimary(primary, fallbacks, 5, "fresh-id")).toBeNull();
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
});

const primaryConfig: LlmConfig = {
  provider: "openai",
  base_url: "https://api.openai.com/v1",
  model: "gpt-test",
};

function Harness({
  initialFallbacks = [],
  onFallbacks,
  onRetry,
}: {
  initialFallbacks?: LlmProviderEntry[];
  onFallbacks?: (next: LlmProviderEntry[]) => void;
  onRetry?: (next: LlmRetryConfig) => void;
}) {
  const [config, setConfig] = useState<LlmConfig | null>(primaryConfig);
  const [fallbacks, setFallbacks] = useState<LlmProviderEntry[]>(initialFallbacks);
  const [retry, setRetry] = useState<LlmRetryConfig>({ network_retry_attempts: 2, retry_base_delay_ms: 500 });
  return (
    <LlmConfigPanel
      config={config}
      onChange={setConfig}
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
    // 主用标签只在存在备用服务时出现，单服务用户的观感与改动前一致
    expect(screen.queryByText("主用")).not.toBeInTheDocument();
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

    const attempts = await screen.findByLabelText("网络故障重试次数");
    expect(attempts).toHaveAttribute("aria-valuemin", "0");
    expect(attempts).toHaveAttribute("aria-valuemax", String(MAX_NETWORK_RETRY_ATTEMPTS));
    fireEvent.change(attempts, { target: { value: "4" } });
    await waitFor(() => expect(onRetry).toHaveBeenCalled());
    expect(lastCallArg(onRetry).network_retry_attempts).toBe(4);

    onRetry.mockClear();
    const delay = screen.getByLabelText("首次重试等待");
    expect(delay).toHaveAttribute("aria-valuemin", String(MIN_RETRY_BASE_DELAY_MS));
    expect(delay).toHaveAttribute("aria-valuemax", String(MAX_RETRY_BASE_DELAY_MS));
    fireEvent.change(delay, { target: { value: "800" } });
    await waitFor(() => expect(onRetry).toHaveBeenCalled());
    expect(lastCallArg(onRetry).retry_base_delay_ms).toBe(800);
  });
});
