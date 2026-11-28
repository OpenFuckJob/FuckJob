import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Drawer,
  Image,
  Input,
  Modal,
  Radio,
  Select,
  Segmented,
  Space,
  Steps,
  Tag,
  Typography,
  message,
} from "antd";
import {
  CopyOutlined,
  EyeInvisibleOutlined,
  EyeOutlined,
  FilterOutlined,
  LoadingOutlined,
  SearchOutlined,
  SettingOutlined,
  UndoOutlined,
  WarningOutlined,
} from "@ant-design/icons";
import {
  IconBellAlert,
  IconBrowserCheck,
  IconCalendarSpark,
  IconClockRepeat,
  IconPlaySolid,
  IconReplyPulse,
  IconResumeDoc,
  IconShieldCheck,
  IconSliders,
  IconTarget,
  IconTerminal,
  OfferFlowLogo,
  PlatformBadgeIcon,
} from "../../components/icons";
import { invoke } from "@tauri-apps/api/core";
import type { CommandResult } from "../../types/command";
import { commandErrorMessage } from "../../types/command";
import type {
  EnvCheckResult,
  EnvCheckStep,
  EnvCheckStatus,
  FlowMode,
  JobTaskInfo,
  JobTaskOverview,
  PlatformKind,
  ReadinessReport,
  TodayGreetCounts,
} from "../../types/rpa";
import {
  buildPeriodicPlan,
  countJobTasks,
  describePeriodicDelivery,
  describePeriodicPlan,
  isActiveJobTask,
} from "../../types/rpa";
import type { PeriodicPlan } from "../../types/rpa";
import type { JobListItem, JobPipelineStage } from "../../types/job-detail";
import { useJobDataRefresh } from "../../hooks/useJobDataRefresh";
import ManualReviewDrawer, { useManualReview } from "./manual-review-drawer";
import {
  DEFAULT_MAX_GREETS_PER_DAY,
  getDefaultJobProfile,
  DEFAULT_MAX_PARALLEL_TASKS,
  getJobProfiles,
  getPeriodicDeliveryConfig,
  getReplyPollingConfig,
  isLlmActive,
  type AppRuntimeConfig,
  type ConfigGroup,
  type GreetConfig,
  type PeriodicDeliveryConfig,
  type ReplyPollingConfig,
} from "../../types/app-config";
import type { ConfigViewMode } from "../config";
import { getCityLabel } from "../../lib/constants";
import { describeSafetySummary } from "../../lib/safety-presets";
import PeriodicDeliverySection, {
  canResetPeriodicDelivery,
} from "../config/PeriodicDeliverySection";
import "./style.css";

type CheckPhase = "idle" | "checking" | "done";

interface StepInfo {
  title: string;
  status: EnvCheckStep;
}

interface PlatformEnvState {
  phase: CheckPhase;
  result: EnvCheckResult | null;
  message: string;
}

type QueueFilter = "all" | PlatformKind;

interface PlatformMeta {
  label: string;
  shortLabel: string;
  accent: string;
  supportsChat: boolean;
}

interface FlowModeOption {
  key: FlowMode;
  label: string;
  description: string;
}

const ENV_STEPS: StepInfo[] = [
  { title: "浏览器环境", status: "browser" },
  { title: "登录状态", status: "platform_login" },
  { title: "检查完成", status: "completed" },
];

function getStepIndex(current: EnvCheckStep): number {
  return ENV_STEPS.findIndex((s) => s.status === current);
}

export function resolveStepStatus(
  phase: CheckPhase,
  envStatus: EnvCheckStatus | null,
  stepIndex: number,
  currentStepIndex: number,
  hasError = false,
): "wait" | "process" | "finish" | "error" {
  if (phase === "idle") return "wait";
  if (phase === "checking") {
    if (stepIndex < currentStepIndex) return "finish";
    if (stepIndex === currentStepIndex) return "process";
    return "wait";
  }
  if (stepIndex < currentStepIndex) return "finish";
  if (stepIndex === currentStepIndex) {
    if (envStatus === "login_required" || (!envStatus && hasError)) {
      return "error";
    }
    return "finish";
  }
  return "wait";
}

export function isGreetConfigReady(
  config: Pick<AppRuntimeConfig, "llm_config" | "llm_enabled">,
  greet: GreetConfig,
): boolean {
  const llmResourceReady =
    greet.enable_llm && isLlmActive(config) && Boolean(greet.reply_prompt?.trim());
  const hasEnabledLlm = greet.default_template.some(
    (item) => item.enabled !== false && item.resource_type === "LLM",
  );
  if (greet.enable_llm && hasEnabledLlm && !llmResourceReady) {
    return false;
  }
  return greet.default_template.some((item) => {
    if (item.enabled === false) return false;
    if (item.resource_type === "LLM") return llmResourceReady;
    return item.content.trim().length > 0;
  });
}

export function resolveWorkspaceEffectiveStage(job: JobListItem): JobPipelineStage {
  if (job.effective_stage) return job.effective_stage;
  if (job.pipeline_stage) return job.pipeline_stage;
  if (job.communication_status === "rejected") return "rejected";
  if (job.communication_status === "interview_invited") return "interview_scheduled";
  if (job.communication_status === "replied" || job.is_reply || (job.unread_count ?? 0) > 0) {
    return "replied";
  }
  if (job.is_send_resume) return "applied";
  return "pending";
}

const PLATFORM_META: Record<PlatformKind, PlatformMeta> = {
  boss: {
    label: "BOSS 直聘",
    shortLabel: "BOSS",
    accent: "#00a6a6",
    supportsChat: true,
  },
  liepin: {
    label: "猎聘",
    shortLabel: "猎聘",
    accent: "#7c3aed",
    supportsChat: true,
  },
  "51job": {
    label: "前程无忧",
    shortLabel: "51job",
    accent: "#f97316",
    supportsChat: false,
  },
};

const FLOW_MODE_OPTIONS: FlowModeOption[] = [
  {
    key: "job_hunting",
    label: "单轮自动投递",
    description: "按当前求职意向筛选一轮岗位并自动打招呼，完成后停止。",
  },
  {
    key: "periodic_job_hunting",
    label: "周期自动投递",
    description: "按防封节奏分批投递，间隔期间自动处理 HR 未读回复。",
  },
  {
    key: "reply_unread",
    label: "回复未读消息",
    description: "立刻扫描并回复当前平台里 HR 发来的未读消息。",
  },
  {
    key: "polling_reply",
    label: "持续盯回复",
    description: "只盯未读消息并自动回复，不新增投递。",
  },
  {
    key: "sync_chat_history",
    label: "零点击同步状态",
    description: "仅拉取沟通列表、未读摘要与约面信号，不点开会话、保留未读红点。",
  },
];

function getFlowModeLabel(mode: FlowMode): string {
  return FLOW_MODE_OPTIONS.find((option) => option.key === mode)?.label ?? "自动投递";
}

