import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Image,
  InputNumber,
  Modal,
  Radio,
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
  StopOutlined,
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
  JobTaskStatus,
  PlatformKind,
  ReadinessReport,
} from "../../types/rpa";
import type { JobDetail } from "../../types/job-detail";

type CheckPhase = "idle" | "checking" | "done";

interface StepInfo {
  title: string;
  status: EnvCheckStep;
}

type LogFilter = "all" | PlatformKind;

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
    limitation: "猎聘暂不支持自动发送图片资源，图片话术会被跳过。",
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
    key: "periodic_job_hunting",
    label: "周期投递",
    description: "每轮完成后按设定间隔继续下一轮。",
  },
];

const LOG_FILTER_OPTIONS: { value: LogFilter; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "boss", label: "BOSS" },
  { value: "liepin", label: "猎聘" },
];

function getFlowModeLabel(mode: FlowMode): string {
  return FLOW_MODE_OPTIONS.find((option) => option.key === mode)?.label ?? "自动求职";
}

function lineMatchesPlatform(line: string, target: PlatformKind): boolean {
  return target === "liepin" ? line.includes("[猎聘]") : line.includes("[BOSS]");
}

function filterLogContent(content: string, filter: LogFilter): string {
  if (!content.trim() || filter === "all") {
    return content;
  }

  return content
    .split("\n")
    .filter((line) => lineMatchesPlatform(line, filter))
    .join("\n");
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
  onOpenConfig,
}: {
  onNavigate?: (tab: "job-data") => void;
  onOpenConfig?: (group: "resume" | "llm" | "job" | "greet" | "reply" | "browser") => void;
}) => {
  const [checkPhase, setCheckPhase] = useState<CheckPhase>("idle");
  const [envResult, setEnvResult] = useState<EnvCheckResult | null>(null);
  const [taskRunning, setTaskRunning] = useState(false);
  const [runningPlatform, setRunningPlatform] = useState<PlatformKind | null>(null);
  const [logContent, setLogContent] = useState("");
  const [checkMsg, setCheckMsg] = useState("");
  const [startModalOpen, setStartModalOpen] = useState(false);
  const [selectedMode, setSelectedMode] = useState<FlowMode>("job_hunting");
  const [intervalMinutes, setIntervalMinutes] = useState<number>(30);
  const [platform, setPlatform] = useState<PlatformKind>("boss");
  const [logFilter, setLogFilter] = useState<LogFilter>("boss");
  const [readiness, setReadiness] = useState<ReadinessReport | null>(null);
  const [preflightLoading, setPreflightLoading] = useState(false);
  const [jobs, setJobs] = useState<JobDetail[]>([]);
  const [logCollapsed, setLogCollapsed] = useState(true);
  const logRef = useRef<HTMLPreElement>(null);
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

  const refreshTaskStatus = useCallback(async () => {
    try {
      const result = await invoke<CommandResult<JobTaskStatus>>(
        "get_job_task_status",
      );
      if (result.success && result.data) {
        const wasRunning = taskRunning;
        setTaskRunning(result.data.running);
        if (!result.data.running) {
          setRunningPlatform(null);
        }
        // reload jobs whenever task state flips (stat stale remediation)
        if (wasRunning !== result.data.running) {
          void loadJobs();
        }
      }
    } catch {
      // ignore
    }
  }, [loadJobs, taskRunning]);

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
    void refreshLog();
    void refreshTaskStatus();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const timer = setInterval(() => {
      void refreshLog();
      void refreshTaskStatus();
    }, 2000);
    return () => clearInterval(timer);
  }, [refreshLog, refreshTaskStatus]);

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logContent]);

  const handleCheckEnv = useCallback(async () => {
    setCheckPhase("checking");
    setEnvResult(null);
    setCheckMsg("");
    try {
      const result = await invoke<CommandResult<EnvCheckResult>>("check_env", {
        platform,
      });
      if (!result.success || result.data === null) {
        setCheckPhase("done");
        const errorMessage = commandErrorMessage(result.error, "环境检查失败");
        setCheckMsg(errorMessage);
        messageApi.error(errorMessage);
        return;
      }
      setEnvResult(result.data);
      setCheckPhase("done");
      if (result.data.status === "completed") {
        messageApi.success(`${PLATFORM_META[platform].label} 环境检查通过`);
      }
    } catch (error: unknown) {
      const msg =
        error instanceof Error ? error.message : "环境检查异常";
      setCheckPhase("done");
      setCheckMsg(msg);
      messageApi.error(msg);
    }
  }, [messageApi, platform]);

  const handleRpaFlow = useCallback(
    async (mode: FlowMode, intervalMinutes?: number) => {
      try {
        const result = await invoke<CommandResult<void>>("rpa_flow", {
          platform,
          mode,
          intervalMinutes: mode === "periodic_job_hunting" ? intervalMinutes : undefined,
        });
        if (!result.success) {
          messageApi.error(commandErrorMessage(result.error, "启动失败"));
          return;
        }
        setTaskRunning(true);
        setRunningPlatform(platform);
        messageApi.success(`${PLATFORM_META[platform].label} ${getFlowModeLabel(mode)}已启动`);
      } catch (error: unknown) {
        messageApi.error(
          error instanceof Error ? error.message : "启动失败",
        );
      }
    },
    [messageApi, platform],
  );

  const handleStartConfirm = useCallback(async () => {
    setPreflightLoading(true);
    try {
      const result = await invoke<CommandResult<ReadinessReport>>("preflight_job_task", {
        platform,
        mode: selectedMode,
      });
      if (!result.success || !result.data) {
        messageApi.error(commandErrorMessage(result.error, "启动前检查失败"));
        return;
      }
      setReadiness(result.data);
      if (!result.data.ready) {
        messageApi.warning("准备工作尚未完成，请处理阻塞项后重试");
        return;
      }
      setStartModalOpen(false);
      await handleRpaFlow(
        selectedMode,
        selectedMode === "periodic_job_hunting" ? intervalMinutes : undefined,
      );
    } catch (error) {
      messageApi.error(error instanceof Error ? error.message : "启动前检查失败");
    } finally {
      setPreflightLoading(false);
    }
  }, [handleRpaFlow, intervalMinutes, messageApi, platform, selectedMode]);

  const handleStopTask = useCallback(async () => {
    try {
      const result = await invoke<CommandResult<void>>("stop_job_task");
      if (!result.success) {
        messageApi.error(commandErrorMessage(result.error, "停止失败"));
        return;
      }
      setTaskRunning(false);
      setRunningPlatform(null);
      messageApi.success("已发送停止请求");
    } catch (error: unknown) {
      messageApi.error(
        error instanceof Error ? error.message : "停止失败",
      );
    }
  }, [messageApi]);

  const handleCopyLog = useCallback(() => {
    if (!logContent) return;
    void navigator.clipboard.writeText(filterLogContent(logContent, logFilter));
    messageApi.success("日志已复制到剪贴板");
  }, [logContent, logFilter, messageApi]);

  const currentStepIndex = envResult
    ? getStepIndex(envResult.current_step)
    : 0;

  const qrSrc = envResult?.qr_code_base64
    ? `data:image/png;base64,${envResult.qr_code_base64}`
    : null;

  const showSteps = checkPhase !== "idle";
  const currentPlatform = PLATFORM_META[platform];
  const runningPlatformLabel = runningPlatform
    ? PLATFORM_META[runningPlatform].label
    : "当前";
  const filteredLogContent = filterLogContent(logContent, logFilter);

  /* derived stats */
  const totalJobs = jobs.length;
  const sentCount = jobs.filter((j) => j.is_send_resume).length;
  const repliedCount = jobs.filter((j) => j.is_reply).length;
  const replyRate = totalJobs > 0 ? `${((repliedCount / totalJobs) * 100).toFixed(0)}%` : "—";
  const runningModeLabel = taskRunning && runningPlatform
    ? `${PLATFORM_META[runningPlatform].shortLabel} 运行中`
    : "空闲";

  const statTiles: StatTile[] = [
    {
      label: "运行状态",
      value: runningModeLabel,
      subtitle: taskRunning ? "任务进行中" : "等待启动",
      icon: <RocketOutlined style={{ fontSize: 18 }} />,
      color: taskRunning ? "#1677ff" : "#64748b",
      bg: taskRunning ? "rgba(22,119,255,0.1)" : "rgba(148,163,184,0.1)",
    },
    {
      label: "已检索岗位",
      value: `${totalJobs}`,
      subtitle: "累计采集",
      icon: <DatabaseOutlined style={{ fontSize: 18 }} />,
      color: "#0ea5e9",
      bg: "rgba(14,165,233,0.1)",
    },
    {
      label: "已投递简历",
      value: `${sentCount}`,
      subtitle: totalJobs > 0 ? `占 ${((sentCount / totalJobs) * 100).toFixed(0)}%` : "暂无数据",
      icon: <SendOutlined style={{ fontSize: 18 }} />,
      color: "#f59e0b",
      bg: "rgba(245,158,11,0.1)",
    },
    {
      label: "沟通回复",
      value: `${repliedCount}`,
      subtitle: `回复率 ${replyRate}`,
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

      {/* ── Platform + environment check ── */}
      <section
        style={{
          flex: "0 0 auto",
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))",
          gap: 16,
          alignItems: "stretch",
        }}
      >
        <Card styles={{ body: { padding: 18 } }}>
          <Space direction="vertical" size={12} style={{ width: "100%" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <Typography.Text strong style={{ fontSize: 15 }}>平台选择</Typography.Text>
              {taskRunning ? (
                <Tag color="processing">{runningPlatformLabel} 运行中</Tag>
              ) : (
                <Tag color="success">就绪</Tag>
              )}
            </div>
            <Segmented<PlatformKind>
              block
              value={platform}
              disabled={taskRunning || checkPhase === "checking"}
              options={[
                { value: "boss", label: "BOSS 直聘" },
                { value: "liepin", label: "猎聘" },
              ]}
              onChange={(nextPlatform) => {
                setPlatform(nextPlatform);
                setLogFilter(nextPlatform);
                setCheckPhase("idle");
                setEnvResult(null);
                setCheckMsg("");
              }}
            />
            <div
              style={{
                borderLeft: `4px solid ${currentPlatform.accent}`,
                paddingLeft: 12,
                marginTop: 4,
                background: "rgba(248, 250, 252, 0.8)",
                padding: "10px 12px",
                borderRadius: "0 8px 8px 0",
              }}
            >
              <Typography.Title level={5} style={{ margin: 0, color: currentPlatform.accent }}>
                {currentPlatform.label}
              </Typography.Title>
              <Typography.Text type="secondary" style={{ fontSize: 12.5 }}>
                {currentPlatform.description}
              </Typography.Text>
            </div>
          </Space>
        </Card>

        <Card styles={{ body: { padding: 18 } }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "center" }}>
              <div>
                <Typography.Title level={5} style={{ margin: 0 }}>
                  {currentPlatform.shortLabel} 环境状态
                </Typography.Title>
                <Typography.Text type="secondary" style={{ fontSize: 12.5 }}>
                  确认浏览器驱动与平台登录凭证。
                </Typography.Text>
              </div>
              <Button
                type="primary"
                icon={checkPhase === "checking" ? <LoadingOutlined /> : <CheckCircleOutlined />}
                loading={checkPhase === "checking"}
                onClick={handleCheckEnv}
                disabled={taskRunning}
              >
                {checkPhase === "checking" ? "检查中..." : `检查${currentPlatform.shortLabel}环境`}
              </Button>
            </div>

            {showSteps ? (
              <Steps
                size="small"
                current={
                  checkPhase === "checking"
                    ? currentStepIndex
                    : checkPhase === "done"
                      ? ENV_STEPS.length - 1
                      : -1
                }
                items={ENV_STEPS.map((step, idx) => ({
                  title: step.title,
                  status: resolveStepStatus(
                    checkPhase,
                    envResult?.status ?? null,
                    idx,
                    currentStepIndex,
                  ),
                  icon:
                    checkPhase === "checking" && idx === currentStepIndex ? (
                      <LoadingOutlined />
                    ) : checkPhase === "done" &&
                      idx === currentStepIndex &&
                      envResult?.status === "login_required" ? (
                      <WarningOutlined />
                    ) : undefined,
                }))}
              />
            ) : (
              <Alert
                type="info"
                showIcon
                message={`当前选择 ${currentPlatform.label}`}
                description="准备就绪，点击右侧按钮测试环境联通性。"
              />
            )}

            {checkMsg && <Alert type="warning" showIcon message={checkMsg} />}

            {envResult?.message && !checkMsg && showSteps && (
              <Typography.Text type="secondary" style={{ fontSize: 13 }}>{envResult.message}</Typography.Text>
            )}

            {qrSrc && (
              <div
                style={{
                  padding: 14,
                  background: "#f8fafc",
                  borderRadius: 12,
                  display: "inline-flex",
                  flexDirection: "column",
                  alignItems: "center",
                  gap: 8,
                  border: "1px solid #e2e8f0",
                }}
              >
                <Image
                  src={qrSrc}
                  width={180}
                  height={180}
                  preview={false}
                  alt="平台登录二维码"
                  style={{ borderRadius: 8 }}
                />
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {envResult?.message ?? `请使用 ${currentPlatform.label} App 扫码登录`}
                </Typography.Text>
              </div>
            )}
          </div>
        </Card>
      </section>

      {/* ── Automation control bar ── */}
      <Card styles={{ body: { padding: 18 } }}>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            gap: 16,
            alignItems: "center",
            flexWrap: "wrap",
          }}
        >
          <div>
            <Typography.Title level={5} style={{ margin: 0 }}>
              {currentPlatform.shortLabel} 自动化控制台
            </Typography.Title>
            <Typography.Text type="secondary" style={{ fontSize: 13 }}>
              根据预设筛选条件、简历与回复策略，安全发起求职流。
            </Typography.Text>
            {currentPlatform.limitation && (
              <Typography.Text type="warning" style={{ display: "block", marginTop: 4, fontSize: 12 }}>
                {currentPlatform.limitation}
              </Typography.Text>
            )}
          </div>
          <Space wrap>
            <Button
              type="primary"
              size="large"
              icon={<PlayCircleOutlined />}
              disabled={taskRunning}
              onClick={() => setStartModalOpen(true)}
            >
              启动 {currentPlatform.shortLabel} 任务
            </Button>
            <Button
              danger
              size="large"
              icon={<StopOutlined />}
              onClick={handleStopTask}
              disabled={!taskRunning}
            >
              停止任务
            </Button>
            {taskRunning && (
              <Typography.Text type="warning" style={{ alignSelf: "center", fontWeight: 500 }}>
                {runningPlatformLabel} 任务运行中...
              </Typography.Text>
            )}
          </Space>
        </div>
      </Card>

      {/* ── Start modal ── */}
      <Modal
        title={`启动 ${currentPlatform.label} 任务`}
        open={startModalOpen}
        onOk={() => void handleStartConfirm()}
        confirmLoading={preflightLoading}
        onCancel={() => setStartModalOpen(false)}
        okText="确认启动"
        cancelText="取消"
        okButtonProps={{
          disabled:
            taskRunning ||
            (selectedMode === "periodic_job_hunting" &&
              (!intervalMinutes || intervalMinutes <= 0)),
        }}
      >
        <Space direction="vertical" size={16} style={{ width: "100%", paddingTop: 8 }}>
          {readiness && (
            <Alert
              type={readiness.ready ? "success" : "warning"}
              showIcon
              message={readiness.ready ? "环境与配置预检通过" : "尚有配置未完善"}
              description={
                <Space direction="vertical" size={6} style={{ width: "100%", marginTop: 4 }}>
                  {readiness.summary.map((line) => <Typography.Text key={line}>{line}</Typography.Text>)}
                  {readiness.items.map((item) => (
                    <div key={item.key} style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
                      <Typography.Text type={item.level === "blocked" ? "danger" : item.level === "warning" ? "warning" : "success"}>
                        {item.label}：{item.message}
                      </Typography.Text>
                      {item.level !== "ready" && item.config_group && onOpenConfig && (
                        <Button size="small" type="link" onClick={() => onOpenConfig(item.config_group as "resume" | "llm" | "job" | "greet" | "reply" | "browser")}>
                          前往配置
                        </Button>
                      )}
                    </div>
                  ))}
                </Space>
              }
            />
          )}
          <Radio.Group
            value={selectedMode}
            onChange={(event) => setSelectedMode(event.target.value)}
            style={{ width: "100%" }}
          >
            <Space direction="vertical" style={{ width: "100%" }} size={12}>
              {FLOW_MODE_OPTIONS.map((option) => (
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
              <InputNumber
                min={1}
                max={1440}
                value={intervalMinutes}
                onChange={(v) => setIntervalMinutes(v ?? 30)}
                addonAfter="分钟"
                style={{ width: 160 }}
              />
            </div>
          )}
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
            <Tag color={logFilter === "all" ? "default" : "processing"}>
              {logFilter === "all" ? "全部平台" : `${PLATFORM_META[logFilter].shortLabel}视图`}
            </Tag>
          </Space>
          <Space>
            <Button size="small" icon={logCollapsed ? <EyeOutlined /> : <EyeInvisibleOutlined />} onClick={() => setLogCollapsed(!logCollapsed)}>
              {logCollapsed ? "展开" : "收起"}
            </Button>
            {!logCollapsed && (
              <>
                <Button size="small" icon={<CopyOutlined />} onClick={handleCopyLog}>复制</Button>
                <Segmented<LogFilter>
                  size="small"
                  value={logFilter}
                  options={LOG_FILTER_OPTIONS}
                  onChange={setLogFilter}
                />
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
