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
  llm_fallbacks: [],
  llm_retry_config: { network_retry_attempts: 2, retry_base_delay_ms: 500, request_timeout_seconds: 120 },
  job_filter_config: { query: null, city: null, job_type: 0, salary: 0, experience: [], dgree: [], industry: [], scale: [], stage: [], keywords: [], exclude_keywords: [], company_keywords: [], company_exclude_keywords: [], enable_semantic_filter: false, semantic_filter_intent: null, regex_rules: [] },
  platform_filter_config: { boss: { active_filter_enabled: true, active_threshold: "this_week", exclude_headhunter_jobs: false }, liepin: { dq: null, salary_code: null, pub_time: null, work_year_code: null, comp_tag: [] } },
  greet_config: { enable_llm: false, reply_prompt: null, default_template: [] },
  replay_config: { enable_template_reply: false, templates: [], enable_llm: false, reply_prompt: null, background_context: null, enable_auto_send_resume: true, max_auto_replies: 5, max_reply_chars: 200, dry_run: false },
  browser_config: { user_data_dir: "profile", chrome_exe_path: null, max_parallel_tasks: 2 },
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

  it("preserves newer edits made while an earlier version is being saved", async () => {
    vi.mocked(api.loadAppConfig).mockResolvedValue(config);
    let finishSave: (() => void) | undefined;
    vi.mocked(api.saveAppConfig).mockImplementation((submitted) => new Promise<AppRuntimeConfig>((resolve) => {
      finishSave = () => resolve(submitted);
    }));
    const { result } = renderHook(() => useAppConfig());
    await waitFor(() => expect(result.current.config).not.toBeNull());

    act(() => result.current.updateConfig((current) => ({
      ...current,
      browser_config: { ...current.browser_config, user_data_dir: "first" },
    })));
    await waitFor(() => expect(result.current.dirty).toBe(true));
    let saving: Promise<boolean>;
    act(() => { saving = result.current.save(); });
    await waitFor(() => expect(result.current.status).toBe("loading"));

    act(() => result.current.updateConfig((current) => ({
      ...current,
      browser_config: { ...current.browser_config, user_data_dir: "second" },
    })));
    await act(async () => {
      finishSave?.();
      await saving!;
    });

    expect(result.current.config?.browser_config.user_data_dir).toBe("second");
    expect(result.current.dirty).toBe(true);
  });

  it("applies an explicitly saved onboarding config to the live state", async () => {
    vi.mocked(api.loadAppConfig).mockResolvedValue(config);
    vi.mocked(api.saveAppConfig).mockImplementation(async (submitted) => submitted);
    const { result } = renderHook(() => useAppConfig());
    await waitFor(() => expect(result.current.config).not.toBeNull());

    const completed = { ...config, onboarding_completed: true };
    await act(() => result.current.save(completed));

    expect(result.current.config?.onboarding_completed).toBe(true);
    expect(result.current.dirty).toBe(false);
  });

  it("reports save success and failure", async () => {
    vi.mocked(api.loadAppConfig).mockResolvedValue(config);
    vi.mocked(api.saveAppConfig).mockImplementation(async (submitted) => submitted);
    const { result } = renderHook(() => useAppConfig());
    await waitFor(() => expect(result.current.config).not.toBeNull());
    await act(() => result.current.save());
    expect(result.current.status).toBe("saved");
    vi.mocked(api.saveAppConfig).mockRejectedValueOnce(new Error("无法保存"));
    await act(() => result.current.save());
    expect(result.current.status).toBe("error");
    expect(result.current.message).toBe("无法保存");
  });

  /**
   * 后端在保存路径上会做迁移、夹取上下界、补生成拟人化的人格种子，落盘内容
   * 和提交内容并不相同。拿提交的那份当已保存快照的话，下次保存又原样提交一遍——
   * 对夹取类字段只是显示不同步，对人格种子则是每存一次换一套人格
   */
  it("adopts the normalized config the backend actually wrote", async () => {
    vi.mocked(api.loadAppConfig).mockResolvedValue(config);
    const normalized: AppRuntimeConfig = {
      ...config,
      humanize_config: { enabled: true, intensity: "standard", persona_seed: 8_123_456_789 },
    };
    vi.mocked(api.saveAppConfig).mockResolvedValue(normalized);
    const { result } = renderHook(() => useAppConfig());
    await waitFor(() => expect(result.current.config).not.toBeNull());

    await act(() => result.current.save({
      ...config,
      humanize_config: { enabled: true, intensity: "standard", persona_seed: 0 },
    }));

    expect(result.current.config?.humanize_config?.persona_seed).toBe(8_123_456_789);
    // 已保存快照也必须换成落盘的那份，否则界面立刻显示为「未保存」
    expect(result.current.dirty).toBe(false);
  });
});
