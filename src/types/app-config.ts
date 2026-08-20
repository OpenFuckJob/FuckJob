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
  /** 单次模型请求的超时时间（秒） */
  request_timeout_seconds: number;
}

/** 与 Rust 侧 validate_and_normalize 保持一致的取值区间 */
export const MAX_NETWORK_RETRY_ATTEMPTS = 5;
export const MIN_RETRY_BASE_DELAY_MS = 100;
export const MAX_RETRY_BASE_DELAY_MS = 10_000;
export const DEFAULT_LLM_REQUEST_TIMEOUT_SECONDS = 120;
export const MIN_LLM_REQUEST_TIMEOUT_SECONDS = 1;
export const MAX_LLM_REQUEST_TIMEOUT_SECONDS = 600;

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

/** 一天的分钟数。投递时段用「零点起的分钟数」表示，才装得下 09:30 这种半点边界 */
export const MINUTES_PER_DAY = 24 * 60;
export const MAX_GREETS_PER_ROUND = 200;
export const MAX_ROUND_MINUTES = 240;
export const MAX_RUN_HOURS = 72;

/** 一段投递时段，以「零点起的分钟数」表示，左闭右开 */
export interface DailyWindow {
  start_minute: number;
  end_minute: number;
}

/**
 * 周期投递的默认参数。
 *
 * 存的是启动弹窗的初值，不是正在跑的任务的参数——任务一旦入队就带着自己的计划
 * 快照，之后改这里不影响它。和轮询节奏一样放在顶层：这是运行节奏，不是求职策略。
 */
export interface PeriodicDeliveryConfig {
  /** 两轮投递之间的间隔 */
  interval_minutes: number;
  /** 是否只在指定时段投递，关掉后全天可投 */
  window_enabled: boolean;
  /** 投递时段，可以有多段：上午一段、下午一段，中间的午休就空出来了 */
  windows: DailyWindow[];
  /** 启动后最多跑多少小时，0 表示不自动结束 */
  max_run_hours: number;
  /** 单轮最多打招呼多少条，0 表示不限。这是「一直在投递、回复轮不上」的正解 */
  max_greets_per_round: number;
  /** 单轮最长跑多少分钟，0 表示不限 */
  max_round_minutes: number;
}

/** 与 Rust 侧 PeriodicDeliveryConfig 的 serde 默认值保持一致 */
export const DEFAULT_PERIODIC_DELIVERY_CONFIG: PeriodicDeliveryConfig = {
  interval_minutes: 30,
  window_enabled: false,
  windows: [{ start_minute: 9 * 60, end_minute: 18 * 60 }],
  max_run_hours: 0,
  max_greets_per_round: 30,
  max_round_minutes: 60,
};

/** 时段格子的粒度：一格一小时 */
export const HOURS_PER_DAY = 24;

/**
 * 把时段列表摊成 24 个小时格的选中状态，供格子控件渲染。
 *
 * 一格代表 `[h:00, h+1:00)`，只要这一小时里有任何一分钟落在时段内就算选中。
 * 半点边界（09:30）因此会被向外取整——格子的粒度只到整点，这是它的取舍
 */
export function windowsToHours(windows: DailyWindow[]): boolean[] {
  const hours = Array.from({ length: HOURS_PER_DAY }, () => false);
  for (const window of windows) {
    const start = Math.max(0, Math.min(MINUTES_PER_DAY, window.start_minute));
    const end = Math.max(0, Math.min(MINUTES_PER_DAY, window.end_minute));
    // 跨零点的段（22:00-06:00）在格子上就是两头都亮，不需要单独的表示法
    const ranges =
      start < end
        ? [[start, end]]
        : start > end
          ? [
              [start, MINUTES_PER_DAY],
              [0, end],
            ]
          : [[0, MINUTES_PER_DAY]]; // 起止相同 = 全天
    for (const [from, to] of ranges) {
      for (let hour = Math.floor(from / 60); hour < Math.ceil(to / 60); hour += 1) {
        if (hour >= 0 && hour < HOURS_PER_DAY) hours[hour] = true;
      }
    }
  }
  return hours;
}

/**
 * 把 24 个小时格合并回时段列表：连续亮着的格子并成一段。
 *
 * 全选返回空数组——空在后端一律读作「不限制」，塞一个 00:00-24:00 进去
 * 只会让每一步判断都多绕一次
 */
export function hoursToWindows(hours: boolean[]): DailyWindow[] {
  const windows: DailyWindow[] = [];
  let start: number | null = null;
  for (let hour = 0; hour <= HOURS_PER_DAY; hour += 1) {
    const on = hour < HOURS_PER_DAY && hours[hour] === true;
    if (on && start === null) start = hour;
    if (!on && start !== null) {
      windows.push({ start_minute: start * 60, end_minute: hour * 60 });
      start = null;
    }
  }
  if (windows.length === 1 && windows[0].start_minute === 0 && windows[0].end_minute === MINUTES_PER_DAY) {
    return [];
  }
  return windows;
}

/**
 * 拟人化强度。只决定扰动幅度，不引入新的数值参数——
 * 休息阈值、停顿长度、打字速度全部由后端从既有配置派生。
 */
export type HumanizeIntensity = "light" | "standard" | "cautious";

