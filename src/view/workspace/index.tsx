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
  LoadingOutlined,
  PlayCircleOutlined,
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

const WorkspacePage = ({
  onNavigate,
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
  const logRef = useRef<HTMLPreElement>(null);
  const [messageApi, contextHolder] = message.useMessage();

  const refreshTaskStatus = useCallback(async () => {
    try {
      const result = await invoke<CommandResult<JobTaskStatus>>(
        "get_job_task_status",
      );
      if (result.success && result.data) {
        setTaskRunning(result.data.running);
        if (!result.data.running) {
          setRunningPlatform(null);
        }
      }
    } catch {
      // ignore
    }
  }, []);

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
    void refreshLog();
    void refreshTaskStatus();
  }, [refreshLog, refreshTaskStatus]);

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

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", gap: 20 }}>
      {contextHolder}

      <section style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))", gap: 12 }}>
        <Card size="small" title="今日行动">
          <Typography.Text type="secondary">从准备检查开始，避免任务运行后才发现配置缺失。</Typography.Text>
          <div style={{ marginTop: 12 }}><Button type="primary" onClick={() => setStartModalOpen(true)} disabled={taskRunning}>启动任务预检</Button></div>
        </Card>
        <Card size="small" title="岗位管理">
          <Typography.Text type="secondary">查看岗位、沟通记录和面试分析。</Typography.Text>
          <div style={{ marginTop: 12 }}><Button onClick={() => onNavigate?.("job-data")}>打开岗位管理</Button></div>
        </Card>
        <Card size="small" title="配置准备">
          <Typography.Text type="secondary">浏览器、筛选、简历和话术统一配置。</Typography.Text>
          <div style={{ marginTop: 12 }}><Button onClick={() => onOpenConfig?.("browser")}>检查配置</Button></div>
        </Card>
      </section>

      <section
        style={{
          flex: "0 0 auto",
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(280px, 1fr))",
          gap: 16,
          alignItems: "stretch",
        }}
      >
        <div
          style={{
            border: "1px solid #e5e7eb",
            borderRadius: 8,
            padding: 16,
            display: "flex",
            flexDirection: "column",
            gap: 12,
          }}
        >
          <Typography.Text strong>平台工作区</Typography.Text>
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
              borderLeft: `3px solid ${currentPlatform.accent}`,
              paddingLeft: 10,
              minHeight: 62,
            }}
          >
            <Typography.Title level={4} style={{ margin: 0 }}>
              {currentPlatform.label}
            </Typography.Title>
            <Typography.Text type="secondary">
              {currentPlatform.description}
            </Typography.Text>
          </div>
          {taskRunning ? (
            <Tag color="processing">{runningPlatformLabel}任务运行中，平台已锁定</Tag>
          ) : (
            <Tag color="default">空闲，可切换平台</Tag>
          )}
        </div>

        <div
          style={{
            border: "1px solid #e5e7eb",
            borderRadius: 8,
            padding: 16,
            display: "flex",
            flexDirection: "column",
            gap: 12,
          }}
        >
          <div style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
            <div>
              <Typography.Title level={5} style={{ margin: 0 }}>
                {currentPlatform.shortLabel} 环境状态
              </Typography.Title>
              <Typography.Text type="secondary">
                启动任务前先确认浏览器和登录状态。
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
              description="环境检查结果会按当前平台展示，任务运行时平台切换会被锁定。"
            />
          )}

          {checkMsg && <Alert type="warning" showIcon message={checkMsg} />}

          {envResult?.message && !checkMsg && showSteps && (
            <Typography.Text type="secondary">{envResult.message}</Typography.Text>
          )}

          {qrSrc && (
            <div
              style={{
                padding: 16,
                background: "#fafafa",
                borderRadius: 8,
                display: "inline-flex",
                flexDirection: "column",
                alignItems: "center",
                gap: 8,
                alignSelf: "flex-start",
              }}
            >
              <Image
                src={qrSrc}
                width={200}
                height={200}
                preview={false}
                alt="平台登录二维码"
              />
              <Typography.Text type="secondary">
                {envResult?.message ?? `请使用${currentPlatform.label} App 扫码登录`}
              </Typography.Text>
            </div>
          )}
        </div>
      </section>

      <section
        style={{
          flex: "0 0 auto",
          border: "1px solid #e5e7eb",
          borderRadius: 8,
          padding: 16,
          display: "flex",
          justifyContent: "space-between",
          gap: 16,
          alignItems: "center",
          flexWrap: "wrap",
        }}
      >
        <div>
          <Typography.Title level={5} style={{ margin: 0 }}>
            {currentPlatform.shortLabel} 自动求职
          </Typography.Title>
          <Typography.Text type="secondary">
            使用配置中心的筛选条件、简历和话术启动当前平台任务。
          </Typography.Text>
          {currentPlatform.limitation && (
            <Typography.Text type="warning" style={{ display: "block", marginTop: 4 }}>
              {currentPlatform.limitation}
            </Typography.Text>
          )}
        </div>
        <Space wrap>
          <Button
            type="primary"
            icon={<PlayCircleOutlined />}
            disabled={taskRunning}
            onClick={() => setStartModalOpen(true)}
          >
            启动{currentPlatform.shortLabel}任务
          </Button>
          <Button
            danger
            icon={<StopOutlined />}
            onClick={handleStopTask}
            disabled={!taskRunning}
          >
            停止任务
          </Button>
          {taskRunning && (
            <Typography.Text type="warning" style={{ alignSelf: "center" }}>
              {runningPlatformLabel}任务运行中...
            </Typography.Text>
          )}
        </Space>
      </section>

      <Modal
        title={`启动${currentPlatform.label}任务`}
        open={startModalOpen}
        onOk={() => void handleStartConfirm()}
        confirmLoading={preflightLoading}
        onCancel={() => setStartModalOpen(false)}
        okText="启动"
        cancelText="取消"
        okButtonProps={{
          disabled:
            taskRunning ||
            (selectedMode === "periodic_job_hunting" &&
              (!intervalMinutes || intervalMinutes <= 0)),
        }}
      >
        <Space direction="vertical" size={16} style={{ width: "100%" }}>
          {/* <Alert
            type="info"
            showIcon
            message={`当前平台：${currentPlatform.label}`}
            description="任务会使用配置中心当前保存的筛选条件、简历和自动回复资源。"
          /> */}
          {readiness && (
            <Alert
              type={readiness.ready ? "success" : "warning"}
              showIcon
              message={readiness.ready ? "启动准备已完成" : "还有项目需要处理"}
              description={
                <Space direction="vertical" size={6} style={{ width: "100%" }}>
                  {readiness.summary.map((line) => <Typography.Text key={line}>{line}</Typography.Text>)}
                  {readiness.items.map((item) => (
                    <div key={item.key} style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
                      <Typography.Text type={item.level === "blocked" ? "danger" : item.level === "warning" ? "warning" : "success"}>
                        {item.label}：{item.message}
                      </Typography.Text>
                      {item.level !== "ready" && item.config_group && onOpenConfig && (
                        <Button size="small" type="link" onClick={() => onOpenConfig(item.config_group as "resume" | "llm" | "job" | "greet" | "reply" | "browser")}>
                          去配置
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
            <Space direction="vertical" style={{ width: "100%" }}>
              {FLOW_MODE_OPTIONS.map((option) => (
                <Radio key={option.key} value={option.key}>
                  <Space direction="vertical" size={0}>
                    <Typography.Text strong>{option.label}</Typography.Text>
                    <Typography.Text type="secondary">{option.description}</Typography.Text>
                  </Space>
                </Radio>
              ))}
            </Space>
          </Radio.Group>

          {selectedMode === "periodic_job_hunting" && (
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <Typography.Text>每轮投递间隔</Typography.Text>
              <InputNumber
                min={1}
                max={1440}
                value={intervalMinutes}
                onChange={(v) => setIntervalMinutes(v ?? 30)}
                addonAfter="分钟"
                style={{ width: 180 }}
              />
            </div>
          )}
        </Space>
      </Modal>

      <section style={{ flex: "1 1 0", minHeight: 0, display: "flex", flexDirection: "column" }}>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            gap: 12,
            marginBottom: 8,
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
          <Segmented<LogFilter>
            size="small"
            value={logFilter}
            options={LOG_FILTER_OPTIONS}
            onChange={setLogFilter}
          />
        </div>
        <pre
          ref={logRef}
          style={{
            flex: 1,
            minHeight: 200,
            margin: 0,
            padding: 12,
            background: "#1e1e1e",
            color: "#d4d4d4",
            borderRadius: 8,
            fontFamily: "'Menlo', 'Monaco', 'Courier New', monospace",
            fontSize: 12,
            lineHeight: 1.6,
            overflowY: "auto",
            whiteSpace: "pre-wrap",
            wordBreak: "break-all",
          }}
        >
          {filteredLogContent || "暂无匹配日志"}
        </pre>
      </section>
    </div>
  );
};

export default WorkspacePage;