export function filterTaskLogContent(content: string, taskId: string | null): string {
  if (!content.trim() || !taskId) {
    return content;
  }

  return content
    .split("\n")
    .filter((line) => line.includes(`[task=${taskId}]`))
    .join("\n");
}

interface StatTile {
  label: string;
  value: string;
  subtitle?: string;
  color: string;
  bg: string;
}

/**
 * 「待你处理」磁贴的外观。
 * 0 条时保持安静灰，有待接手会话时立刻转成醒目红色提示。
 */
export function describePendingReview(
  count: number,
): Pick<StatTile, "value" | "subtitle" | "color" | "bg"> {
  const urgent = count > 0;
  return {
    value: `${count}`,
    subtitle: urgent ? "点击查看并接手" : "AI 都自己处理了",
    color: urgent ? "#ef4444" : "#64748b",
    bg: urgent ? "rgba(239,68,68,0.1)" : "rgba(148,163,184,0.1)",
  };
}

export function getTaskProfileLabel(
  task: Pick<JobTaskInfo, "mode" | "profile_id" | "profile_name">,
): string {
  if ((task.mode === "reply_unread" || task.mode === "polling_reply") && !task.profile_id) {
    return "按会话方案自动路由";
  }
  if (task.mode === "sync_chat_history" && !task.profile_id) return "不使用求职方案";
  return task.profile_name || "默认求职方案";
}

const TASK_STATE_ORDER: Record<JobTaskInfo["status"], number> = {
  running: 0,
  starting: 0,
  stopping: 0,
  queued: 1,
  failed: 2,
  cancelled: 2,
  succeeded: 2,
};

const PLATFORM_ORDER: PlatformKind[] = ["boss", "liepin", "51job"];

export function sortTasksForQueue(tasks: JobTaskInfo[]): JobTaskInfo[] {
  return [...tasks].sort((left, right) => {
    const stateOrder = TASK_STATE_ORDER[left.status] - TASK_STATE_ORDER[right.status];
    if (stateOrder !== 0) return stateOrder;
    if (left.status === "queued" && right.status === "queued") {
      return left.created_at.localeCompare(right.created_at);
    }
    return right.created_at.localeCompare(left.created_at);
  });
}

export function sortTasksNewestFirst(tasks: JobTaskInfo[]): JobTaskInfo[] {
  return [...tasks].sort((left, right) => right.created_at.localeCompare(left.created_at));
}

export function filterTasksByPlatform(tasks: JobTaskInfo[], filter: QueueFilter): JobTaskInfo[] {
  return filter === "all" ? tasks : tasks.filter((task) => task.platform === filter);
}

function taskStateLabel(status: JobTaskInfo["status"]): string {
  return {
    queued: "排队中",
    starting: "启动中",
    running: "运行中",
    stopping: "停止中",
    succeeded: "已完成",
    failed: "失败",
    cancelled: "已取消",
  }[status];
}

function taskStateColor(status: JobTaskInfo["status"]): string {
  return {
    queued: "default",
    starting: "processing",
    running: "processing",
    stopping: "warning",
    succeeded: "success",
    failed: "error",
    cancelled: "default",
  }[status];
}

export function describePeriodicBlocker(
  config: Pick<PeriodicDeliveryConfig, "interval_minutes" | "window_enabled" | "windows">,
): string | null {
  if (config.interval_minutes <= 0) return "投递间隔必须大于 0 分钟";
  if (config.window_enabled && config.windows.length === 0) {
    return "投递时段一格都没选，任务不会投递";
  }
  return null;
}

export type LogLineLevel = "error" | "warn" | "success" | "info";

export function parseLogLineLevel(line: string): LogLineLevel {
  if (/\[ERROR\]|失败|风控|熔断|异常|断开/.test(line)) return "error";
  if (/\[WARN\]|\[WARNING\]|警告|超时|跳过|转人工|限额|上限/.test(line)) return "warn";
  if (/初次沟通成功|通过|已加入任务队列|完成/.test(line)) return "success";
  return "info";
}

export function filterLogLines(
  content: string,
  taskId: string | null,
  levelFilter: "all" | "warn_error",
  keyword: string,
): string[] {
  const scoped = filterTaskLogContent(content, taskId);
  if (!scoped.trim()) return [];
  const normalizedKeyword = keyword.trim().toLowerCase();
  return scoped.split("\n").filter((line) => {
    if (!line.trim()) return false;
    if (levelFilter === "warn_error") {
      const level = parseLogLineLevel(line);
      if (level !== "error" && level !== "warn") return false;
    }
    if (normalizedKeyword && !line.toLowerCase().includes(normalizedKeyword)) {
      return false;
    }
    return true;
  });
}

export function describeReplyActiveHours(
  polling: Pick<ReplyPollingConfig, "active_hours_enabled" | "active_start_hour" | "active_end_hour">,
): string {
  if (!polling.active_hours_enabled || polling.active_start_hour === polling.active_end_hour) {
    return "全天回复";
  }
  return `只在 ${polling.active_start_hour}:00 - ${polling.active_end_hour}:00 之间回复`;
}

export function requestStopJobTask(taskId: string): Promise<CommandResult<void>> {
  return invoke<CommandResult<void>>("stop_job_task", { taskId });
}

/* ────────── Component ────────── */

