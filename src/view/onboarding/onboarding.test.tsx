import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppRuntimeConfig } from "@/types/app-config";
import { Onboarding } from ".";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((command: string) => Promise.resolve(command === "set_llm_api_key"
    ? { success: true, data: { configured: true, source: "keychain" }, error: null }
    : { success: true, data: { browser_found: true, browser_name: "Chrome", browser_path: "/Chrome", user_data_dir: "/profile", user_data_dir_ok: true }, error: null })),
}));
vi.mock("@/view/config/LlmConfigPanel", () => ({
  isValidLlmConfig: (value: AppRuntimeConfig["llm_config"]) => Boolean(value?.base_url && value.model),
  LlmConfigPanel: ({ onPendingApiKeyChange }: { onPendingApiKeyChange?: (value: string) => void }) => (
    <div>
      模型配置面板
      <button onClick={() => onPendingApiKeyChange?.("secret-key")}>输入密钥</button>
    </div>
  ),
}));

import { invoke } from "@tauri-apps/api/core";

const config = { schema_version: 1, onboarding_completed: false, llm_config: null } as AppRuntimeConfig;

describe("Onboarding", () => {
  beforeEach(() => vi.clearAllMocks());
  afterEach(cleanup);

  it("shows privacy and browser detection before optional AI setup", async () => {
    render(<Onboarding config={config} onFinish={vi.fn()} />);
    expect(screen.getByText(/数据默认保存在本机/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /继\s*续/ }));
    await waitFor(() => expect(screen.getByText("已检测到 Chrome")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "继续配置" }));
    expect(screen.getByText("模型配置面板")).toBeInTheDocument();
  });

  it("can skip AI without manufacturing a config", async () => {
    const onFinish = vi.fn().mockResolvedValue(true);
    render(<Onboarding config={config} onFinish={onFinish} />);
    fireEvent.click(screen.getByRole("button", { name: "跳过 AI，进入应用" }));
    await waitFor(() => expect(onFinish).toHaveBeenCalledWith(expect.objectContaining({ onboarding_completed: true, llm_config: null })));
  });

  it("automatically saves a pending API key before entering the app", async () => {
    const configured = {
      ...config,
      llm_config: { provider: "openai" as const, base_url: "https://api.openai.com/v1", model: "gpt-test" },
    };
    const onFinish = vi.fn().mockResolvedValue(true);
    render(<Onboarding config={configured} onFinish={onFinish} />);
    fireEvent.click(screen.getByRole("button", { name: /继\s*续/ }));
    await waitFor(() => expect(screen.getByText("已检测到 Chrome")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "继续配置" }));
    fireEvent.click(screen.getByRole("button", { name: "输入密钥" }));
    fireEvent.click(screen.getByRole("button", { name: "完成并进入应用" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("set_llm_api_key", { apiKey: "secret-key" }));
    expect(onFinish).toHaveBeenCalledWith(expect.objectContaining({ onboarding_completed: true }));
  });

  it("shows a visible error when entering the app fails", async () => {
    const onFinish = vi.fn().mockResolvedValue(false);
    render(<Onboarding config={config} onFinish={onFinish} />);
    fireEvent.click(screen.getByRole("button", { name: "跳过 AI，进入应用" }));
    expect(await screen.findByText("配置保存失败，请检查配置后重试")).toBeInTheDocument();
  });
});
