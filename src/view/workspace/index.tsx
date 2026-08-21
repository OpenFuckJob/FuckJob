import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Drawer,
  Image,
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
  CheckCircleOutlined,
  CopyOutlined,
  DatabaseOutlined,
  ExclamationCircleOutlined,
  EyeInvisibleOutlined,
  EyeOutlined,
  LoadingOutlined,
  MessageOutlined,
  PlayCircleOutlined,
  RocketOutlined,
  SendOutlined,
  SlidersOutlined,
  UndoOutlined,
  WarningOutlined,
} from "@ant-design/icons";
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
} from "../../types/rpa";
import {
  buildPeriodicPlan,
  countJobTasks,
  describePeriodicDelivery,
  describePeriodicPlan,
  isActiveJobTask,
} from "../../types/rpa";
import type { PeriodicPlan } from "../../types/rpa";
import type { JobDetail } from "../../types/job-detail";
import ManualReviewDrawer, { useManualReview } from "./manual-review-drawer";
import {
  getDefaultJobProfile,
  getJobProfiles,
  getPeriodicDeliveryConfig,
  getReplyPollingConfig,
  type AppRuntimeConfig,
  type PeriodicDeliveryConfig,
} from "../../types/app-config";
import PeriodicDeliverySection, {
  canResetPeriodicDelivery,
} from "../config/PeriodicDeliverySection";

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
  description: string;
  limitation?: string;
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

function resolveStepStatus(
  phase: CheckPhase,
  envStatus: EnvCheckStatus | null,
  stepIndex: number,
  currentStepIndex: number,
): "wait" | "process" | "finish" | "error" {
  if (phase === "idle") return "wait";
  if (phase === "checking") {
    if (stepIndex < currentStepIndex) return "finish";
    if (stepIndex === currentStepIndex) return "process";
    return "wait";
  }
  if (stepIndex < currentStepIndex) return "finish";
  if (stepIndex === currentStepIndex) {
    return envStatus === "login_required" ? "error" : "finish";
  }
  return "wait";
}

const PLATFORM_META: Record<PlatformKind, PlatformMeta> = {
  boss: {
    label: "BOSS 直聘",
    shortLabel: "BOSS",
    accent: "#1677ff",
    description: "适合直接沟通、回复未读和周期投递。",
  },
  liepin: {
    label: "猎聘",
    shortLabel: "猎聘",
    accent: "#722ed1",
    description: "适合猎聘职位搜索、筛选和主动沟通。",
  },
};

const FLOW_MODE_OPTIONS: FlowModeOption[] = [
  {
    key: "job_hunting",
    label: "单轮自动求职",
    description: "按当前筛选条件处理一轮岗位。",
  },
  {
    key: "reply_unread",
    label: "回复未读",
    description: "处理当前平台未读沟通消息。",
  },
  {
    key: "sync_chat_history",
    label: "读取聊天消息",
    description: "同步已读会话、岗位和历史消息，不会点开未读消息。",
  },
  {
    key: "periodic_job_hunting",
    label: "周期投递",
    description: "每轮完成后按设定间隔继续下一轮，等待期间自动回复未读消息。",
  },
  {
    key: "polling_reply",
    label: "轮询回复",
    description: "持续盯着未读消息自动回复，不投递岗位。停止前一直运行。",
  },
];

function getFlowModeLabel(mode: FlowMode): string {
  return FLOW_MODE_OPTIONS.find((option) => option.key === mode)?.label ?? "自动求职";
}

export function filterTaskLogContent(
  content: string,
  taskId: string | null,
): string {
  if (!content.trim() || !taskId) {
    return content;
  }

  return content
    .split("\n")
    .filter((line) => line.includes(`[task=${taskId}]`))
    .join("\n");
}

/**
 * 「待你处理」磁贴的外观。
 *
 * 抽出来是因为这条规则正是它能待在磁贴区的前提：0 条时必须安静得像块背景板，
 * 有数时必须一眼扎到人。哪天被改成恒定配色，功能就悄悄退化成没人看的角落，
 * 而界面照样正常渲染，谁都不会发现
 */
