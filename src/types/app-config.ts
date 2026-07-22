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
  | "rules";

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
  | "ollama"
  | "lm_studio"
  | "openai"
  | "deepseek"
  | "dashscope"
  | "custom";

export interface LlmConfig {
  provider: LlmProviderPreset;
  base_url: string;
  model: string;
}

export type ReplayResourceType = "Text" | "Image" | "LLM";

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
  default_template: ReplyResource[];
}

export interface ReplayConfig {
  enable_auto_replay: boolean;
  templates: ReplyTemplate[];
  enable_llm: boolean;
  reply_prompt: string | null;
  background_context: string | null;
}

export interface BrowserConfig {
  user_data_dir: string;
  chrome_exe_path: string | null;
}

export interface ResumeConfig {
  inject_llm_context: boolean;
  resume_path: string | null;
  resume_content: string | null;
}

export interface AppRuntimeConfig {
  schema_version: number;
  onboarding_completed: boolean;
  job_filter_config: JobFilterConfig;
  platform_filter_config: PlatformFilterConfig;
  llm_config: LlmConfig | null;
  greet_config: GreetConfig;
  replay_config: ReplayConfig;
  browser_config: BrowserConfig;
  resume_config: ResumeConfig;
}

export type StatusKind = "idle" | "loading" | "saved" | "error";
