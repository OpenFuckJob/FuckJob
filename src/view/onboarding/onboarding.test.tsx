import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppRuntimeConfig } from "@/types/app-config";
import { Onboarding } from ".";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((command: string) =>
    Promise.resolve(
      command === "set_llm_api_key"
        ? { success: true, data: { configured: true, source: "keychain" }, error: null }
        : {
            success: true,
            data: {
              browser_found: true,
              browser_name: "Chrome",
              browser_path: "/Chrome",
              user_data_dir: "/profile",
              user_data_dir_ok: true,
            },
            error: null,
          },
    ),
  ),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
}));
vi.mock("@/view/config/LlmConfigPanel", () => ({
  isValidLlmConfig: (value: AppRuntimeConfig["llm_config"]) =>
    Boolean(value?.base_url && value.model),
  LlmConfigPanel: ({
    onPendingApiKeyChange,
  }: {
    onPendingApiKeyChange?: (value: string) => void;
  }) => (
    <div>
      模型配置面板
      <button onClick={() => onPendingApiKeyChange?.("secret-key")}>输入密钥</button>
    </div>
  ),
}));

import { invoke } from "@tauri-apps/api/core";

const baseConfig = {
  schema_version: 1,
  onboarding_completed: false,
  llm_config: null,
  llm_fallbacks: [],
  job_filter_config: {
    query: null,
    city: null,
    job_type: 0,
    salary: 0,
    experience: [],
    dgree: [],
    industry: [],
    scale: [],
    stage: [],
    keywords: [],
    exclude_keywords: [],
    company_keywords: [],
    company_exclude_keywords: [],
    enable_semantic_filter: false,
    semantic_filter_intent: null,
    regex_rules: [],
  },
  resume_config: {
    inject_llm_context: false,
    resume_path: null,
    resume_content: null,
    liepin_attachment_resume_name: null,
    boss_attachment_resume_name: null,
  },
} as unknown as AppRuntimeConfig;

describe("Onboarding", () => {
  beforeEach(() => vi.clearAllMocks());
  afterEach(cleanup);

  it("guides the user through job intent, browser detection, and safety preset + optional AI", async () => {
    const onFinish = vi.fn().mockResolvedValue(true);
    render(<Onboarding config={baseConfig} onFinish={onFinish} />);

    expect(screen.getByText(/本地运行的自动投递与 HR 沟通管家/)).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText(/前端开发/), {
      target: { value: "前端开发" },
    });
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));

    await waitFor(() => expect(screen.getByText("已检测到 Chrome")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));

    expect(screen.getByText("保守防封")).toBeInTheDocument();
    expect(screen.getByText("标准防封")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "完成并开始使用" }));

    await waitFor(() =>
      expect(onFinish).toHaveBeenCalledWith(
        expect.objectContaining({
          onboarding_completed: true,
          llm_config: null,
          job_filter_config: expect.objectContaining({
            query: "前端开发",
            max_greets_per_company: 2,
          }),
          periodic_delivery_config: expect.objectContaining({
            max_greets_per_day: 40,
            max_greets_per_round: 25,
          }),
        }),
      ),
    );
  });

  it("can skip directly from step 1 without manufacturing an AI config", async () => {
    const onFinish = vi.fn().mockResolvedValue(true);
    render(<Onboarding config={baseConfig} onFinish={onFinish} />);
    fireEvent.click(screen.getByRole("button", { name: "先跳过，直接进入" }));

    await waitFor(() =>
      expect(onFinish).toHaveBeenCalledWith(
        expect.objectContaining({ onboarding_completed: true, llm_config: null }),
      ),
    );
  });

  it("drops hidden historical fallback drafts when skipping AI", async () => {
    const onFinish = vi.fn().mockResolvedValue(true);
    const historical = {
      ...baseConfig,
      llm_fallbacks: [
        {
          id: "backup-a",
          label: null,
          provider: "deepseek" as const,
          base_url: "https://api.deepseek.com",
          model: "",
          insecure: false,
          enabled: true,
        },
      ],
    };
    render(<Onboarding config={historical} onFinish={onFinish} />);
    fireEvent.click(screen.getByRole("button", { name: "先跳过，直接进入" }));

    await waitFor(() =>
      expect(onFinish).toHaveBeenCalledWith(
        expect.objectContaining({
          onboarding_completed: true,
          llm_config: null,
          llm_fallbacks: [],
        }),
      ),
    );
  });

  it("keeps complete hidden fallbacks and saves pending API key when finishing with AI", async () => {
    const onFinish = vi.fn().mockResolvedValue(true);
    const configured = {
      ...baseConfig,
      llm_config: {
        provider: "openai" as const,
        base_url: "https://api.openai.com/v1",
        model: "gpt-test",
        insecure: false,
      },
      llm_fallbacks: [
        {
          id: "backup-ok",
          label: null,
          provider: "deepseek" as const,
          base_url: "https://api.deepseek.com",
          model: "deepseek-chat",
          enabled: true,
          insecure: false,
        },
        {
          id: "backup-draft",
          label: null,
          provider: "deepseek" as const,
          base_url: "https://api.deepseek.com",
          model: "",
          enabled: true,
          insecure: false,
        },
      ],
    };
    render(<Onboarding config={configured} onFinish={onFinish} />);
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    await waitFor(() => expect(screen.getByText("已检测到 Chrome")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    fireEvent.click(screen.getByRole("button", { name: "输入密钥" }));
    fireEvent.click(screen.getByRole("button", { name: "完成并开始使用" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("set_llm_api_key", { apiKey: "secret-key" });
      expect(onFinish).toHaveBeenCalledWith(
        expect.objectContaining({
          onboarding_completed: true,
          llm_fallbacks: [configured.llm_fallbacks[0]],
        }),
      );
    });
  });

  it("shows a visible error when entering the app fails", async () => {
    const onFinish = vi.fn().mockResolvedValue(false);
    render(<Onboarding config={baseConfig} onFinish={onFinish} />);
    fireEvent.click(screen.getByRole("button", { name: "先跳过，直接进入" }));
    expect(await screen.findByText("配置保存失败，请检查配置后重试")).toBeInTheDocument();
  });
});