export function describePendingReview(count: number): Pick<StatTile, "value" | "subtitle" | "color" | "bg"> {
  const urgent = count > 0;
  return {
    value: `${count}`,
    subtitle: urgent ? "点击查看并接手" : "AI 都自己处理了",
    color: urgent ? "#ef4444" : "#64748b",
    bg: urgent ? "rgba(239,68,68,0.1)" : "rgba(148,163,184,0.1)",
  };
}

export function getTaskProfileLabel(task: Pick<JobTaskInfo, "mode" | "profile_id" | "profile_name">): string {
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

const PLATFORM_ORDER: PlatformKind[] = ["boss", "liepin"];

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
  return [...tasks].sort((left, right) =>
    right.created_at.localeCompare(left.created_at),
  );
}

export function filterTasksByPlatform(
  tasks: JobTaskInfo[],
  filter: QueueFilter,
): JobTaskInfo[] {
  return filter === "all"
    ? tasks
    : tasks.filter((task) => task.platform === filter);
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

/**
 * 周期投递里挡住启动的配置问题，没有问题时返回 null。
 *
 * 「开了时段限制却一格都没选」是个安静的陷阱：任务能入队、能跑，但永远等不到
 * 可投的时刻，日志上只会看到一句「暂停投递」然后再无下文。挡在启动之前才说得清
 */
export function describePeriodicBlocker(
  config: Pick<PeriodicDeliveryConfig, "interval_minutes" | "window_enabled" | "windows">,
): string | null {
  if (config.interval_minutes <= 0) return "投递间隔必须大于 0 分钟";
  if (config.window_enabled && config.windows.length === 0) {
    return "投递时段一格都没选，任务不会投递";
  }
  return null;
}

export function requestStopJobTask(taskId: string): Promise<CommandResult<void>> {
  return invoke<CommandResult<void>>("stop_job_task", { taskId });
}

/* ────────── Stat tile helper ────────── */

interface StatTile {
  label: string;
  value: string;
  subtitle?: string;
  icon: React.ReactNode;
  color: string;
  bg: string;
  onClick?: () => void;
}

/* ────────── Component ────────── */

const WorkspacePage = ({
  config,
  onOpenConversation,
}: {
  config: AppRuntimeConfig;
  onNavigate?: (tab: "job-data") => void;
  onOpenConfig?: (group: "resume" | "llm" | "job" | "greet" | "reply" | "browser") => void;
  onOpenConversation?: (jobId: string) => void;
}) => {
  const [environmentStates, setEnvironmentStates] = useState<
    Record<PlatformKind, PlatformEnvState>
  >({
    boss: { phase: "idle", result: null, message: "" },
    liepin: { phase: "idle", result: null, message: "" },
  });
  const [taskOverview, setTaskOverview] = useState<JobTaskOverview>({
    tasks: [],
    running_count: 0,
    queued_count: 0,
    max_parallel_tasks: 2,
  });
  const [logContent, setLogContent] = useState("");
  const [startModalOpen, setStartModalOpen] = useState(false);
  const [selectedMode, setSelectedMode] = useState<FlowMode>("job_hunting");
  // 弹窗里的周期参数是本次任务的，改动不回写配置——配置页存的只是初值
  const [periodicDraft, setPeriodicDraft] = useState<PeriodicDeliveryConfig>(() =>
    getPeriodicDeliveryConfig(config),
  );
  const [paramsDrawerOpen, setParamsDrawerOpen] = useState(false);
  const [selectedProfileId, setSelectedProfileId] = useState<string>(() => getDefaultJobProfile(config).id);
  const [platform, setPlatform] = useState<PlatformKind>("boss");
  const [reviewDrawerOpen, setReviewDrawerOpen] = useState(false);
  const manualReview = useManualReview();
  const [modalPlatform, setModalPlatform] = useState<PlatformKind>("boss");
  const [pendingStartPlatforms, setPendingStartPlatforms] = useState<
    Partial<Record<PlatformKind, boolean>>
  >({});
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [queueFilter, setQueueFilter] = useState<QueueFilter>("all");
  const [jobs, setJobs] = useState<JobDetail[]>([]);
  const [logCollapsed, setLogCollapsed] = useState(true);
  const logRef = useRef<HTMLPreElement>(null);
  const previousActiveTaskIdsRef = useRef<Set<string>>(new Set());
  const taskRefreshPromiseRef = useRef<Promise<void> | null>(null);
  const [messageApi, contextHolder] = message.useMessage();

  const loadJobs = useCallback(async () => {
    try {
      const result = await invoke<CommandResult<JobDetail[]>>("job_list");
      if (result.success && result.data) {
        setJobs(result.data);
      }
    } catch {
      // ignore
    }
  }, []);

  const refreshTaskStatus = useCallback((): Promise<void> => {
    if (taskRefreshPromiseRef.current) return taskRefreshPromiseRef.current;

    const request = (async () => {
      try {
        const result = await invoke<CommandResult<JobTaskOverview>>(
          "get_job_task_status",
        );
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
        // A transient poll failure must not tear down the dashboard polling loop.
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
      const result = await invoke<CommandResult<string>>("read_log_file", {
        lines: 500,
      });
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
      await Promise.all([refreshLog(), refreshTaskStatus()]);
      if (!disposed) timer = setTimeout(() => void poll(), 2000);
    };
    void poll();
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
    };
  }, [loadJobs, refreshLog, refreshTaskStatus]);

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logContent]);

  const handleCheckEnv = useCallback(async (targetPlatform: PlatformKind) => {
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
        messageApi.success(`${PLATFORM_META[targetPlatform].label} 环境检查通过`);
      }
    } catch (error: unknown) {
      const msg = error instanceof Error ? error.message : "环境检查异常";
      setEnvironmentStates((current) => ({
        ...current,
        [targetPlatform]: { phase: "done", result: null, message: msg },
      }));
      messageApi.error(msg);
    }
  }, [messageApi]);

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
          profileId: mode === "job_hunting" || mode === "periodic_job_hunting" ? profileId : undefined,
        });
        if (!result.success || !result.data) {
          messageApi.error(commandErrorMessage(result.error, "启动失败"));
          return;
        }
        await refreshTaskStatus();
        messageApi.success(
          `${PLATFORM_META[startedPlatform].label} ${getFlowModeLabel(mode)}已加入任务队列`,
        );
      } catch (error: unknown) {
        messageApi.error(
          error instanceof Error ? error.message : "启动失败",
        );
      } finally {
        setPendingStartPlatforms((current) => ({
          ...current,
          [startedPlatform]: false,
        }));
      }
    },
    [messageApi, refreshTaskStatus],
  );

  /** 打开启动弹窗。环境状态由左侧按钮显式检查，不在弹窗重复请求。 */
  const openStartModal = useCallback((targetPlatform: PlatformKind) => {
    setModalPlatform(targetPlatform);
    setSelectedMode("job_hunting");
    setSelectedProfileId(getDefaultJobProfile(config).id);
    // 每次打开都从配置重新取初值，上一次弹窗里的临时改动不该粘在这一次上
    setPeriodicDraft(getPeriodicDeliveryConfig(config));
    setParamsDrawerOpen(false);
    setStartModalOpen(true);
  }, [config]);

  const closeParamsDrawer = useCallback(() => setParamsDrawerOpen(false), []);

  const closeStartModal = useCallback(() => {
    setStartModalOpen(false);
    // 抽屉挂在弹窗之上，弹窗关了它还浮着就成了没有来路的孤儿面板
    setParamsDrawerOpen(false);
  }, []);

  const handleStartConfirm = useCallback(async () => {
    closeStartModal();
    await handleRpaFlow(
      modalPlatform,
      selectedMode,
      // 「最长运行 N 小时」在这里才换算成绝对结束时刻，基准是点下确认的这一刻
      selectedMode === "periodic_job_hunting" ? buildPeriodicPlan(periodicDraft) : undefined,
      selectedMode === "job_hunting" || selectedMode === "periodic_job_hunting" ? selectedProfileId : undefined,
    );
  }, [
    closeStartModal,
    handleRpaFlow,
    modalPlatform,
    periodicDraft,
    selectedMode,
    selectedProfileId,
  ]);

  const handleStopTask = useCallback(async (taskId: string) => {
    try {
      const result = await requestStopJobTask(taskId);
      if (!result.success) {
        messageApi.error(commandErrorMessage(result.error, "停止失败"));
        return;
      }
      await refreshTaskStatus();
      messageApi.success("已发送停止请求");
    } catch (error: unknown) {
      messageApi.error(
        error instanceof Error ? error.message : "停止失败",
      );
    }
  }, [messageApi, refreshTaskStatus]);

  const handleCopyLog = useCallback(() => {
    if (!logContent) return;
    void navigator.clipboard.writeText(
      filterTaskLogContent(logContent, selectedTaskId),
    );
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
  const currentPlatformHasActiveTask = activeTasks.some(
    (task) => task.platform === platform,
  );
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
  const filteredLogContent = filterTaskLogContent(logContent, selectedTaskId);

  /* derived stats */
  const totalJobs = jobs.length;
  const sentCount = jobs.filter((j) => j.is_send_resume).length;
  const repliedCount = jobs.filter((j) => j.is_reply).length;
  const replyRate = totalJobs > 0 ? `${((repliedCount / totalJobs) * 100).toFixed(0)}%` : "—";
  const runningModeLabel = `${taskCounts.running} / ${taskOverview.max_parallel_tasks}`;
  const pendingReviewCount = manualReview.records.length;

  /** 摘要区的一行：左侧窄标签、右侧内容，两行之间靠这个网格对齐 */
  const summaryRow = (label: string, content: React.ReactNode) => (
    <>
      <Typography.Text type="secondary" style={{ fontSize: 13 }}>{label}</Typography.Text>
      <div style={{ minWidth: 0 }}>{content}</div>
    </>
  );

  const profileRow = summaryRow(
    "求职方案",
    <>
      <Select
        aria-label="本次使用的求职方案"
        style={{ width: "100%" }}
        value={selectedProfileId}
        onChange={setSelectedProfileId}
        options={getJobProfiles(config).filter((profile) => !profile.archived).map((profile) => ({
          value: profile.id,
          label: `${profile.name}${profile.id === (config.default_job_profile_id || getDefaultJobProfile(config).id) ? "（默认）" : ""}`,
        }))}
      />
      <Typography.Text type="secondary" style={{ display: "block", marginTop: 4, fontSize: 12 }}>
        入队后固定使用该方案快照，之后改配置不影响本次任务
      </Typography.Text>
    </>,
  );

  /**
   * 选中模式后，在列表下方的摘要区交代这次任务的关键参数。
   *
   * 摘要区只出结论，不出表单：周期投递那六项参数都有合理默认，绝大多数启动
   * 不需要动，铺在确认按钮上方只会把一个可选项渲染成必填项。要改的走抽屉
   */
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
                <div style={{ display: "flex", alignItems: "center", gap: 12, justifyContent: "space-between" }}>
                  <Typography.Text style={{ fontSize: 12 }}>
                    {describePeriodicDelivery(periodicDraft)}
                  </Typography.Text>
                  <Button size="small" icon={<SlidersOutlined />} onClick={() => setParamsDrawerOpen(true)}>
                    调整参数
                  </Button>
                </div>
                {periodicBlocker && (
                  // 按钮灰着却不说为什么，用户只会以为程序坏了
                  <Typography.Text style={{ display: "block", marginTop: 4, fontSize: 12, color: "#d97706" }}>
                    {periodicBlocker}，请在「调整参数」里修正
                  </Typography.Text>
                )}
              </div>,
            )}
          </>
        );
      case "reply_unread":
        return summaryRow(
          "求职方案",
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            按会话自动路由：每条未读消息沿用首次联系该岗位时使用的方案
          </Typography.Text>,
        );
      case "polling_reply":
        return summaryRow(
          "轮询节奏",
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            每 {polling.interval_minutes} 分钟检查一次新消息
            {polling.active_hours_enabled
              ? `，只在 ${polling.active_start_hour}:00 - ${polling.active_end_hour}:00 之间回复`
              : "，全天回复"}
            。可在「配置中心 · 自动回复」调整
          </Typography.Text>,
        );
      case "sync_chat_history":
        return null;
    }
  };

  const periodicBlocker =
    selectedMode === "periodic_job_hunting"
      ? describePeriodicBlocker(periodicDraft)
      : null;
  const modeSummary = renderModeSummary(selectedMode);

  const statTiles: StatTile[] = [
    {
      label: "运行状态",
      value: runningModeLabel,
      subtitle: taskCounts.queued > 0
        ? `${taskCounts.queued} 个任务排队中`
        : taskCounts.active > 0
          ? `${taskCounts.active} 个活动任务`
          : "等待启动",
      icon: <RocketOutlined style={{ fontSize: 18 }} />,
      color: activeTasks.length > 0 ? "#1677ff" : "#64748b",
      bg: activeTasks.length > 0 ? "rgba(22,119,255,0.1)" : "rgba(148,163,184,0.1)",
    },
    {
      label: "已建联岗位",
      value: `${totalJobs}`,
      subtitle: "本地已入库",
      icon: <DatabaseOutlined style={{ fontSize: 18 }} />,
      color: "#0ea5e9",
      bg: "rgba(14,165,233,0.1)",
    },
    {
      label: "已投递简历",
      value: `${sentCount}`,
      subtitle: totalJobs > 0 ? `占已建联 ${((sentCount / totalJobs) * 100).toFixed(0)}%` : "暂无数据",
      icon: <SendOutlined style={{ fontSize: 18 }} />,
      color: "#f59e0b",
      bg: "rgba(245,158,11,0.1)",
    },
    {
      label: "沟通回复",
      value: `${repliedCount}`,
      subtitle: `已建联岗位回复率 ${replyRate}`,
      icon: <MessageOutlined style={{ fontSize: 18 }} />,
      color: "#10b981",
      bg: "rgba(16,185,129,0.1)",
    },
    {
      label: "待你处理",
      ...describePendingReview(pendingReviewCount),
      icon: <ExclamationCircleOutlined style={{ fontSize: 18 }} />,
      onClick: () => setReviewDrawerOpen(true),
    },
  ];

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", gap: 20 }}>
      {contextHolder}

      {/* ── Stat tiles ── */}
      <section style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))", gap: 14 }}>
        {statTiles.map((tile) => (
          <Card
            key={tile.label}
            size="small"
            hoverable={!!tile.onClick}
            onClick={tile.onClick}
            styles={{ body: { padding: "14px 16px" } }}
            style={{
              background: "linear-gradient(135deg, #ffffff 0%, #f8fafc 100%)",
              cursor: tile.onClick ? "pointer" : undefined,
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 6 }}>
              <div style={{ padding: 7, borderRadius: 8, background: tile.bg, color: tile.color }}>
                {tile.icon}
              </div>
              <Typography.Text type="secondary" style={{ fontSize: 12, fontWeight: 500 }}>{tile.label}</Typography.Text>
            </div>
            <Typography.Text strong style={{ fontSize: 22, lineHeight: 1.2, display: "block" }}>
              {tile.value}
            </Typography.Text>
            <Typography.Text type="secondary" style={{ fontSize: 11 }}>{tile.subtitle}</Typography.Text>
          </Card>
        ))}
      </section>

      {/* ── Platform card + task queue ── */}
      <section
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(380px, 1fr) minmax(440px, 1.15fr)",
          gap: 16,
          alignItems: "start",
        }}
      >
        <Card
          styles={{ body: { padding: 20, height: "100%" } }}
          style={{
            borderTop: `4px solid ${currentPlatformMeta.accent}`,
            background: "linear-gradient(180deg, #ffffff 0%, #fbfdff 100%)",
          }}
        >
          <Space direction="vertical" size={16} style={{ width: "100%" }}>
            <Segmented<PlatformKind>
              block
              value={platform}
              options={PLATFORM_ORDER.map((value) => ({
                value,
                label: PLATFORM_META[value].label,
              }))}
              onChange={setPlatform}
            />

            <div>
              <Typography.Title level={4} style={{ margin: 0, color: currentPlatformMeta.accent }}>
                {currentPlatformMeta.label}
              </Typography.Title>
              <Typography.Text type="secondary" style={{ fontSize: 12.5 }}>
                {currentPlatformMeta.description}
              </Typography.Text>
            </div>

            <div style={{ padding: 14, border: "1px solid #e8edf3", borderRadius: 12, background: "#fff" }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12, marginBottom: 12 }}>
                <div>
                  <Typography.Text strong>环境状态</Typography.Text>
                  <Typography.Text type="secondary" style={{ display: "block", fontSize: 12 }}>浏览器与平台登录状态</Typography.Text>
                </div>
                <Button
                  size="small"
                  icon={currentEnvironment.phase === "checking" ? <LoadingOutlined /> : <CheckCircleOutlined />}
                  loading={currentEnvironment.phase === "checking"}
                  disabled={currentStartPending || currentPlatformHasActiveTask}
                  onClick={() => void handleCheckEnv(platform)}
                >检查环境</Button>
              </div>

              {currentEnvironment.phase === "idle" ? (
                <Typography.Text type="secondary" style={{ fontSize: 12.5 }}>尚未检查。需要确认登录状态时点击右侧按钮。</Typography.Text>
              ) : (
                <Steps
                  size="small"
                  responsive={false}
                  current={currentEnvironment.phase === "checking" ? currentStepIndex : ENV_STEPS.length - 1}
                  items={ENV_STEPS.map((step, index) => ({
                    title: step.title,
                    status: resolveStepStatus(currentEnvironment.phase, currentEnvironment.result?.status ?? null, index, currentStepIndex),
                    icon:
                      currentEnvironment.phase === "checking" && index === currentStepIndex ? <LoadingOutlined />
                        : currentEnvironment.phase === "done" && index === currentStepIndex && currentEnvironment.result?.status === "login_required" ? <WarningOutlined />
                          : undefined,
                  }))}
                />
              )}
              {currentEnvironment.message && <Alert style={{ marginTop: 12 }} type="warning" showIcon message={currentEnvironment.message} />}
              {currentEnvironment.result?.message && !currentEnvironment.message && (
                <Typography.Text type="secondary" style={{ display: "block", marginTop: 10, fontSize: 12.5 }}>{currentEnvironment.result.message}</Typography.Text>
              )}
              {currentQrSrc && (
                <div style={{ marginTop: 12, padding: 12, display: "flex", alignItems: "center", gap: 14, background: "#f8fafc", borderRadius: 10 }}>
                  <Image src={currentQrSrc} width={112} height={112} preview={false} alt={`${currentPlatformMeta.label}登录二维码`} />
                  <Typography.Text type="secondary" style={{ fontSize: 12 }}>请使用 {currentPlatformMeta.label} App 扫码登录，完成后重新检查环境。</Typography.Text>
                </div>
              )}
            </div>

            <Button
              block
              type="primary"
              size="large"
              icon={<PlayCircleOutlined />}
              loading={currentStartPending}
              disabled={currentStartPending}
              style={{ background: currentPlatformMeta.accent }}
              onClick={() => openStartModal(platform)}
            >
              {currentStartPending ? "正在加入队列..." : `添加 ${currentPlatformMeta.shortLabel} 任务`}
            </Button>
          </Space>
        </Card>

        <Card styles={{ body: { padding: 18, display: "flex", flexDirection: "column" } }}>
        <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "center", marginBottom: 12, flexWrap: "wrap" }}>
          <div>
            <Typography.Title level={5} style={{ margin: 0 }}>任务队列</Typography.Title>
            <Typography.Text type="secondary" style={{ fontSize: 12.5 }}>
              最新任务在上 · 运行中 {taskCounts.running} · 排队 {taskCounts.queued}
            </Typography.Text>
          </div>
          <Space wrap>
            <Segmented<QueueFilter>
              size="small"
              value={queueFilter}
              options={[
                { value: "all", label: "全部" },
                { value: "boss", label: "BOSS" },
                { value: "liepin", label: "猎聘" },
              ]}
              onChange={setQueueFilter}
            />
            <Button
              size="small"
              type={selectedTaskId === null ? "primary" : "default"}
              onClick={() => {
                setSelectedTaskId(null);
                setLogCollapsed(false);
              }}
            >
              查看全部日志
            </Button>
          </Space>
        </div>

        {visibleQueueTasks.length === 0 ? (
          <Alert
            type="info"
            showIcon
            message={queueTasks.length === 0 ? "任务队列为空" : "当前筛选下没有任务"}
          />
        ) : (
          <div
            style={{
              display: "grid",
              gap: 8,
              maxHeight: 296,
              overflowY: "auto",
              paddingRight: 4,
            }}
          >
            {visibleQueueTasks.map((task) => {
              const selected = selectedTaskId === task.task_id;
              const queuePosition = task.status === "queued"
                ? queuedTasksInExecutionOrder.findIndex((candidate) => candidate.task_id === task.task_id) + 1
                : null;
              return (
                <div
                  key={task.task_id}
                  role="button"
                  tabIndex={0}
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
                    display: "grid",
                    gridTemplateColumns: "minmax(170px, 1.3fr) minmax(100px, .7fr) auto",
                    alignItems: "center",
                    gap: 12,
                    padding: "11px 12px",
                    border: `1px solid ${selected ? PLATFORM_META[task.platform].accent : "#e8edf3"}`,
                    borderRadius: 10,
                    background: selected ? "rgba(22, 119, 255, 0.04)" : "#fff",
                    cursor: "pointer",
                  }}
                >
                  <div>
                    <Space size={6}>
                      <Tag color={task.platform === "boss" ? "blue" : "purple"}>{PLATFORM_META[task.platform].shortLabel}</Tag>
                      <Typography.Text strong>{getFlowModeLabel(task.mode)}</Typography.Text>
                    </Space>
                    <Typography.Text type="secondary" style={{ display: "block", marginTop: 3, fontSize: 11.5 }}>
                      {task.task_id.slice(0, 8)} · {new Date(task.created_at).toLocaleString()}
                    </Typography.Text>
                    <Typography.Text type="secondary" style={{ display: "block", marginTop: 2, fontSize: 11.5 }}>
                      方案：{getTaskProfileLabel(task)}
                    </Typography.Text>
                    {task.plan && (
                      <Typography.Text type="secondary" style={{ display: "block", marginTop: 2, fontSize: 11.5 }}>
                        节奏：{describePeriodicPlan(task.plan)}
                      </Typography.Text>
                    )}
                  </div>
                  <Tag color={taskStateColor(task.status)} style={{ width: "fit-content" }}>
                    {taskStateLabel(task.status)}{queuePosition ? ` · 第 ${queuePosition} 位` : ""}
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
                    >停止</Button>
                  ) : <span />}
                </div>
              );
            })}
          </div>
        )}
        </Card>
      </section>

      {/* ── Start modal ── */}
      <Modal
        title={`添加 ${PLATFORM_META[modalPlatform].label} 任务`}
        open={startModalOpen}
        centered
        // 参数都搬进抽屉之后这里只剩五行列表加两行摘要，再宽就是一片空白
        width={680}
        styles={{ body: { maxHeight: "60vh", overflowY: "auto" } }}
        onOk={() => void handleStartConfirm()}
        onCancel={closeStartModal}
        okText="确认启动"
        cancelText="取消"
        okButtonProps={{
          disabled: modalTaskStartPending || !!periodicBlocker,
        }}
      >
        <Radio.Group
          value={selectedMode}
          onChange={(event) => setSelectedMode(event.target.value)}
          style={{ width: "100%", paddingTop: 4 }}
        >
          {FLOW_MODE_OPTIONS.filter(
            (option) => modalPlatform === "boss" || option.key !== "sync_chat_history",
          ).map((option) => {
            const selected = selectedMode === option.key;
            return (
              <div
                key={option.key}
                onClick={() => setSelectedMode(option.key)}
                style={{
                  display: "grid",
                  gridTemplateColumns: "168px 1fr",
                  alignItems: "center",
                  gap: 12,
                  padding: "9px 12px",
                  borderRadius: 8,
                  cursor: "pointer",
                  background: selected ? "rgba(22, 119, 255, 0.07)" : undefined,
                }}
              >
                <Radio value={option.key}>
                  <Typography.Text strong={selected}>{option.label}</Typography.Text>
                </Radio>
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {option.description}
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
              borderTop: "1px solid #e8edf3",
            }}
          >
            {modeSummary}
          </div>
        )}
      </Modal>

      {/* ── 周期投递参数抽屉 ── */}
      <Drawer
        title="周期投递参数"
        width={760}
        open={paramsDrawerOpen}
        onClose={closeParamsDrawer}
        // 挂在启动弹窗之上，默认层级会被弹窗的遮罩压住
        zIndex={1100}
        extra={
          <Space>
            <Button
              icon={<UndoOutlined />}
              disabled={!canResetPeriodicDelivery(periodicDraft, getPeriodicDeliveryConfig(config))}
              onClick={() => setPeriodicDraft(getPeriodicDeliveryConfig(config))}
            >
              恢复我的默认
            </Button>
            <Button type="primary" onClick={closeParamsDrawer}>完成</Button>
          </Space>
        }
      >
        <Alert
          type="info"
          showIcon
          style={{ marginBottom: 16 }}
          message="这里的改动只作用于本次任务"
          description="要改每次启动的初值，请到「配置中心 · 岗位筛选 · 周期投递」。"
        />
        <PeriodicDeliverySection
          variant="plain"
          config={periodicDraft}
          onChange={(next) => setPeriodicDraft((current) => ({ ...current, ...next }))}
        />
      </Drawer>

      {/* ── Collapsible log terminal ── */}
      <section style={{ flex: "1 1 0", minHeight: 0, display: "flex", flexDirection: "column" }}>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            gap: 12,
            marginBottom: logCollapsed ? 0 : 10,
            flexWrap: "wrap",
          }}
        >
          <Space>
            <Typography.Title level={5} style={{ margin: 0 }}>
              运行日志
            </Typography.Title>
            <Tag color={selectedTask ? "processing" : "default"}>
              {selectedTask
                ? `${PLATFORM_META[selectedTask.platform].shortLabel} · ${selectedTask.task_id.slice(0, 8)}`
                : "全部任务"}
            </Tag>
          </Space>
          <Space>
            <Button size="small" icon={logCollapsed ? <EyeOutlined /> : <EyeInvisibleOutlined />} onClick={() => setLogCollapsed(!logCollapsed)}>
              {logCollapsed ? "展开" : "收起"}
            </Button>
            {!logCollapsed && (
              <>
                <Button size="small" icon={<CopyOutlined />} onClick={handleCopyLog}>复制</Button>
                {selectedTaskId && (
                  <Button size="small" onClick={() => setSelectedTaskId(null)}>
                    退出任务日志
                  </Button>
                )}
              </>
            )}
          </Space>
        </div>
        {!logCollapsed && (
          <pre
            ref={logRef}
            style={{
              flex: 1,
              minHeight: 180,
              margin: 0,
              padding: "14px 16px",
              background: "#0f172a",
              color: "#38bdf8",
              borderRadius: 12,
              fontFamily: "'JetBrains Mono', 'Fira Code', 'Menlo', 'Monaco', monospace",
              fontSize: 12.5,
              lineHeight: 1.6,
              overflowY: "auto",
              whiteSpace: "pre-wrap",
              wordBreak: "break-all",
              boxShadow: "inset 0 2px 6px rgba(0,0,0,0.4)",
              border: "1px solid #1e293b",
            }}
          >
            {filteredLogContent || "// 暂无运行日志..."}
          </pre>
        )}
      </section>

      <ManualReviewDrawer
        open={reviewDrawerOpen}
        onClose={() => setReviewDrawerOpen(false)}
        records={manualReview.records}
        loading={manualReview.loading}
        onResolve={(record) => void manualReview.resolve(record)}
        onClear={() => void manualReview.clear()}
        onOpenConversation={
          onOpenConversation
            ? (jobId) => {
                setReviewDrawerOpen(false);
                onOpenConversation(jobId);
              }
            : undefined
        }
      />
    </div>
  );
};

export default WorkspacePage;
