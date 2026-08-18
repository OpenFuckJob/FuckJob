import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Card,
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
  EyeInvisibleOutlined,
  EyeOutlined,
  LoadingOutlined,
  MessageOutlined,
  PlayCircleOutlined,
  RocketOutlined,
  SendOutlined,
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
  countJobTasks,
  isActiveJobTask,
} from "../../types/rpa";
import type { JobDetail } from "../../types/job-detail";
import ManualReviewPanel from "./manual-review-panel";
import {
  getDefaultJobProfile,
  getJobProfiles,
  getReplyPollingConfig,
  type AppRuntimeConfig,
} from "../../types/app-config";
import { NumberField } from "../../components/NumberField";

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

const DEFAULT_INTERVAL_MINUTES = 30;
const MIN_INTERVAL_MINUTES = 1;
const MAX_INTERVAL_MINUTES = 1440;

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
}

/* ────────── Component ────────── */

const WorkspacePage = ({
  config,
}: {
  config: AppRuntimeConfig;
  onNavigate?: (tab: "job-data") => void;
  onOpenConfig?: (group: "resume" | "llm" | "job" | "greet" | "reply" | "browser") => void;
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
  const [intervalMinutes, setIntervalMinutes] = useState<number>(DEFAULT_INTERVAL_MINUTES);
  const [selectedProfileId, setSelectedProfileId] = useState<string>(() => getDefaultJobProfile(config).id);
  const [platform, setPlatform] = useState<PlatformKind>("boss");
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
      intervalMinutes?: number,
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
          intervalMinutes: mode === "periodic_job_hunting" ? intervalMinutes : undefined,
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
    setStartModalOpen(true);
  }, [config]);

  const closeStartModal = useCallback(() => {
    setStartModalOpen(false);
  }, []);

  const handleStartConfirm = useCallback(async () => {
    closeStartModal();
    await handleRpaFlow(
      modalPlatform,
      selectedMode,
      selectedMode === "periodic_job_hunting" ? intervalMinutes : undefined,
      selectedMode === "job_hunting" || selectedMode === "periodic_job_hunting" ? selectedProfileId : undefined,
    );
  }, [
    closeStartModal,
    handleRpaFlow,
    intervalMinutes,
    modalPlatform,
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
            styles={{ body: { padding: "14px 16px" } }}
            style={{ background: "linear-gradient(135deg, #ffffff 0%, #f8fafc 100%)" }}
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

      {/* ── Conversations waiting on the user ── */}
      <section>
        <ManualReviewPanel />
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
                <Typography.Text type="secondary" style={{ fontSize: 12.5 }}>尚未检查。需要确认登录状态时点击右侧按钮，本次结果会保留在当前平台卡片中。</Typography.Text>
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
            description={queueTasks.length === 0 ? "从左侧添加任务后，会在这里显示执行状态和顺序。" : "可切换为全部或另一个平台查看任务。"}
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
        width={640}
        styles={{ body: { maxHeight: "60vh", overflowY: "auto", paddingRight: 4 } }}
        onOk={() => void handleStartConfirm()}
        onCancel={closeStartModal}
        okText="确认启动"
        cancelText="取消"
        okButtonProps={{
          disabled:
            modalTaskStartPending ||
            (selectedMode === "periodic_job_hunting" &&
              (!intervalMinutes || intervalMinutes <= 0)),
        }}
      >
        <Space direction="vertical" size={16} style={{ width: "100%", paddingTop: 8 }}>
          <Radio.Group
            value={selectedMode}
            onChange={(event) => setSelectedMode(event.target.value)}
            style={{ width: "100%" }}
          >
            <Space direction="vertical" style={{ width: "100%" }} size={12}>
              {FLOW_MODE_OPTIONS.filter(
                (option) => modalPlatform === "boss" || option.key !== "sync_chat_history",
              ).map((option) => (
                <Card
                  key={option.key}
                  size="small"
                  hoverable
                  style={{
                    borderColor: selectedMode === option.key ? "#1677ff" : undefined,
                    background: selectedMode === option.key ? "rgba(22, 119, 255, 0.02)" : undefined,
                  }}
                  onClick={() => setSelectedMode(option.key)}
                >
                  <Radio value={option.key}>
                    <Space direction="vertical" size={0}>
                      <Typography.Text strong>{option.label}</Typography.Text>
                      <Typography.Text type="secondary" style={{ fontSize: 12 }}>{option.description}</Typography.Text>
                    </Space>
                  </Radio>
                </Card>
              ))}
            </Space>
          </Radio.Group>

          {selectedMode === "periodic_job_hunting" && (
            <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "8px 12px", background: "#f8fafc", borderRadius: 8 }}>
              <Typography.Text style={{ fontSize: 13 }}>每轮投递间隔时间</Typography.Text>
              <NumberField
                min={MIN_INTERVAL_MINUTES}
                max={MAX_INTERVAL_MINUTES}
                precision={0}
                fallback={DEFAULT_INTERVAL_MINUTES}
                value={intervalMinutes}
                onChange={setIntervalMinutes}
                addonAfter="分钟"
                style={{ width: 160 }}
              />
            </div>
          )}
          {selectedMode === "polling_reply" && (
            <div style={{ padding: "8px 12px", background: "#f8fafc", borderRadius: 8 }}>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                每 {getReplyPollingConfig(config).interval_minutes} 分钟检查一次新消息
                {getReplyPollingConfig(config).active_hours_enabled
                  ? `，只在 ${getReplyPollingConfig(config).active_start_hour}:00 - ${getReplyPollingConfig(config).active_end_hour}:00 之间回复`
                  : "，全天回复"}
                。节奏可在「设置 · 自动回复」里调整。
              </Typography.Text>
            </div>
          )}
          {(selectedMode === "job_hunting" || selectedMode === "periodic_job_hunting") ? (
            <div style={{ padding: "12px", background: "#f0f7ff", border: "1px solid #d6e8ff", borderRadius: 8 }}>
              <Typography.Text strong style={{ display: "block", marginBottom: 8 }}>本次使用的求职方案</Typography.Text>
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
              <Typography.Text type="secondary" style={{ display: "block", marginTop: 6, fontSize: 12 }}>
                任务加入队列后会固定使用该方案快照，之后修改配置不会影响本次任务。
              </Typography.Text>
            </div>
          ) : selectedMode === "reply_unread" ? (
            <Alert type="info" showIcon message="按会话方案自动路由" description="每条未读消息会沿用首次联系该岗位时使用的求职方案。" />
          ) : null}
        </Space>
      </Modal>

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
    </div>
  );
};

export default WorkspacePage;
