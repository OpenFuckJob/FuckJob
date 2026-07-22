import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppRuntimeConfig } from "@/types/app-config";
import { useAppConfig } from "./useAppConfig";

vi.mock("@/lib/tauriConfig", () => ({
  loadAppConfig: vi.fn(),
  saveAppConfig: vi.fn(),
  importAppConfig: vi.fn(),
  exportAppConfig: vi.fn(),
}));

import * as api from "@/lib/tauriConfig";

const config: AppRuntimeConfig = {
  schema_version: 1,
  onboarding_completed: false,
  llm_config: null,
  job_filter_config: { query: null, city: null, job_type: 0, salary: 0, experience: [], dgree: [], industry: [], scale: [], stage: [], keywords: [], exclude_keywords: [], company_keywords: [], company_exclude_keywords: [], regex_rules: [] },
  platform_filter_config: { liepin: { dq: null, salary_code: null, pub_time: null, work_year_code: null, comp_tag: [] } },
  greet_config: { enable_llm: false, reply_prompt: null, default_template: [] },
  replay_config: { enable_auto_replay: false, templates: [], enable_llm: false, reply_prompt: null, background_context: null },
  browser_config: { user_data_dir: "profile", chrome_exe_path: null },
  resume_config: { inject_llm_context: false, resume_path: null, resume_content: null },
};

describe("useAppConfig", () => {
  beforeEach(() => vi.resetAllMocks());

  it("loads the initial config and preserves a null LLM config", async () => {
    vi.mocked(api.loadAppConfig).mockResolvedValue(config);
    const { result } = renderHook(() => useAppConfig());
    expect(result.current.status).toBe("loading");
    await waitFor(() => expect(result.current.status).toBe("idle"));
    expect(result.current.config?.llm_config).toBeNull();
  });

  it("exposes a recoverable load error", async () => {
    vi.mocked(api.loadAppConfig).mockRejectedValue(new Error("配置损坏"));
    const { result } = renderHook(() => useAppConfig());
    await waitFor(() => expect(result.current.status).toBe("error"));
    expect(result.current.message).toBe("配置损坏");
  });

  it("updates nested config immutably", async () => {
    vi.mocked(api.loadAppConfig).mockResolvedValue(config);
    const { result } = renderHook(() => useAppConfig());
    await waitFor(() => expect(result.current.config).not.toBeNull());
    const original = result.current.config;
    act(() => result.current.updateConfig((current) => ({ ...current, browser_config: { ...current.browser_config, user_data_dir: "next" } })));
    expect(result.current.config).not.toBe(original);
    expect(original?.browser_config.user_data_dir).toBe("profile");
    expect(result.current.config?.browser_config.user_data_dir).toBe("next");
  });

  it("reports save success and failure", async () => {
    vi.mocked(api.loadAppConfig).mockResolvedValue(config);
    vi.mocked(api.saveAppConfig).mockResolvedValue();
    const { result } = renderHook(() => useAppConfig());
    await waitFor(() => expect(result.current.config).not.toBeNull());
    await act(() => result.current.save());
    expect(result.current.status).toBe("saved");
    vi.mocked(api.saveAppConfig).mockRejectedValueOnce(new Error("无法保存"));
    await act(() => result.current.save());
    expect(result.current.status).toBe("error");
    expect(result.current.message).toBe("无法保存");
  });
});