const WorkspacePage = ({
  config,
  onNavigate,
  onOpenConfig,
  onOpenConversation,
  pendingReviewCount: externalPendingReviewCount,
  onOpenManualReview,
}: {
  config: AppRuntimeConfig;
  onNavigate?: (tab: "job-data") => void;
  onOpenConfig?: (group: ConfigGroup, mode?: ConfigViewMode) => void;
  onOpenConversation?: (jobId: string) => void;
  pendingReviewCount?: number;
  onOpenManualReview?: () => void;
}) => {
  const [environmentStates, setEnvironmentStates] = useState<
    Record<PlatformKind, PlatformEnvState>
  >({
    boss: { phase: "idle", result: null, message: "" },
    liepin: { phase: "idle", result: null, message: "" },
    "51job": { phase: "idle", result: null, message: "" },
  });
  const [taskOverview, setTaskOverview] = useState<JobTaskOverview>({
    tasks: [],
    running_count: 0,
    queued_count: 0,
    max_parallel_tasks: DEFAULT_MAX_PARALLEL_TASKS,
  });
  const [logContent, setLogContent] = useState("");
  const [startModalOpen, setStartModalOpen] = useState(false);
  const [selectedMode, setSelectedMode] = useState<FlowMode>("job_hunting");
  const [periodicDraft, setPeriodicDraft] = useState<PeriodicDeliveryConfig>(() =>
    getPeriodicDeliveryConfig(config),
  );
  const [paramsDrawerOpen, setParamsDrawerOpen] = useState(false);
  const [selectedProfileId, setSelectedProfileId] = useState<string>(
    () => getDefaultJobProfile(config).id,
  );
  const [platform, setPlatform] = useState<PlatformKind>("boss");
  const [reviewDrawerOpen, setReviewDrawerOpen] = useState(false);
  const usesExternalReview = onOpenManualReview !== undefined;
  const manualReview = useManualReview(!usesExternalReview);
  const [modalPlatform, setModalPlatform] = useState<PlatformKind>("boss");
  const [pendingStartPlatforms, setPendingStartPlatforms] = useState<
    Partial<Record<PlatformKind, boolean>>
  >({});
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [queueFilter, setQueueFilter] = useState<QueueFilter>("all");
  const [jobs, setJobs] = useState<JobListItem[]>([]);
  const [todayCounts, setTodayCounts] = useState<TodayGreetCounts>({
    boss: 0,
    liepin: 0,
    job51: 0,
  });
  const [readinessReport, setReadinessReport] = useState<ReadinessReport | null>(null);
  const [readinessLoading, setReadinessLoading] = useState(false);
  const [logCollapsed, setLogCollapsed] = useState(true);
  const [logLevelFilter, setLogLevelFilter] = useState<"all" | "warn_error">("all");
  const [logSearch, setLogSearch] = useState("");
  const logCollapsedRef = useRef(logCollapsed);
  logCollapsedRef.current = logCollapsed;
  const logRef = useRef<HTMLPreElement>(null);
  const previousActiveTaskIdsRef = useRef<Set<string>>(new Set());
  const taskRefreshPromiseRef = useRef<Promise<void> | null>(null);
  const [messageApi, contextHolder] = message.useMessage();

  const availableProfiles = getJobProfiles(config).filter((profile) => !profile.archived);
  const activeProfile =
    availableProfiles.find((profile) => profile.id === selectedProfileId) ??
    getDefaultJobProfile(config);

  const loadJobs = useCallback(async () => {
    try {
      const [jobsResult, countsResult] = await Promise.all([
        invoke<CommandResult<JobListItem[]>>("job_list_with_status"),
        invoke<CommandResult<TodayGreetCounts>>("job_today_greet_counts").catch(() => null),
      ]);
      if (jobsResult.success && jobsResult.data) {
        setJobs(jobsResult.data);
      }
      if (countsResult?.success && countsResult.data) {
        setTodayCounts(countsResult.data);
      }
    } catch {
      // ignore
    }
  }, []);

  useJobDataRefresh(loadJobs);

  const refreshTaskStatus = useCallback((): Promise<void> => {
    if (taskRefreshPromiseRef.current) return taskRefreshPromiseRef.current;

    const request = (async () => {
      try {
        const result = await invoke<CommandResult<JobTaskOverview>>("get_job_task_status");
        if (result.success && result.data) {
          const nextActiveIds = new Set(
            result.data.tasks.filter(isActiveJobTask).map((task) => task.task_id),
          );
          const taskFinished = [...previousActiveTaskIdsRef.current].some(
            (taskId) => !nextActiveIds.has(taskId),
          );
          previousActiveTaskIdsRef.current = nextActiveIds;
          setTaskOverview(result.data);
          if (taskFinished) await loadJobs();
        }
      } catch {
        // ignore transient errors
      }
    })();

    taskRefreshPromiseRef.current = request;
    void request.then(() => {
      if (taskRefreshPromiseRef.current === request) {
        taskRefreshPromiseRef.current = null;
      }
    });
    return request;
  }, [loadJobs]);

  const refreshLog = useCallback(async () => {
    try {
      const result = await invoke<CommandResult<string>>("read_log_file", { lines: 500 });
      if (result.success && result.data !== null) {
        setLogContent(result.data);
      }
    } catch {
      // ignore
    }
  }, []);

  useEffect(() => {
    void loadJobs();
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      if (logCollapsedRef.current) {
        await refreshTaskStatus();
      } else {
        await Promise.all([refreshLog(), refreshTaskStatus()]);
      }
      if (!disposed) timer = setTimeout(() => void poll(), 2000);
    };
    void poll();
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
    };
  }, [loadJobs, refreshLog, refreshTaskStatus]);

  useEffect(() => {
    if (!logCollapsed) void refreshLog();
  }, [logCollapsed, refreshLog]);

  useEffect(() => {
    if (!startModalOpen) {
      setReadinessReport(null);
      return;
    }
    let stale = false;
    setReadinessLoading(true);
    const needsProfile =
      selectedMode === "job_hunting" || selectedMode === "periodic_job_hunting";
    void invoke<CommandResult<ReadinessReport>>("get_readiness_report", {
      platform: modalPlatform,
      mode: selectedMode,
      profileId: needsProfile ? activeProfile.id : undefined,
    })
      .then((result) => {
        if (!stale && result.success && result.data) {
          setReadinessReport(result.data);
        }
      })
      .catch(() => {
        if (!stale) setReadinessReport(null);
      })
      .finally(() => {
        if (!stale) setReadinessLoading(false);
      });
    return () => {
      stale = true;
    };
  }, [activeProfile.id, modalPlatform, selectedMode, startModalOpen]);

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logContent]);

  const handleCheckEnv = useCallback(
    async (targetPlatform: PlatformKind) => {
      setEnvironmentStates((current) => ({
        ...current,
        [targetPlatform]: { phase: "checking", result: null, message: "" },
      }));
      try {
        const result = await invoke<CommandResult<EnvCheckResult>>("check_env", {
          platform: targetPlatform,
        });
        if (!result.success || result.data === null) {
          const errorMessage = commandErrorMessage(result.error, "环境检查失败");
          setEnvironmentStates((current) => ({
            ...current,
            [targetPlatform]: { phase: "done", result: null, message: errorMessage },
          }));
          messageApi.error(errorMessage);
          return;
        }
        setEnvironmentStates((current) => ({
          ...current,
          [targetPlatform]: { phase: "done", result: result.data, message: "" },
        }));
        if (result.data.status === "completed") {
          messageApi.success(`${PLATFORM_META[targetPlatform].label} 登录与浏览器已就绪`);
        }
      } catch (error: unknown) {
        const msg = error instanceof Error ? error.message : "环境检查异常";
        setEnvironmentStates((current) => ({
          ...current,
          [targetPlatform]: { phase: "done", result: null, message: msg },
        }));
        messageApi.error(msg);
      }
    },
    [messageApi],
  );

  const handleRpaFlow = useCallback(
    async (
      startedPlatform: PlatformKind,
      mode: FlowMode,
      plan?: PeriodicPlan,
      profileId?: string,
    ) => {
      setPendingStartPlatforms((current) => ({
        ...current,
        [startedPlatform]: true,
      }));
      try {
        const result = await invoke<CommandResult<JobTaskInfo>>("start_job_task", {
          platform: startedPlatform,
          mode,
          plan: mode === "periodic_job_hunting" ? plan : undefined,
          profileId:
            mode === "job_hunting" || mode === "periodic_job_hunting" ? profileId : undefined,
        });
        if (!result.success || !result.data) {
          messageApi.error(commandErrorMessage(result.error, "启动失败"));
          return;
        }
        await refreshTaskStatus();
        messageApi.success(
          `${PLATFORM_META[startedPlatform].label} ${getFlowModeLabel(mode)}已开始运行`,
        );
      } catch (error: unknown) {
        messageApi.error(error instanceof Error ? error.message : "启动失败");
      } finally {
        setPendingStartPlatforms((current) => ({
          ...current,
          [startedPlatform]: false,
        }));
      }
    },
    [messageApi, refreshTaskStatus],
  );

  const openStartModal = useCallback(
    (targetPlatform: PlatformKind, defaultMode: FlowMode = "job_hunting") => {
      setModalPlatform(targetPlatform);
      setSelectedMode(defaultMode);
      setPeriodicDraft(getPeriodicDeliveryConfig(config));
      setParamsDrawerOpen(false);
      setStartModalOpen(true);
    },
    [config],
  );

  const closeParamsDrawer = useCallback(() => setParamsDrawerOpen(false), []);
  const closeStartModal = useCallback(() => {
    setStartModalOpen(false);
    setParamsDrawerOpen(false);
  }, []);

  const handleStartConfirm = useCallback(async () => {
    closeStartModal();
    await handleRpaFlow(
      modalPlatform,
      selectedMode,
      selectedMode === "periodic_job_hunting" ? buildPeriodicPlan(periodicDraft) : undefined,
      selectedMode === "job_hunting" || selectedMode === "periodic_job_hunting"
        ? activeProfile.id
        : undefined,
    );
  }, [
    activeProfile.id,
    closeStartModal,
    handleRpaFlow,
    modalPlatform,
    periodicDraft,
    selectedMode,
  ]);

  const handleStopTask = useCallback(
    async (taskId: string) => {
      try {
        const result = await requestStopJobTask(taskId);
        if (!result.success) {
          messageApi.error(commandErrorMessage(result.error, "停止失败"));
          return;
        }
        await refreshTaskStatus();
        messageApi.success("已发送停止请求");
      } catch (error: unknown) {
        messageApi.error(error instanceof Error ? error.message : "停止失败");
      }
    },
    [messageApi, refreshTaskStatus],
  );

  const handleCopyLog = useCallback(() => {
    if (!logContent) return;
    void navigator.clipboard.writeText(filterTaskLogContent(logContent, selectedTaskId));
    messageApi.success("日志已复制到剪贴板");
  }, [logContent, messageApi, selectedTaskId]);

  const activeTasks = taskOverview.tasks.filter(isActiveJobTask);
  const taskCounts = countJobTasks(taskOverview.tasks);
  const modalTaskStartPending = pendingStartPlatforms[modalPlatform] === true;
  const queueTasks = sortTasksNewestFirst(taskOverview.tasks);
  const visibleQueueTasks = filterTasksByPlatform(queueTasks, queueFilter);
  const queuedTasksInExecutionOrder = sortTasksForQueue(taskOverview.tasks).filter(
    (task) => task.status === "queued",
  );
  const currentPlatformMeta = PLATFORM_META[platform];
  const currentEnvironment = environmentStates[platform];
  const currentPlatformHasActiveTask = activeTasks.some((task) => task.platform === platform);
  const currentStartPending = pendingStartPlatforms[platform] === true;
  const currentStepIndex = currentEnvironment.result
    ? getStepIndex(currentEnvironment.result.current_step)
    : 0;
  const currentQrSrc = currentEnvironment.result?.qr_code_base64
    ? `data:image/png;base64,${currentEnvironment.result.qr_code_base64}`
    : null;
  const selectedTask = selectedTaskId
    ? taskOverview.tasks.find((task) => task.task_id === selectedTaskId) ?? null
    : null;
  const filteredLogLines = filterLogLines(logContent, selectedTaskId, logLevelFilter, logSearch);

  /* ── Derived metrics for top status strip ── */
  const totalJobs = jobs.length;
  const repliedCount = jobs.filter((j) => {
    const stage = resolveWorkspaceEffectiveStage(j);
    return (
      j.is_reply ||
      j.communication_status === "replied" ||
      j.communication_status === "interview_invited" ||
      (j.communication_status === "rejected" && j.latest_message_received === true) ||
      (j.unread_count ?? 0) > 0 ||
      stage === "replied" ||
      stage === "interview_scheduled" ||
      stage === "interviewing" ||
      stage === "offer"
    );
  }).length;
  const replyRate = totalJobs > 0 ? `${((repliedCount / totalJobs) * 100).toFixed(0)}%` : "—";
  const interviewCount = jobs.filter((j) => {
    const stage = resolveWorkspaceEffectiveStage(j);
    return stage === "interview_scheduled" || stage === "interviewing" || stage === "offer";
  }).length;
  const offerCount = jobs.filter((j) => resolveWorkspaceEffectiveStage(j) === "offer").length;
  const pendingReviewCount = externalPendingReviewCount ?? manualReview.records.length;
  const pendingReviewUi = describePendingReview(pendingReviewCount);

  const dailyLimit =
    getPeriodicDeliveryConfig(config).max_greets_per_day ?? DEFAULT_MAX_GREETS_PER_DAY;
  const todayUsed =
    platform === "boss"
      ? todayCounts.boss
      : platform === "liepin"
        ? todayCounts.liepin
        : todayCounts.job51;
  const quotaPct = dailyLimit > 0 ? Math.min(100, Math.round((todayUsed / dailyLimit) * 100)) : 0;
  const quotaColor =
    dailyLimit > 0 && todayUsed >= dailyLimit
      ? "#ef4444"
      : dailyLimit > 0 && todayUsed >= Math.ceil(dailyLimit * 0.8)
        ? "#f59e0b"
        : "#10b981";

  /* ── Readiness lamps for the active profile ── */
  const filter = activeProfile.job_filter_config;
  const queryText = (filter.query ?? "").trim();
  const hasIntent = Boolean(queryText);
  const hasResume = Boolean((activeProfile.resume_config.resume_content ?? "").trim());
  const greetReady = isGreetConfigReady(config, activeProfile.greet_config);
  const envReady = currentEnvironment.result?.status === "completed";
  const envNeedsLogin = currentEnvironment.result?.status === "login_required";
  const greetModeLabel = activeProfile.greet_config.enable_llm
    ? isLlmActive(config)
      ? "AI 定制打招呼"
      : "AI 打招呼（待配模型）"
    : "固定话术打招呼";

  const periodicBlocker =
    selectedMode === "periodic_job_hunting" ? describePeriodicBlocker(periodicDraft) : null;

  const summaryRow = (label: string, content: React.ReactNode) => (
    <>
      <Typography.Text type="secondary" style={{ fontSize: 13 }}>
        {label}
      </Typography.Text>
      <div style={{ minWidth: 0 }}>{content}</div>
    </>
  );

  const profileRow =
    availableProfiles.length > 1
      ? summaryRow(
          "求职方案",
          <Select
            aria-label="本次使用的求职方案"
            style={{ width: "100%" }}
            value={activeProfile.id}
            onChange={setSelectedProfileId}
            options={availableProfiles.map((profile) => ({
              value: profile.id,
              label: `${profile.name}${profile.id === (config.default_job_profile_id || getDefaultJobProfile(config).id) ? "（默认）" : ""}`,
            }))}
          />,
        )
      : null;

  const renderModeSummary = (mode: FlowMode): React.ReactNode => {
    const polling = getReplyPollingConfig(config);
    switch (mode) {
      case "job_hunting":
        return profileRow;
      case "periodic_job_hunting":
        return (
          <>
            {profileRow}
            {summaryRow(
              "运行节奏",
              <div>
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 12,
                    justifyContent: "space-between",
                  }}
                >
                  <Typography.Text style={{ fontSize: 12 }}>
                    {describePeriodicDelivery(periodicDraft)}
                  </Typography.Text>
                  <Button
                    size="small"
                    icon={<IconSliders size={13} />}
                    onClick={() => setParamsDrawerOpen(true)}
                  >
                    微调本次参数
                  </Button>
                </div>
                {periodicBlocker && (
                  <Typography.Text
                    style={{ display: "block", marginTop: 4, fontSize: 12, color: "#d97706" }}
                  >
                    {periodicBlocker}，请先修正后再启动
                  </Typography.Text>
                )}
              </div>,
            )}
          </>
        );
      case "reply_unread":
        return summaryRow(
          "自动回复",
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            逐条检查当前平台的未读消息，并按已设置的规则或 AI 回复。
          </Typography.Text>,
        );
      case "polling_reply":
        return summaryRow(
          "轮询节奏",
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            每 {polling.interval_minutes} 分钟检查一次新消息，{describeReplyActiveHours(polling)}。
          </Typography.Text>,
        );
      case "sync_chat_history":
        return null;
    }
  };

  const modeSummary = renderModeSummary(selectedMode);

  return (
    <div className="ws-root">
      {contextHolder}

      {/* ── Top compact daily status strip ── */}
      <section className="ws-strip">
        <button
          type="button"
          className={`ws-strip-card${onOpenConfig ? " is-clickable" : ""}`}
          onClick={() => onOpenConfig?.("runtime", "basic")}
          title="点击调整每日防封安全档位"
        >
          <span
            className="ws-strip-icon"
            style={{ background: "rgba(16,185,129,0.12)", color: quotaColor }}
          >
            <IconShieldCheck size={19} />
          </span>
          <span className="ws-strip-meta">
            <span className="ws-strip-label">{currentPlatformMeta.shortLabel} 今日安全额度</span>
            <span className="ws-strip-value">
              {todayUsed}
              {dailyLimit > 0 ? ` / ${dailyLimit}` : " 条"}
            </span>
            {dailyLimit > 0 ? (
              <span className="ws-quota-bar" aria-hidden="true">
                <span
                  className="ws-quota-fill"
                  style={{ width: `${quotaPct}%`, background: quotaColor }}
                />
              </span>
            ) : (
              <span className="ws-strip-sub">未设日上限</span>
            )}
          </span>
        </button>

        <button
          type="button"
          className={`ws-strip-card${onNavigate ? " is-clickable" : ""}`}
          onClick={() => onNavigate?.("job-data")}
        >
          <span
            className="ws-strip-icon"
            style={{ background: "var(--of-accent-soft)", color: "var(--of-accent)" }}
          >
            <IconReplyPulse size={19} />
          </span>
          <span className="ws-strip-meta">
            <span className="ws-strip-label">HR 已回复 / 已建联</span>
            <span className="ws-strip-value">
              {repliedCount} / {totalJobs}
            </span>
            <span className="ws-strip-sub">回复率 {replyRate} · 点击查看沟通</span>
          </span>
        </button>

        <button
          type="button"
          className={`ws-strip-card${onNavigate ? " is-clickable" : ""}`}
          onClick={() => onNavigate?.("job-data")}
        >
          <span
            className="ws-strip-icon"
            style={{ background: "rgba(139,92,246,0.12)", color: "#8b5cf6" }}
          >
            <IconCalendarSpark size={19} />
          </span>
          <span className="ws-strip-meta">
            <span className="ws-strip-label">约面与 Offer</span>
            <span className="ws-strip-value">{interviewCount}</span>
            <span className="ws-strip-sub">
              {offerCount > 0 ? `其中 Offer ${offerCount} 个` : "推进阶段在「沟通与进度」管理"}
            </span>
          </span>
        </button>

        <button
          type="button"
          className={`ws-strip-card is-clickable${pendingReviewCount > 0 ? " is-urgent" : ""}`}
          onClick={() => {
            if (onOpenManualReview) onOpenManualReview();
            else setReviewDrawerOpen(true);
          }}
        >
          <span
            className="ws-strip-icon"
            style={{ background: pendingReviewUi.bg, color: pendingReviewUi.color }}
          >
            <IconBellAlert size={19} />
          </span>
          <span className="ws-strip-meta">
            <span className="ws-strip-label">待你接手</span>
            <span
              className="ws-strip-value"
              style={{ color: pendingReviewCount > 0 ? pendingReviewUi.color : undefined }}
            >
              {pendingReviewUi.value}
            </span>
            <span className="ws-strip-sub">{pendingReviewUi.subtitle}</span>
          </span>
        </button>
      </section>

      {/* ── Main focus card + live queue ── */}
      <section className="ws-main-grid">
        <Card
          styles={{ body: { padding: 20, height: "100%", display: "flex", flexDirection: "column" } }}
          style={
            {
              "--platform-accent": currentPlatformMeta.accent,
            } as React.CSSProperties
          }
        >
          <Space direction="vertical" size={16} style={{ width: "100%", flex: 1 }}>
            {/* Bespoke platform selector with custom SVG badges */}
            <div className="ws-platform-tabs" role="tablist" aria-label="选择招聘平台">
              {PLATFORM_ORDER.map((value) => {
                const meta = PLATFORM_META[value];
                const active = platform === value;
                return (
                  <button
                    key={value}
                    type="button"
                    role="tab"
                    aria-selected={active}
                    className={`ws-platform-btn${active ? " is-active" : ""}`}
                    style={{ "--platform-accent": meta.accent } as React.CSSProperties}
                    onClick={() => setPlatform(value)}
                  >
                    <PlatformBadgeIcon platform={value} size={19} />
                    <span>{meta.label}</span>
                  </button>
                );
              })}
            </div>

            {/* Goal summary card */}
            <div className="ws-goal-card">
              <div className="ws-goal-top">
                <div style={{ minWidth: 0 }}>
                  <div className="ws-goal-title">
                    {queryText ? `目标岗位：${queryText}` : "尚未填写目标岗位关键词"}
                  </div>
                  <div className="ws-goal-tags" style={{ marginTop: 7 }}>
                    <span className="ws-goal-chip">{getCityLabel(filter.city)}</span>
                    <span className="ws-goal-chip">
                      {filter.min_salary_k ? `月薪 ≥ ${filter.min_salary_k}K` : "薪资不限"}
                    </span>
                    <span className="ws-goal-chip">{describeSafetySummary(config)}</span>
                    <span className="ws-goal-chip">{greetModeLabel}</span>
                  </div>
                </div>
                {onOpenConfig && (
                  <Button
                    size="small"
                    type="link"
                    icon={<SettingOutlined />}
                    style={{ paddingInline: 4, flexShrink: 0 }}
                    onClick={() => onOpenConfig("job", "basic")}
                  >
                    修改设置
                  </Button>
                )}
              </div>
              {availableProfiles.length > 1 && (
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <Typography.Text type="secondary" style={{ fontSize: 12, flexShrink: 0 }}>
                    当前方案
                  </Typography.Text>
                  <Select
                    size="small"
                    style={{ flex: 1 }}
                    value={activeProfile.id}
                    onChange={setSelectedProfileId}
                    options={availableProfiles.map((p) => ({ value: p.id, label: p.name }))}
                  />
                </div>
              )}
            </div>

            {/* Three readiness lamps */}
            <div className="ws-readiness-grid">
              <button
                type="button"
                className={`ws-lamp ${hasIntent ? "is-ready" : "is-warn"}`}
                onClick={() => onOpenConfig?.("job", "basic")}
              >
                <span className="ws-lamp-head">
                  <span className="ws-lamp-title">
                    <IconTarget size={15} style={{ color: hasIntent ? "#10b981" : "#f59e0b" }} />
                    <span>1. 求职意向</span>
                  </span>
                  <span className={`ws-lamp-dot ${hasIntent ? "is-ready" : "is-warn"}`} />
                </span>
                <span className="ws-lamp-sub">
                  {hasIntent ? `${queryText} · ${getCityLabel(filter.city)}` : "点击填写岗位与城市"}
                </span>
              </button>

              <button
                type="button"
                className={`ws-lamp ${hasResume && greetReady ? "is-ready" : "is-warn"}`}
                onClick={() => onOpenConfig?.("resume", "basic")}
              >
                <span className="ws-lamp-head">
                  <span className="ws-lamp-title">
                    <IconResumeDoc
                      size={15}
                      style={{ color: hasResume && greetReady ? "#10b981" : "#f59e0b" }}
                    />
                    <span>2. 简历与话术</span>
                  </span>
                  <span
                    className={`ws-lamp-dot ${hasResume && greetReady ? "is-ready" : "is-warn"}`}
                  />
                </span>
                <span className="ws-lamp-sub">
                  {hasResume
                    ? greetReady
                      ? "简历与开场白已就绪"
                      : "点此补齐打招呼方式"
                    : "建议上传 PDF 简历"}
                </span>
              </button>

              <button
                type="button"
                className={`ws-lamp ${envReady ? "is-ready" : envNeedsLogin ? "is-warn" : ""}`}
                disabled={
                  currentEnvironment.phase === "checking" ||
                  currentStartPending ||
                  currentPlatformHasActiveTask
                }
                onClick={() => void handleCheckEnv(platform)}
              >
                <span className="ws-lamp-head">
                  <span className="ws-lamp-title">
                    {currentEnvironment.phase === "checking" ? (
                      <LoadingOutlined style={{ color: "var(--of-accent)" }} />
                    ) : (
                      <IconBrowserCheck
                        size={15}
                        style={{
                          color: envReady
                            ? "#10b981"
                            : envNeedsLogin
                              ? "#f59e0b"
                              : "var(--of-text-muted)",
                        }}
                      />
                    )}
                    <span>3. {currentPlatformMeta.shortLabel} 登录</span>
                  </span>
                  <span
                    className={`ws-lamp-dot ${
                      envReady ? "is-ready" : envNeedsLogin ? "is-warn" : "is-idle"
                    }`}
                  />
                </span>
                <span className="ws-lamp-sub">
                  {currentEnvironment.phase === "checking"
                    ? "正在检查浏览器与账号..."
                    : envReady
                      ? "已登录，可直接开跑"
                      : envNeedsLogin
                        ? "请下方扫码后重检"
                        : "点击检查登录状态"}
                </span>
              </button>
            </div>

            {/* Inline env check feedback (only visible when checking or done) */}
            {currentEnvironment.phase !== "idle" && (
              <div className="ws-env-feedback">
                <Steps
                  size="small"
                  responsive={false}
                  current={
                    currentEnvironment.phase === "checking" ||
                    (!currentEnvironment.result && Boolean(currentEnvironment.message))
                      ? currentStepIndex
                      : ENV_STEPS.length - 1
                  }
                  items={ENV_STEPS.map((step, index) => ({
                    title: step.title,
                    status: resolveStepStatus(
                      currentEnvironment.phase,
                      currentEnvironment.result?.status ?? null,
                      index,
                      currentStepIndex,
                      Boolean(currentEnvironment.message),
                    ),
                    icon:
                      currentEnvironment.phase === "checking" && index === currentStepIndex ? (
                        <LoadingOutlined />
                      ) : currentEnvironment.phase === "done" &&
                        index === currentStepIndex &&
                        (currentEnvironment.result?.status === "login_required" ||
                          (!currentEnvironment.result && Boolean(currentEnvironment.message))) ? (
                        <WarningOutlined />
                      ) : undefined,
                  }))}
                />
                {currentEnvironment.message && (
                  <Alert
                    style={{ marginTop: 10 }}
                    type="warning"
                    showIcon
                    message={currentEnvironment.message}
                  />
                )}
                {currentEnvironment.result?.message && !currentEnvironment.message && (
                  <Typography.Text
                    type="secondary"
                    style={{ display: "block", marginTop: 8, fontSize: 12 }}
                  >
                    {currentEnvironment.result.message}
                  </Typography.Text>
                )}
                {currentQrSrc && (
                  <div className="ws-qr-box">
                    <Image
                      src={currentQrSrc}
                      width={108}
                      height={108}
                      preview={false}
                      alt={`${currentPlatformMeta.label}登录二维码`}
                    />
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      请用 {currentPlatformMeta.label} App 扫码登录，完成后再次点击「3.{" "}
                      {currentPlatformMeta.shortLabel} 登录」确认。
                    </Typography.Text>
                  </div>
                )}
              </div>
            )}

            {/* Primary + secondary CTA buttons */}
            <div style={{ marginTop: "auto", display: "flex", flexDirection: "column", gap: 10 }}>
              <Button
                block
                type="primary"
                size="large"
                className="ws-hero-cta"
                icon={<IconPlaySolid size={15} />}
                loading={currentStartPending}
                disabled={currentStartPending}
                style={{
                  backgroundColor: currentPlatformMeta.accent,
                  borderColor: currentPlatformMeta.accent,
                }}
                onClick={() => openStartModal(platform, "job_hunting")}
              >
                {currentStartPending
                  ? "正在启动..."
                  : `开始 ${currentPlatformMeta.shortLabel} 自动投递`}
              </Button>

              <div
                className="ws-secondary-actions"
                style={!currentPlatformMeta.supportsChat ? { gridTemplateColumns: "1fr" } : undefined}
              >
                {currentPlatformMeta.supportsChat ? (
                  <>
                    <Button
                      icon={<IconReplyPulse size={15} />}
                      disabled={currentStartPending}
                      onClick={() => openStartModal(platform, "reply_unread")}
                    >
                      处理未读回复
                    </Button>
                    <Button
                      icon={<IconSliders size={15} />}
                      disabled={currentStartPending}
                      onClick={() => openStartModal(platform, "periodic_job_hunting")}
                    >
                      周期挂机 / 更多模式
                    </Button>
                  </>
                ) : (
                  <Button
                    icon={<IconClockRepeat size={15} />}
                    disabled={currentStartPending}
                    onClick={() => openStartModal(platform, "periodic_job_hunting")}
                  >
                    周期定时投递
                  </Button>
                )}
              </div>
            </div>
          </Space>
        </Card>

        {/* Right panel: live queue */}
        <Card
          styles={{
            body: { padding: 20, height: "100%", display: "flex", flexDirection: "column" },
          }}
        >
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              gap: 10,
              alignItems: "center",
              marginBottom: 14,
              flexWrap: "wrap",
            }}
          >
            <div>
              <Typography.Title level={5} style={{ margin: 0, letterSpacing: "-0.015em" }}>
                运行进度
              </Typography.Title>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                {taskCounts.active > 0
                  ? `运行中 ${taskCounts.running} · 排队 ${taskCounts.queued} · 点击任务可查看日志`
                  : "任务入队后会在此实时展示状态"}
              </Typography.Text>
            </div>
            {queueTasks.length > 0 && (
              <Segmented<QueueFilter>
                size="small"
                value={queueFilter}
                options={[
                  { value: "all", label: "全部" },
                  { value: "boss", label: "BOSS" },
                  { value: "liepin", label: "猎聘" },
                  { value: "51job", label: "51job" },
                ]}
                onChange={setQueueFilter}
              />
            )}
          </div>

          {visibleQueueTasks.length === 0 ? (
            <div className="ws-queue-empty">
              <OfferFlowLogo size={42} />
              <Typography.Text strong style={{ fontSize: 13.5 }}>
                {queueTasks.length === 0 ? "当前没有运行中的任务" : "该平台暂无任务记录"}
              </Typography.Text>
              <Typography.Text type="secondary" style={{ fontSize: 12, maxWidth: 270 }}>
                确认左侧 3 盏准备灯后，点击「开始自动投递」即可按安全节奏试跑一轮。
              </Typography.Text>
            </div>
          ) : (
            <div
              style={{
                display: "grid",
                gap: 8,
                overflowY: "auto",
                paddingRight: 2,
                alignContent: "start",
                flex: 1,
                maxHeight: 340,
              }}
            >
              {visibleQueueTasks.map((task) => {
                const selected = selectedTaskId === task.task_id;
                const queuePosition =
                  task.status === "queued"
                    ? queuedTasksInExecutionOrder.findIndex(
                        (candidate) => candidate.task_id === task.task_id,
                      ) + 1
                    : null;
                return (
                  <div
                    key={task.task_id}
                    role="button"
                    tabIndex={0}
                    className={`ws-queue-item${selected ? " is-selected" : ""}`}
                    onClick={() => {
                      setSelectedTaskId(task.task_id);
                      setLogCollapsed(false);
                    }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        setSelectedTaskId(task.task_id);
                        setLogCollapsed(false);
                      }
                    }}
                    style={{
                      borderColor: selected ? PLATFORM_META[task.platform].accent : undefined,
                    }}
                  >
                    <div style={{ minWidth: 0 }}>
                      <Space size={7}>
                        <PlatformBadgeIcon platform={task.platform} size={17} />
                        <Typography.Text strong>{getFlowModeLabel(task.mode)}</Typography.Text>
                      </Space>
                      <Typography.Text
                        type="secondary"
                        style={{ display: "block", marginTop: 4, fontSize: 11.5 }}
                      >
                        {new Date(task.created_at).toLocaleString()}
                        {availableProfiles.length > 1
                          ? ` · ${getTaskProfileLabel(task)}`
                          : ""}
                      </Typography.Text>
                      {task.plan && (
                        <Typography.Text
                          type="secondary"
                          style={{ display: "block", marginTop: 2, fontSize: 11.5 }}
                        >
                          {describePeriodicPlan(task.plan)}
                        </Typography.Text>
                      )}
                    </div>
                    <Tag color={taskStateColor(task.status)} style={{ margin: 0 }}>
                      {taskStateLabel(task.status)}
                      {queuePosition ? ` · #${queuePosition}` : ""}
                    </Tag>
                    {isActiveJobTask(task) ? (
                      <Button
                        size="small"
                        danger
                        loading={task.status === "stopping"}
                        disabled={task.status === "stopping"}
                        onClick={(event) => {
                          event.stopPropagation();
                          void handleStopTask(task.task_id);
                        }}
                      >
                        停止
                      </Button>
                    ) : (
                      <span />
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </Card>
      </section>

      {/* ── Collapsible bottom log terminal ── */}
      <section className="ws-log-section">
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            gap: 12,
            flexWrap: "wrap",
          }}
        >
          <Space size={8}>
            <IconTerminal size={15} style={{ color: "var(--of-text-muted)" }} />
            <Typography.Text strong style={{ fontSize: 13 }}>
              运行日志
            </Typography.Text>
            <Tag color={selectedTask ? "processing" : "default"} style={{ margin: 0 }}>
              {selectedTask
                ? `${PLATFORM_META[selectedTask.platform].shortLabel} · ${selectedTask.task_id.slice(0, 8)}`
                : "全部任务"}
            </Tag>
            {logCollapsed && (
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                平时默认收起，需要排查细节时再展开
              </Typography.Text>
            )}
          </Space>
          <Space wrap size={6}>
            {!logCollapsed && (
              <>
                <Button
                  size="small"
                  type={logLevelFilter === "warn_error" ? "primary" : "default"}
                  icon={<FilterOutlined />}
                  onClick={() =>
                    setLogLevelFilter((prev) => (prev === "all" ? "warn_error" : "all"))
                  }
                >
                  仅看警告/错误
                </Button>
                <Input
                  size="small"
                  allowClear
                  prefix={<SearchOutlined />}
                  placeholder="过滤日志关键字"
                  style={{ width: 165 }}
                  value={logSearch}
                  onChange={(event) => setLogSearch(event.target.value)}
                />
                <Button size="small" icon={<CopyOutlined />} onClick={handleCopyLog}>
                  复制
                </Button>
                {selectedTaskId && (
                  <Button size="small" onClick={() => setSelectedTaskId(null)}>
                    看全部
                  </Button>
                )}
              </>
            )}
            <Button
              size="small"
              type="text"
              icon={logCollapsed ? <EyeOutlined /> : <EyeInvisibleOutlined />}
              onClick={() => setLogCollapsed(!logCollapsed)}
            >
              {logCollapsed ? "展开日志" : "收起"}
            </Button>
          </Space>
        </div>
        {!logCollapsed && (
          <pre ref={logRef} className="ws-log-terminal">
            {filteredLogLines.length === 0 ? (
              <span className="ws-log-line is-info">
                {logContent.trim() ? "// 当前过滤条件下无匹配日志" : "// 暂无运行日志..."}
              </span>
            ) : (
              filteredLogLines.map((line, index) => (
                <span key={index} className={`ws-log-line is-${parseLogLineLevel(line)}`}>
                  {line}
                  {"\n"}
                </span>
              ))
            )}
          </pre>
        )}
      </section>

      {/* ── Start task modal ── */}
      <Modal
        title={`启动 ${PLATFORM_META[modalPlatform].label} 任务`}
        open={startModalOpen}
        centered
        width={640}
        styles={{ body: { maxHeight: "62vh", overflowY: "auto" } }}
        onOk={() => void handleStartConfirm()}
        onCancel={closeStartModal}
        okText="立即开始"
        cancelText="取消"
        okButtonProps={{
          disabled:
            modalTaskStartPending ||
            !!periodicBlocker ||
            (readinessReport !== null && !readinessReport.ready),
        }}
      >
        <Radio.Group
          value={selectedMode}
          onChange={(event) => setSelectedMode(event.target.value)}
          style={{ width: "100%", paddingTop: 4 }}
        >
          {FLOW_MODE_OPTIONS.filter((option) =>
            modalPlatform === "51job"
              ? ["job_hunting", "periodic_job_hunting"].includes(option.key)
              : true,
          ).map((option) => {
            const selected = selectedMode === option.key;
            return (
              <div
                key={option.key}
                onClick={() => setSelectedMode(option.key)}
                style={{
                  display: "grid",
                  gridTemplateColumns: "152px 1fr",
                  alignItems: "center",
                  gap: 12,
                  padding: "9px 12px",
                  borderRadius: 8,
                  cursor: "pointer",
                  background: selected ? "rgba(22, 119, 255, 0.08)" : undefined,
                }}
              >
                <Radio value={option.key}>
                  <Typography.Text strong={selected}>{option.label}</Typography.Text>
                </Radio>
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {modalPlatform === "51job" && option.key === "periodic_job_hunting"
                    ? "按防封间隔分批直接投递，等待期间不执行聊天。"
                    : option.description}
                </Typography.Text>
              </div>
            );
          })}
        </Radio.Group>

        {modeSummary && (
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "68px 1fr",
              alignItems: "start",
              rowGap: 12,
              columnGap: 12,
              marginTop: 14,
              paddingTop: 14,
              borderTop: "1px solid var(--of-border)",
            }}
          >
            {modeSummary}
          </div>
        )}

        {readinessReport &&
          readinessReport.items.some((item) => item.level !== "ready") && (
            <div
              style={{
                marginTop: 14,
                paddingTop: 14,
                borderTop: "1px solid var(--of-border)",
                display: "flex",
                flexDirection: "column",
                gap: 8,
              }}
            >
              <Typography.Text strong style={{ fontSize: 12.5 }}>
                启动前检查
              </Typography.Text>
              {readinessReport.items
                .filter((item) => item.level !== "ready")
                .map((item) => (
                  <Alert
                    key={item.key}
                    type={item.level === "blocked" ? "error" : "warning"}
                    showIcon
                    message={`${item.label}：${item.message}`}
                    action={
                      item.config_group && onOpenConfig ? (
                        <Button
                          size="small"
                          type="link"
                          onClick={() => {
                            closeStartModal();
                            onOpenConfig(item.config_group as ConfigGroup);
                          }}
                        >
                          去设置
                        </Button>
                      ) : undefined
                    }
                  />
                ))}
            </div>
          )}
        {readinessLoading && !readinessReport && (
          <Typography.Text
            type="secondary"
            style={{ display: "block", marginTop: 12, fontSize: 12 }}
          >
            正在检查当前设置是否就绪…
          </Typography.Text>
        )}
      </Modal>

      {/* ── Per-task periodic drawer ── */}
      <Drawer
        title="微调本次周期参数"
        width={740}
        open={paramsDrawerOpen}
        onClose={closeParamsDrawer}
        zIndex={1100}
        extra={
          <Space>
            <Button
              icon={<UndoOutlined />}
              disabled={!canResetPeriodicDelivery(periodicDraft, getPeriodicDeliveryConfig(config))}
              onClick={() => setPeriodicDraft(getPeriodicDeliveryConfig(config))}
            >
              恢复默认
            </Button>
            <Button type="primary" onClick={closeParamsDrawer}>
              完成
            </Button>
          </Space>
        }
      >
        <Alert
          type="info"
          showIcon
          style={{ marginBottom: 16 }}
          message="这里的改动仅作用于本次任务"
          description="如需永久修改默认防封档位或每日上限，请前往「求职设置」。"
        />
        <PeriodicDeliverySection
          variant="plain"
          config={periodicDraft}
          onChange={(next) => setPeriodicDraft((current) => ({ ...current, ...next }))}
        />
      </Drawer>

      {!usesExternalReview && (
        <ManualReviewDrawer
          open={reviewDrawerOpen}
          onClose={() => setReviewDrawerOpen(false)}
          records={manualReview.records}
          loading={manualReview.loading}
          onResolve={(record) => void manualReview.resolve(record)}
          onClear={() => void manualReview.clear()}
          onRefresh={() => void manualReview.refresh()}
          onOpenConversation={
            onOpenConversation
              ? (jobId) => {
                  setReviewDrawerOpen(false);
                  onOpenConversation(jobId);
                }
              : undefined
          }
        />
      )}
    </div>
  );
};

export default WorkspacePage;
