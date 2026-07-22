export type PlatformKind = "boss" | "liepin";

export type EnvCheckStep = "browser" | "platform_login" | "completed";
export type EnvCheckStatus = "login_required" | "completed";

export interface EnvCheckResult {
  platform: PlatformKind;
  current_step: EnvCheckStep;
  status: EnvCheckStatus;
  qr_code_base64: string | null;
  message: string;
}

export type FlowMode =
  | "job_hunting"
  | "reply_unread"
  | "periodic_job_hunting";

export interface JobTaskStatus {
  running: boolean;
}

export interface BrowserEnvStatus {
  browser_found: boolean;
  browser_name: string | null;
  browser_path: string | null;
  user_data_dir_ok: boolean;
  user_data_dir: string | null;
}

export type ReadinessLevel = "ready" | "warning" | "blocked";
export interface ReadinessItem {
  key: string;
  label: string;
  level: ReadinessLevel;
  message: string;
  config_group: string | null;
}
export interface ReadinessReport {
  ready: boolean;
  platform: PlatformKind;
  mode: FlowMode;
  items: ReadinessItem[];
  summary: string[];
}
