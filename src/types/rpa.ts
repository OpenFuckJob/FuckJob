import type { DailyWindow, PeriodicDeliveryConfig } from "./app-config";

export type { DailyWindow };

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
  | "periodic_job_hunting"
  | "polling_reply";

/** 永不自然结束、只能手工停止的模式。同一平台同时只允许存在一个 */
export function isLongRunningMode(mode: FlowMode): boolean {
  return mode === "periodic_job_hunting" || mode === "polling_reply";
}

export type JobTaskState =
  | "queued"
  | "starting"
  | "running"
  | "stopping"
  | "succeeded"
  | "failed"
  | "cancelled";

/**
 * 一次周期投递任务的计划。任务入队时固定下来，之后改配置不影响它。
 *
 * 除 `interval_minutes` 外全部可选，只带间隔时等价于改造前的行为：
 * 无时段限制、不自动结束、单轮不设上界。
 */
export interface PeriodicPlan {
  interval_minutes: number;
  /** 每日投递时段，可以有多段。空或缺省表示全天可投 */
  windows?: DailyWindow[];
  /** RFC3339 本地时刻，到点整个任务收工 */
  run_until?: string | null;
  /** 单轮最多打招呼多少条，0 表示不限 */
  max_greets_per_round?: number;
  /** 单轮最长跑多少分钟，0 表示不限 */
  max_round_minutes?: number;
}

export interface JobTaskInfo {
  task_id: string;
  platform: PlatformKind;
  mode: FlowMode;
  status: JobTaskState;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  error: string | null;
  /** 求职任务绑定的方案快照；回复未读等路由任务为空。 */
  profile_id?: string | null;
  profile_name?: string | null;
  profile_snapshot_id?: string | null;
  /** 周期投递任务的计划快照；其他模式为空。 */
  plan?: PeriodicPlan | null;
}

/**
 * 把表单里的一组数值拧成提交给后端的计划。
 *
 * 「最长运行 N 小时」在这里换算成绝对的结束时刻：表单存的是「下次也这么跑」的
 * 模板，存死某个钟点隔天就过期了；而任务一旦跑起来，判断该不该收工只能靠绝对时刻。
 * 关掉的开关一律落成 null / 0，让后端按「不限制」处理，而不是塞一个装作没启用的值
 */
export function buildPeriodicPlan(
  config: PeriodicDeliveryConfig,
  now: Date = new Date(),
): PeriodicPlan {
  return {
    interval_minutes: config.interval_minutes,
    windows: config.window_enabled ? config.windows : [],
    run_until:
      config.max_run_hours > 0
        ? new Date(now.getTime() + config.max_run_hours * 3_600_000).toISOString()
        : null,
    max_greets_per_round: config.max_greets_per_round,
    max_round_minutes: config.max_round_minutes,
  };
}

/** 把「零点起的分钟数」格式化成 HH:MM */
export function formatMinuteOfDay(minute: number): string {
  const clamped = Math.max(0, Math.min(24 * 60, Math.round(minute)));
  return `${String(Math.floor(clamped / 60)).padStart(2, "0")}:${String(clamped % 60).padStart(2, "0")}`;
}

/** 多段时段的可读描述，例如 `09:00-12:00、14:00-18:00` */
export function describeWindows(windows: DailyWindow[]): string {
  if (windows.length === 0) return "全天";
  return windows
    .map((window) => `${formatMinuteOfDay(window.start_minute)}-${formatMinuteOfDay(window.end_minute)}`)
    .join("、");
}

/**
 * 表单形态的一行摘要，配置折叠起来时替它说话。
 *
 * 与 [describePeriodicPlan] 的差别只在自动结束：这里还没启动，说「8 小时后结束」
 * 才对得上用户刚拖的那个滑块；换算成绝对时刻要等任务真的入队。
 */
export function describePeriodicDelivery(config: PeriodicDeliveryConfig): string {
  const parts = [`每 ${config.interval_minutes} 分钟一轮`];

  if (config.window_enabled && config.windows.length > 0) {
    parts.push(`${describeWindows(config.windows)} 投递`);
  }
  if (config.max_run_hours > 0) {
    parts.push(`${config.max_run_hours} 小时后结束`);
  }
  if (config.max_greets_per_round) {
    parts.push(`单轮至多 ${config.max_greets_per_round} 条`);
  }
  if (config.max_round_minutes) {
    parts.push(`单轮至多 ${config.max_round_minutes} 分钟`);
  }

  return parts.join(" · ");
}

/**
 * 周期投递计划的一行摘要，任务卡片用。
 *
 * 只说约束，不说默认：没设时段就不提时段。把「全天 · 不自动结束 · 不限条数」
 * 一并铺出来，真正被限制住的那几项反而看不见了。
 */
export function describePeriodicPlan(plan: PeriodicPlan): string {
  const parts = [`每 ${plan.interval_minutes} 分钟一轮`];

  if (plan.windows && plan.windows.length > 0) {
    parts.push(`${describeWindows(plan.windows)} 投递`);
  }
  if (plan.run_until) {
    const end = new Date(plan.run_until);
    if (!Number.isNaN(end.getTime())) {
      parts.push(
        `至 ${String(end.getMonth() + 1).padStart(2, "0")}-${String(end.getDate()).padStart(2, "0")} ${String(end.getHours()).padStart(2, "0")}:${String(end.getMinutes()).padStart(2, "0")} 结束`,
      );
    }
  }
  if (plan.max_greets_per_round) {
    parts.push(`单轮至多 ${plan.max_greets_per_round} 条`);
  }
  if (plan.max_round_minutes) {
    parts.push(`单轮至多 ${plan.max_round_minutes} 分钟`);
  }

  return parts.join(" · ");
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
