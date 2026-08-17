export type MatchTarget = "Title" | "Company" | "Description" | "All";
export type RuleMode = "ACCEPT" | "REJECT";
export type AppPage =
  | "workspace"
  | "config"
  | "practice"
  | "resumeOptimizer"
  | "interviewPrep";
export type ConfigGroup =
  | "job"
  | "llm"
  | "greet"
  | "reply"
  | "browser"
  | "resume"
  | "rules"
  | "data"
  | "about";

export interface RegexRule {
  name: string;
  pattern: string;
  target: MatchTarget;
  mode: RuleMode;
}

export interface JobFilterConfig {
  query: string | null;
  city: number | null;
  job_type: number;
  salary: number;
  experience: number[];
  dgree: number[];
  industry: number[];
  scale: number[];
  stage: number[];
  keywords: string[];
  exclude_keywords: string[];
  company_keywords: string[];
  company_exclude_keywords: string[];
  enable_semantic_filter: boolean;
  semantic_filter_intent: string | null;
  regex_rules: RegexRule[];
}

export interface LiepinFilterConfig {
  dq: string | null;
  salary_code: string | null;
  pub_time: string | null;
  work_year_code: string | null;
  comp_tag: string[];
}

export interface PlatformFilterConfig {
  liepin: LiepinFilterConfig;
}

export type LlmProviderPreset =
  | "anthropic"
  | "deepseek"
  | "openai"
  | "openai_responses"
  | "minimax"
  | "moonshot"
  | "ollama"
  | "openrouter"
  | "xiaomi_mimo"
  | "zai";

export interface LlmConfig {
  provider: LlmProviderPreset;
  base_url: string;
  model: string;
}

/** 主用服务在降级链中的保留标识，其 API Key 沿用旧的存储条目 */
export const PRIMARY_LLM_ENTRY_ID = "primary";

/** 降级链中的一个备用大模型服务 */
export interface LlmProviderEntry {
  /** 稳定标识，用于关联独立存储的 API Key；重排序时不得变化 */
  id: string;
  /** 展示名称，为空时界面回退到模型名 */
  label: string | null;
  provider: LlmProviderPreset;
  base_url: string;
  model: string;
  /** 是否参与降级链 */
  enabled: boolean;
}

/** 大模型调用的重试策略 */
export interface LlmRetryConfig {
  /** 网络类瞬时故障的额外重试次数（不含首次请求），0 表示不重试 */
  network_retry_attempts: number;
  /** 首次重试前的等待毫秒数，之后按指数退避 */
  retry_base_delay_ms: number;
}

/** 与 Rust 侧 validate_and_normalize 保持一致的取值区间 */
export const MAX_NETWORK_RETRY_ATTEMPTS = 5;
export const MIN_RETRY_BASE_DELAY_MS = 100;
export const MAX_RETRY_BASE_DELAY_MS = 10_000;

export type ReplayResourceType = "Text" | "Image" | "LLM";

export interface GreetResource {
  /** 是否参与发送；旧配置缺失时视为启用 */
  enabled?: boolean;
  resource_type: ReplayResourceType;
  content: string;
}

export interface ReplyResource {
  resource_type: ReplayResourceType;
  content: string;
}

export interface ReplyRegexRule {
  name: string;
  pattern: string;
  limit: number;
}

export interface ReplyTemplate {
  regex_rule: ReplyRegexRule;
  content: ReplyResource[];
}

export interface GreetConfig {
  enable_llm: boolean;
  reply_prompt: string | null;
  default_template: GreetResource[];
}

export interface ReplayConfig {
  enable_auto_replay: boolean;
  templates: ReplyTemplate[];
  enable_llm: boolean;
  reply_prompt: string | null;
  background_context: string | null;
  /** 关掉后模型仍会判断投递时机，但只回消息，投递交回人工 */
  enable_auto_send_resume: boolean;
  /** 没有上限时模型会和 HR 无限客套下去 */
  max_auto_replies: number;
  /** 超长的求职消息本身就不像真人写的 */
  max_reply_chars: number;
  /** 演练模式：判断与生成照常，但不实际发送，只写日志 */
  dry_run: boolean;
}

/** 与 Rust 侧 ReplayConfig 的 serde 默认值保持一致 */
export const DEFAULT_MAX_AUTO_REPLIES = 5;
export const DEFAULT_MAX_REPLY_CHARS = 200;

export interface BrowserConfig {
  user_data_dir: string;
  chrome_exe_path: string | null;
  max_parallel_tasks: number;
}

export interface ResumeConfig {
  inject_llm_context: boolean;
  resume_path: string | null;
  resume_content: string | null;
}

/** 一套可独立执行的求职方向、简历与沟通策略。 */
export interface JobProfile {
  id: string;
  name: string;
  description?: string | null;
  archived: boolean;
  job_filter_config: JobFilterConfig;
  platform_filter_config: PlatformFilterConfig;
  resume_config: ResumeConfig;
  greet_config: GreetConfig;
  replay_config: ReplayConfig;
}

export interface AppRuntimeConfig {
  schema_version: number;
  onboarding_completed: boolean;
  job_filter_config: JobFilterConfig;
  platform_filter_config: PlatformFilterConfig;
  llm_config: LlmConfig | null;
  llm_fallbacks: LlmProviderEntry[];
  llm_retry_config: LlmRetryConfig;
  greet_config: GreetConfig;
  replay_config: ReplayConfig;
  browser_config: BrowserConfig;
  resume_config: ResumeConfig;
  /** 旧配置/测试 mock 可能暂时不包含这两个字段，读取时请使用 getJobProfiles。 */
  job_profiles?: JobProfile[];
  default_job_profile_id?: string;
}

/** 把旧版顶层求职配置投影为默认方案，供迁移期 UI 安全读取。 */
export function getJobProfiles(config: AppRuntimeConfig): JobProfile[] {
  if (config.job_profiles?.length) return config.job_profiles;
  return [{
    id: config.default_job_profile_id || "default",
    name: "默认求职方案",
    description: "由原有求职配置自动生成",
    archived: false,
    job_filter_config: config.job_filter_config,
    platform_filter_config: config.platform_filter_config,
    resume_config: config.resume_config,
    greet_config: config.greet_config,
    replay_config: config.replay_config,
  }];
}

export function getDefaultJobProfile(config: AppRuntimeConfig): JobProfile {
  const profiles = getJobProfiles(config);
  return profiles.find((profile) => profile.id === config.default_job_profile_id && !profile.archived)
    ?? profiles.find((profile) => !profile.archived)
    ?? profiles[0];
}

export function copyJobProfile(source: JobProfile, id: string, suffix: string): JobProfile {
  return {
    ...structuredClone(source),
    id,
    name: `${source.name} · ${suffix}`,
    archived: false,
  };
}

export function selectProfileAfterRemoval(profiles: JobProfile[], removedId: string): JobProfile | null {
  return profiles.find((profile) => profile.id !== removedId && !profile.archived)
    ?? profiles.find((profile) => profile.id !== removedId)
    ?? null;
}

export type StatusKind = "idle" | "loading" | "saved" | "error";
