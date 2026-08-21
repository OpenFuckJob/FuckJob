import type { AppRuntimeConfig, JobProfile } from "@/types/app-config";
import type { AgentTrace, RoundTrace } from "@/types/playground";

/** 测试夹具：只给测试用，生产代码不引用它 */

export function makeProfile(overrides: Partial<JobProfile> = {}): JobProfile {
  return {
    id: "p1",
    name: "后端方案",
    archived: false,
    job_filter_config: {
      query: null, city: null, job_type: 0, salary: 0, experience: [], dgree: [], industry: [],
      scale: [], stage: [], keywords: [], exclude_keywords: [], company_keywords: [],
      company_exclude_keywords: [], enable_semantic_filter: true,
      semantic_filter_intent: "只要后端岗", regex_rules: [],
    },
    platform_filter_config: {
      boss: { active_filter_enabled: true, active_threshold: "this_week" },
      liepin: { dq: null, salary_code: null, pub_time: null, work_year_code: null, comp_tag: [] },
    },
    resume_config: { inject_llm_context: false, resume_path: null, resume_content: null },
    greet_config: { enable_llm: true, reply_prompt: "原打招呼提示词", default_template: [] },
    replay_config: {
      enable_template_reply: false, templates: [], enable_llm: true, reply_prompt: "原回复提示词",
      background_context: null, enable_auto_send_resume: true, max_auto_replies: 5,
      max_reply_chars: 200, dry_run: false,
    },
    ...overrides,
  };
}

export function makeConfig(profiles: JobProfile[] = [makeProfile()]): AppRuntimeConfig {
  const base = profiles[0];
  return {
    schema_version: 1,
    onboarding_completed: true,
    llm_config: { provider: "openai", base_url: "https://example.test", model: "gpt-x" },
    llm_enabled: true,
    llm_fallbacks: [],
    llm_retry_config: { network_retry_attempts: 1, retry_base_delay_ms: 200, request_timeout_seconds: 120 },
    job_filter_config: base.job_filter_config,
    platform_filter_config: base.platform_filter_config,
    greet_config: base.greet_config,
    replay_config: base.replay_config,
    browser_config: { user_data_dir: "profile", chrome_exe_path: null, max_parallel_tasks: 2 },
    resume_config: base.resume_config,
    job_profiles: profiles,
    default_job_profile_id: base.id,
  };
}

export function makeRound(overrides: Partial<RoundTrace> = {}): RoundTrace {
  return {
    round: 1,
    prompt: "系统提示词",
    raw: "{\"action\":\"send\"}",
    model: "gpt-x",
    usage: { prompt_tokens: 120, completion_tokens: 30, total_tokens: 150 },
    duration_ms: 800,
    verdict: { kind: "passed" },
    ...overrides,
  };
}

export function makeTrace(overrides: Partial<AgentTrace> = {}): AgentTrace {
  return {
    id: "t1",
    task_name: "打招呼决策",
    started_at: "2026-08-20T10:00:00+08:00",
    duration_ms: 1400,
    rounds: [makeRound()],
    stop: "first_try",
    error: null,
    ...overrides,
  };
}
