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
  | "sync_chat_history"
  | "periodic_job_hunting";

export type JobTaskState =
  | "queued"
  | "starting"
  | "running"
  | "stopping"
  | "succeeded"
  | "failed"
  | "cancelled";

export interface JobTaskInfo {
  task_id: string;
  platform: PlatformKind;
  mode: FlowMode;
  status: JobTaskState;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  error: string | null;
}

export interface JobTaskOverview {
  tasks: JobTaskInfo[];
  running_count: number;
  queued_count: number;
  max_parallel_tasks: number;
}

export const ACTIVE_JOB_TASK_STATES: ReadonlySet<JobTaskState> = new Set([
  "queued",
  "starting",
  "running",
  "stopping",
]);

export const RUNNING_JOB_TASK_STATES: ReadonlySet<JobTaskState> = new Set([
  "starting",
  "running",
  "stopping",
]);

export function isActiveJobTask(task: Pick<JobTaskInfo, "status">): boolean {
  return ACTIVE_JOB_TASK_STATES.has(task.status);
}

export function getActiveTaskForPlatform(
  tasks: JobTaskInfo[],
  platform: PlatformKind,
): JobTaskInfo | null {
  return tasks.find(
    (task) => task.platform === platform && isActiveJobTask(task),
  ) ?? null;
}

export function countJobTasks(tasks: JobTaskInfo[]): {
  active: number;
  running: number;
  queued: number;
} {
  return tasks.reduce(
    (counts, task) => {
      if (isActiveJobTask(task)) counts.active += 1;
      if (RUNNING_JOB_TASK_STATES.has(task.status)) counts.running += 1;
      if (task.status === "queued") counts.queued += 1;
      return counts;
    },
    { active: 0, running: 0, queued: 0 },
  );
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