/**
 * 拟人化。
 *
 * 平台风控看的不是单次动作像不像人，而是长期模式：每条投递都隔 4 秒、每轮都
 * 正好 30 条——单看每一步都合法，连起来是一条没有呼吸的直线。开启后后端会给
 * 既有的「单轮上限 / 投递间隔 / 停顿」蒙上一层扰动，用户设的量级不变。
 */
export interface HumanizeConfig {
  enabled: boolean;
  intensity: HumanizeIntensity;
  /**
   * 人格种子，由后端首次启用时生成。界面只读不改：改了等于换一个人，
   * 当天已经形成的节奏会整个变掉。
   *
   * 后端刻意把它限制在 2^53 以内，这个字段才能安全地经 JSON 往返
   */
  persona_seed: number;
}

export const DEFAULT_HUMANIZE_CONFIG: HumanizeConfig = {
  enabled: false,
  intensity: "standard",
  persona_seed: 0,
};

/** 读取拟人化配置，旧配置缺这块时回落到「关闭」。 */
export function getHumanizeConfig(config: Pick<AppRuntimeConfig, "humanize_config">): HumanizeConfig {
  return { ...DEFAULT_HUMANIZE_CONFIG, ...(config.humanize_config ?? {}) };
}

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
  /**
   * 主用大模型是否启用；旧配置缺这块时默认视为启用，停用时只改这个字段，不清除 llm_config
   */
  llm_enabled?: boolean;
  llm_fallbacks: LlmProviderEntry[];
  llm_retry_config: LlmRetryConfig;
  greet_config: GreetConfig;
  replay_config: ReplayConfig;
  analysis_config?: AnalysisConfig;
  /** 旧配置没有这块，读取时请使用 getReplyPollingConfig 兜底 */
  reply_polling_config?: ReplyPollingConfig;
  /** 旧配置没有这块，读取时请使用 getPeriodicDeliveryConfig 兜底 */
  periodic_delivery_config?: PeriodicDeliveryConfig;
  /** 旧配置没有这块，读取时请使用 getHumanizeConfig 兜底 */
  humanize_config?: HumanizeConfig;
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

/**
 * 一个大模型服务是否填写完整、可以真正发起调用。
 *
 * 配置页允许保存填了一半的服务（模型名要拉列表才知道，拉列表又得先存好密钥），
 * 所以「存下来了」不等于「能用」。与 Rust 侧的 `service_is_usable` 保持一致。
 */
export function isLlmServiceUsable(
  service: Pick<LlmConfig, "base_url" | "model"> | null | undefined,
): boolean {
  return Boolean(service?.base_url.trim() && service.model.trim());
}

/** 是否已经保存过主用大模型配置。填了一半也算，界面据此显示「继续配置」而不是「去配置」。 */
export function isLlmConfigured(config: Pick<AppRuntimeConfig, "llm_config">): boolean {
  return config.llm_config !== null;
}

/**
 * 主用大模型是否处于可用状态。
 *
 * 旧配置没有 `llm_enabled` 时默认按启用处理；没配置主用服务、
 * 或者服务还没填完时都视为不可用。
 */
export function isLlmActive(
  config: Pick<AppRuntimeConfig, "llm_config" | "llm_enabled">,
): boolean {
  return config.llm_enabled !== false && isLlmServiceUsable(config.llm_config);
}

/** 读取轮询节奏，旧配置缺这块时回落到默认节奏。 */
export function getReplyPollingConfig(config: Pick<AppRuntimeConfig, "reply_polling_config">): ReplyPollingConfig {
  return { ...DEFAULT_REPLY_POLLING_CONFIG, ...(config.reply_polling_config ?? {}) };
}

/** 读取周期投递默认参数，旧配置缺这块时回落到默认值。 */
export function getPeriodicDeliveryConfig(
  config: Pick<AppRuntimeConfig, "periodic_delivery_config">,
): PeriodicDeliveryConfig {
  return {
    ...DEFAULT_PERIODIC_DELIVERY_CONFIG,
    ...(config.periodic_delivery_config ?? {}),
  };
}

/**
 * 两份周期投递配置是否等价。
 *
 * 「恢复默认」按钮靠它决定该不该置灰。逐字段比而不是 JSON.stringify：后者依赖
 * 键顺序，两份内容相同、来源不同的对象会被判成不等，按钮于是永远亮着。
 * `windows` 是数组，必须按内容比——引用比较对它永远返回 false
 */
export function isSamePeriodicDelivery(
  left: PeriodicDeliveryConfig,
  right: PeriodicDeliveryConfig,
): boolean {
  return (
    left.interval_minutes === right.interval_minutes &&
    left.window_enabled === right.window_enabled &&
    left.max_run_hours === right.max_run_hours &&
    left.max_greets_per_round === right.max_greets_per_round &&
    left.max_round_minutes === right.max_round_minutes &&
    isSameWindows(left.windows, right.windows)
  );
}

function isSameWindows(left: DailyWindow[], right: DailyWindow[]): boolean {
  return (
    left.length === right.length &&
    left.every(
      (window, index) =>
        window.start_minute === right[index].start_minute &&
        window.end_minute === right[index].end_minute,
    )
  );
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
