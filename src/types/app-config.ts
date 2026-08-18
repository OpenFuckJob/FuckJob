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
  | "analysis"
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
  /** 只管正则模板这一条路径，不是自动回复的总开关（旧名 enable_auto_replay） */
  enable_template_reply: boolean;
  templates: ReplyTemplate[];
  enable_llm: boolean;
  reply_prompt: string | null;
  background_context: string | null;
  /** 关掉后模型仍会判断投递时机，但只回消息，投递交回人工 */
  enable_auto_send_resume: boolean;
  /** 时间窗内单会话最多自动回复几条，用完挂起转人工 */
  max_auto_replies: number;
  /** 上限所依据的滚动时间窗长度，窗口滑走后额度自动恢复 */
  auto_reply_window_hours?: number;
  /** 超长的求职消息本身就不像真人写的 */
  max_reply_chars: number;
  /** 演练模式：判断与生成照常，但不实际发送，只写日志 */
  dry_run: boolean;
}

/** 与 Rust 侧 ReplayConfig 的 serde 默认值保持一致 */
export const DEFAULT_MAX_AUTO_REPLIES = 5;
export const DEFAULT_AUTO_REPLY_WINDOW_HOURS = 24;
export const DEFAULT_MAX_REPLY_CHARS = 200;
/** 新建正则模板时匹配最近多少条聊天 */
export const DEFAULT_REGEX_RULE_LIMIT = 5;

/** 自动触发岗位分析的时机，一次只能生效一个 */
export type AnalysisTrigger = "off" | "filter_passed" | "greet_sent" | "reply_received";

export interface AnalysisConfig {
  trigger: AnalysisTrigger;
  /** 已有分析结果的岗位不再重复分析 */
  skip_analyzed: boolean;
  /** 单个求职任务内最多自动分析多少个岗位，0 表示不限制 */
  max_per_task: number;
  /** 达到该匹配分才算高匹配岗位，求职数据概览按这个口径统计 */
  high_match_score: number;
}

/** 与 Rust 侧 AnalysisConfig 的 serde 默认值保持一致 */
export const DEFAULT_HIGH_MATCH_SCORE = 80;
export const MIN_HIGH_MATCH_SCORE = 50;
export const DEFAULT_MAX_ANALYSIS_PER_TASK = 20;
export const MAX_ANALYSIS_PER_TASK = 500;

export const DEFAULT_ANALYSIS_CONFIG: AnalysisConfig = {
  trigger: "off",
  skip_analyzed: true,
  max_per_task: DEFAULT_MAX_ANALYSIS_PER_TASK,
  high_match_score: DEFAULT_HIGH_MATCH_SCORE,
};

/**
 * 自动回复的轮询节奏。全局唯一，不随求职方案变化——
 * 一轮轮询会跨越多个岗位、命中多张方案卡，节奏却只能有一套。
 */
export interface ReplyPollingConfig {
  /** 两轮回复之间的基础间隔 */
  interval_minutes: number;
  /** 在基础间隔上叠加的随机抖动上限，精确到秒的固定节律本身就是机器特征 */
  jitter_seconds: number;
  active_hours_enabled: boolean;
  /** 活跃时段起止（0-23，左闭右开）。只约束回复，不约束投递 */
  active_start_hour: number;
  active_end_hour: number;
  /** 单轮最多处理多少个会话，超出的留给下一轮 */
  max_conversations_per_round: number;
  /** 「对方发出消息到我方回复」的目标间隔，轮询间隔本身通常已经填满 */
  humanize_delay_min_seconds: number;
  humanize_delay_max_seconds: number;
}

/** 与 Rust 侧 ReplyPollingConfig 的 serde 默认值保持一致 */
export const DEFAULT_REPLY_POLLING_CONFIG: ReplyPollingConfig = {
  interval_minutes: 5,
  jitter_seconds: 120,
  active_hours_enabled: true,
  active_start_hour: 9,
  active_end_hour: 22,
  max_conversations_per_round: 10,
  humanize_delay_min_seconds: 30,
  humanize_delay_max_seconds: 120,
};

export interface BrowserConfig {
  user_data_dir: string;
  chrome_exe_path: string | null;
  max_parallel_tasks: number;
}

/** BOSS 与猎聘可各跑一个任务，再多就排队 */
export const DEFAULT_MAX_PARALLEL_TASKS = 2;

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
  /** 旧配置没有这块，读取时请使用 getAnalysisConfig 兜底 */
  analysis_config?: AnalysisConfig;
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
  analysis_config?: AnalysisConfig;
  /** 旧配置没有这块，读取时请使用 getReplyPollingConfig 兜底 */
  reply_polling_config?: ReplyPollingConfig;
  browser_config: BrowserConfig;
  resume_config: ResumeConfig;
  /** 旧配置/测试 mock 可能暂时不包含这两个字段，读取时请使用 getJobProfiles。 */
  job_profiles?: JobProfile[];
  default_job_profile_id?: string;
}

/** 读取分析配置，旧配置缺这块时回落到「不自动分析」。 */
export function getAnalysisConfig(
  source: Pick<AppRuntimeConfig, "analysis_config"> | Pick<JobProfile, "analysis_config">,
): AnalysisConfig {
  return { ...DEFAULT_ANALYSIS_CONFIG, ...(source.analysis_config ?? {}) };
}

/** 读取轮询节奏，旧配置缺这块时回落到默认节奏。 */
export function getReplyPollingConfig(config: Pick<AppRuntimeConfig, "reply_polling_config">): ReplyPollingConfig {
  return { ...DEFAULT_REPLY_POLLING_CONFIG, ...(config.reply_polling_config ?? {}) };
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
    analysis_config: getAnalysisConfig(config),
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
