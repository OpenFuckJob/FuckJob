import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Button,
  Dropdown,
  Input,
  Modal,
  Popconfirm,
  Segmented,
  Select,
  Space,
  Table,
  Tabs,
  Tag,
  Typography,
  message,
} from "antd";
import {
  CalendarOutlined,
  DeleteOutlined,
  DownOutlined,
  DownloadOutlined,
  EllipsisOutlined,
  EyeOutlined,
  FileTextOutlined,
  InboxOutlined,
  LeftOutlined,
  LinkOutlined,
  MessageOutlined,
  RightOutlined,
  SearchOutlined,
  StopOutlined,
  ThunderboltOutlined,
} from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import type { ColumnsType } from "antd/es/table";
import type { CommandResult } from "../../types/command";
import { commandErrorMessage } from "../../types/command";
import type {
  CommunicationStatus,
  JobDetail,
  JobListItem,
  JobPipelineStage,
  JobPlatform,
  PlatformSyncMeta,
  SyncStatusSummary,
} from "../../types/job-detail";
import type { InterviewJobAnalysis } from "../../types/analysis";
import { DEFAULT_HIGH_MATCH_SCORE } from "../../types/app-config";
import AnalysisReport from "./AnalysisReport";
import ChatThreadModal from "./ChatThread";
import JobBrief from "./JobBrief";
import { useJobDataRefresh } from "../../hooks/useJobDataRefresh";
import { IconRefreshSync, PlatformBadgeIcon } from "../../components/icons";
import "./style.css";

/** 与 Rust 侧 BatchAnalysisResult 对应 */
interface BatchAnalysisResult {
  analyzed: number;
  skipped: number;
  failed: number;
  failures: string[];
}

const getJobPlatform = (job: JobDetail): "boss" | "liepin" | "51job" =>
  job.platform === "51job" || job.id.startsWith("51job:") ? "51job" :
  job.platform === "liepin" || job.id.startsWith("liepin:")
    ? "liepin"
    : "boss";

type AnalysisFilter = "all" | "analyzed" | "not_analyzed" | "high_match";

/** 与分析报告里的评分配色保持一致 */
const matchScoreColor = (score: number): string =>
  score >= 80 ? "green" : score >= 60 ? "gold" : "red";

/** 列表和看板共用的匹配度展示：没分析过要一眼看得出来 */
function MatchScoreTag({ analysis }: { analysis?: InterviewJobAnalysis }) {
  if (!analysis) {
    return <Tag className="match-tag is-empty">未分析</Tag>;
  }
  if (analysis.parse_error) {
    return <Tag color="orange">解析失败</Tag>;
  }
  return (
    <Tag color={matchScoreColor(analysis.match_score)} className="match-tag">
      {analysis.match_score} 分
    </Tag>
  );
}

const COMMUNICATION_STATUS_META: Record<
  CommunicationStatus,
  { label: string; color: string }
> = {
  rejected: { label: "明确拒绝", color: "red" },
  interview_invited: { label: "收到约面", color: "purple" },
  replied: { label: "已回复", color: "green" },
  no_reply: { label: "未回复", color: "orange" },
};

const renderCommunicationStatus = (status: CommunicationStatus) => {
  const meta = COMMUNICATION_STATUS_META[status];
  return <Tag color={meta.color}>{meta.label}</Tag>;
};

const formatLatestMessageTime = (time: number): string =>
  new Date(time).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });

const renderLatestMessage = (job: JobListItem) => {
  if (!job.latest_message?.trim()) {
    return "-";
  }

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
      {typeof job.latest_message_received === "boolean" && (
        <Tag color={job.latest_message_received ? "green" : "blue"} style={{ margin: 0 }}>
          {job.latest_message_received ? "招聘方" : "我"}
        </Tag>
      )}
      {job.latest_message_at != null && (
        <Typography.Text type="secondary" style={{ flex: "0 0 auto", fontSize: 12 }}>
          {formatLatestMessageTime(job.latest_message_at)}
        </Typography.Text>
      )}
      <Typography.Text ellipsis title={job.latest_message} style={{ minWidth: 0, flex: 1 }}>
        {job.latest_message}
      </Typography.Text>
    </div>
  );
};
/* ────────── CRM stage & Kanban lane config ────────── */

const STAGE_META: Record<JobPipelineStage, { label: string; color: string }> = {
  pending: { label: "已打招呼", color: "default" },
  applied: { label: "已发简历", color: "blue" },
  replied: { label: "HR 已回复", color: "green" },
  interview_scheduled: { label: "已约面", color: "cyan" },
  interviewing: { label: "面试中", color: "purple" },
  offer: { label: "已拿 Offer", color: "gold" },
  rejected: { label: "已婉拒", color: "orange" },
  archived: { label: "已归档", color: "default" },
};

const STAGE_OPTIONS: Array<{ value: JobPipelineStage; label: string }> = (
  Object.entries(STAGE_META) as Array<[JobPipelineStage, { label: string; color: string }]>
).map(([value, meta]) => ({ value, label: meta.label }));

export function resolveEffectiveStage(job: JobListItem): JobPipelineStage {
  if (job.effective_stage) return job.effective_stage;
  if (job.pipeline_stage) return job.pipeline_stage;
  if (job.communication_status === "rejected") return "rejected";
  if (job.communication_status === "interview_invited") return "interview_scheduled";
  if (
    job.communication_status === "replied" ||
    job.is_reply ||
    (job.unread_count ?? 0) > 0
  ) {
    return "replied";
  }
  if (job.is_send_resume) return "applied";
  return "pending";
}

interface KanbanLane {
  key: string;
  label: string;
  color: string;
  hint: string;
  stages: JobPipelineStage[];
  primaryStage: JobPipelineStage;
  collapsible?: boolean;
}

/** 6 列交互式求职 CRM 泳道，支持拖拽卡片跨列流转、零点击未读角标与折叠归档列 */
const KANBAN_LANES: KanbanLane[] = [
  {
    key: "pending",
    label: "已打招呼",
    color: "#94a3b8",
    hint: "已建联打招呼，尚未发送附件简历",
    stages: ["pending"],
    primaryStage: "pending",
  },
  {
    key: "applied",
    label: "已发简历",
    color: "#4f46e5",
    hint: "已投递附件简历，等待 HR 回应",
    stages: ["applied"],
    primaryStage: "applied",
  },
  {
    key: "replied",
    label: "HR 已回复",
    color: "#10b981",
    hint: "对方已回应或有未读消息，优先推进约面",
    stages: ["replied"],
    primaryStage: "replied",
  },
  {
    key: "interview",
    label: "约面与面试",
    color: "#8b5cf6",
    hint: "自动识别约面邀约或手动推进至此",
    stages: ["interview_scheduled", "interviewing"],
    primaryStage: "interview_scheduled",
  },
  {
    key: "offer",
    label: "Offer",
    color: "#eab308",
    hint: "已拿到 Offer 或谈薪阶段",
    stages: ["offer"],
    primaryStage: "offer",
  },
  {
    key: "closed",
    label: "已婉拒 / 归档",
    color: "#f97316",
    hint: "明确不合适或主动归档",
    stages: ["rejected", "archived"],
    primaryStage: "rejected",
    collapsible: true,
  },
];

function groupByLane(jobs: JobListItem[]): Array<KanbanLane & { jobs: JobListItem[] }> {
  const lanes = KANBAN_LANES.map((lane) => ({ ...lane, jobs: [] as JobListItem[] }));
  for (const job of jobs) {
    const stage = resolveEffectiveStage(job);
    const lane = lanes.find((candidate) => candidate.stages.includes(stage)) ?? lanes[0];
    lane.jobs.push(job);
  }
  return lanes;
}

/* ────────── Job card renderer ────────── */

function JobKanbanCard({
  job,
  analysis,
  dragging,
  onDragStart,
  onDragEnd,
  onView,
  onChat,
  onDelete,
  onChangeStage,
}: {
  job: JobListItem;
  analysis?: InterviewJobAnalysis;
  dragging: boolean;
  onDragStart: (jobId: string) => void;
  onDragEnd: () => void;
  onView: (job: JobDetail) => void;
  onChat: (job: JobDetail) => void;
  onDelete: (id: string) => void;
  onChangeStage: (job: JobListItem, stage: JobPipelineStage) => void;
}) {
  const platform = getJobPlatform(job);
  const stage = resolveEffectiveStage(job);
  const time = new Date(job.created_at).toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });

  return (
    <div
      className={`job-card${dragging ? " is-dragging" : ""}`}
      role="button"
      tabIndex={0}
      draggable
      title="点击查看详情与笔记，拖拽卡片可切换推进阶段"
      onDragStart={(event) => {
        event.dataTransfer.setData("text/plain", job.id);
        event.dataTransfer.effectAllowed = "move";
        onDragStart(job.id);
      }}
      onDragEnd={onDragEnd}
      onClick={() => onView(job)}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onView(job);
        }
      }}
    >
      <div className="job-card-top">
        <div className="job-card-title">{job.title}</div>
        {(job.unread_count ?? 0) > 0 && (
          <span className="job-card-unread" title="零点击同步检测到未读消息（手机端仍保留未读红点）">
            未读 {job.unread_count}
          </span>
        )}
        <MatchScoreTag analysis={analysis} />
      </div>

      <div className="job-card-company">
        <PlatformBadgeIcon platform={platform} size={15} />
        <span>{job.company_name}</span>
      </div>

      <div className="job-card-tags">
        {job.salary && <span className="job-card-chip salary">{job.salary}</span>}
        {job.location && <span className="job-card-chip">{job.location}</span>}
        {job.communication_status === "interview_invited" && (
          <span className="job-card-invite">收到约面</span>
        )}
        {job.interview_at && (
          <span className="job-card-chip" title={`面试时间：${job.interview_at}`}>
            <CalendarOutlined /> {job.interview_at}
          </span>
        )}
        {job.notes && (
          <span className="job-card-chip" title={job.notes}>
            <FileTextOutlined /> 有笔记
          </span>
        )}
      </div>

      {job.latest_message?.trim() && (
        <div
          className={`job-card-preview${job.latest_message_received === true ? " is-received" : ""}`}
          title={job.latest_message}
        >
          {typeof job.latest_message_received === "boolean"
            ? job.latest_message_received
              ? "HR："
              : "我："
            : ""}
          {job.latest_message}
        </div>
      )}

      <div className="job-card-foot">
        <div
          style={{ display: "flex", alignItems: "center", gap: 6 }}
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
        >
          <Dropdown
            trigger={["click"]}
            menu={{
              selectedKeys: [stage],
              items: STAGE_OPTIONS.map((opt) => ({
                key: opt.value,
                label: opt.label,
                onClick: () => onChangeStage(job, opt.value),
              })),
            }}
          >
            <button type="button" className="job-card-stage-pill" title="点击切换推进阶段">
              <span>{STAGE_META[stage].label}</span>
              <DownOutlined style={{ fontSize: 9 }} />
            </button>
          </Dropdown>
          <span className="job-card-time">{time}</span>
        </div>
        {/* 阻止冒泡：这些按钮各有各的动作，不该顺带打开详情 */}
        <div
          className="job-card-actions"
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
        >
          <Button
            size="small"
            type="text"
            icon={<MessageOutlined />}
            title="沟通记录"
            onClick={() => onChat(job)}
          />
          <Popconfirm
            title="确认删除"
            description={`确定要删除「${job.title}」吗？`}
            onConfirm={() => onDelete(job.id)}
            okText="确认删除"
            cancelText="取消"
            okButtonProps={{ danger: true }}
          >
            <Button size="small" type="text" danger icon={<DeleteOutlined />} title="删除" />
          </Popconfirm>
        </div>
      </div>
    </div>
  );
}

/* ────────── Kanban view ────────── */

function KanbanView({
  jobs,
  analyses,
  onView,
  onChat,
  onDelete,
  onChangeStage,
}: {
  jobs: JobListItem[];
  analyses: Record<string, InterviewJobAnalysis>;
  onView: (job: JobDetail) => void;
  onChat: (job: JobDetail) => void;
  onDelete: (id: string) => void;
  onChangeStage: (job: JobListItem, stage: JobPipelineStage) => void;
}) {
  const lanes = useMemo(() => groupByLane(jobs), [jobs]);
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dragOverLane, setDragOverLane] = useState<string | null>(null);
  const [closedCollapsed, setClosedCollapsed] = useState(false);

  const handleDropOnLane = (lane: KanbanLane, event: React.DragEvent) => {
    event.preventDefault();
    const jobId = event.dataTransfer.getData("text/plain") || draggingId;
    setDraggingId(null);
    setDragOverLane(null);
    if (!jobId) return;
    const job = jobs.find((item) => item.id === jobId);
    if (!job) return;
    const currentStage = resolveEffectiveStage(job);
    if (lane.stages.includes(currentStage)) return;
    onChangeStage(job, lane.primaryStage);
  };

  return (
    <div className="kanban-board">
      {lanes.map((lane) => {
        const isCollapsed = Boolean(lane.collapsible && closedCollapsed);
        const isDragOver = dragOverLane === lane.key;

        if (isCollapsed) {
          return (
            <div
              key={lane.key}
              className={`kanban-lane is-collapsed${isDragOver ? " is-drag-over" : ""}`}
              title={`展开「${lane.label}」（${lane.jobs.length} 条），也可直接拖入卡片归档`}
              onClick={() => setClosedCollapsed(false)}
              onDragOver={(event) => {
                event.preventDefault();
                setDragOverLane(lane.key);
              }}
              onDragLeave={() => setDragOverLane((prev) => (prev === lane.key ? null : prev))}
              onDrop={(event) => handleDropOnLane(lane, event)}
            >
              <LeftOutlined style={{ fontSize: 11, color: "var(--jd-muted)" }} />
              <span className="kanban-lane-dot" style={{ background: lane.color }} />
              <span className="kanban-lane-count" style={{ marginLeft: 0 }}>{lane.jobs.length}</span>
              <span className="kanban-collapsed-title">{lane.label}</span>
            </div>
          );
        }

        return (
          <div
            className={`kanban-lane${isDragOver ? " is-drag-over" : ""}`}
            key={lane.key}
            onDragOver={(event) => {
              event.preventDefault();
              event.dataTransfer.dropEffect = "move";
              if (dragOverLane !== lane.key) setDragOverLane(lane.key);
            }}
            onDragLeave={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
                setDragOverLane((prev) => (prev === lane.key ? null : prev));
              }
            }}
            onDrop={(event) => handleDropOnLane(lane, event)}
          >
            <div className="kanban-lane-head" title={lane.hint}>
              <span className="kanban-lane-dot" style={{ background: lane.color }} />
              <span className="kanban-lane-title">{lane.label}</span>
              <span className="kanban-lane-count">{lane.jobs.length}</span>
              {lane.collapsible && (
                <Button
                  type="text"
                  size="small"
                  icon={<RightOutlined style={{ fontSize: 11 }} />}
                  title="折叠此列腾出更多看板空间"
                  onClick={() => setClosedCollapsed(true)}
                  style={{ width: 22, height: 22, padding: 0 }}
                />
              )}
            </div>
            <div className="kanban-lane-body">
              {lane.jobs.length === 0 ? (
                <div className="kanban-empty">
                  <InboxOutlined />
                  <span>{isDragOver ? "松开移入此阶段" : lane.hint}</span>
                </div>
              ) : (
                lane.jobs.map((job) => (
                  <JobKanbanCard
                    key={job.id}
                    job={job}
                    analysis={analyses[job.id]}
                    dragging={draggingId === job.id}
                    onDragStart={setDraggingId}
                    onDragEnd={() => {
                      setDraggingId(null);
                      setDragOverLane(null);
                    }}
                    onView={onView}
                    onChat={onChat}
                    onDelete={onDelete}
                    onChangeStage={onChangeStage}
                  />
                ))
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/* ────────── Page ────────── */

type FocusFilter = "all" | "actionable" | "waiting";

const ACTIONABLE_STAGES: ReadonlySet<JobPipelineStage> = new Set([
  "replied",
  "interview_scheduled",
  "interviewing",
  "offer",
]);

const JobDataPage = ({ aiConfigured, llmConfigured, onConfigureAi, focusJobId, onFocusHandled, highMatchScore = DEFAULT_HIGH_MATCH_SCORE, onBlacklistCompany }: {
  aiConfigured: boolean;
  llmConfigured: boolean;
  onConfigureAi: () => void;
  /** 从其他页面跳转过来时需要直接打开沟通记录的岗位 */
  focusJobId?: string;
  onFocusHandled?: () => void;
  /** 高匹配的判定分数线，与求职方案的岗位分析配置一致 */
  highMatchScore?: number;
  /** 一键将公司名追加到当前求职方案的排除公司列表 */
  onBlacklistCompany?: (
    companyName: string,
  ) => void | boolean | { ok: boolean; warning?: string };
}) => {
  const [jobs, setJobs] = useState<JobListItem[]>([]);
  // 初始即为加载中，避免首次挂载时把待跳转的岗位当成「不存在」
  const [loading, setLoading] = useState(true);
  const [keyword, setKeyword] = useState("");
  const [analyses, setAnalyses] = useState<Record<string, InterviewJobAnalysis>>({});
  const [focusFilter, setFocusFilter] = useState<FocusFilter>("all");
  const [platformFilter, setPlatformFilter] = useState<JobPlatform | "all">("all");
  const [stageFilter, setStageFilter] = useState<JobPipelineStage | "all">("all");
  const [communicationStatusFilter, setCommunicationStatusFilter] =
    useState<CommunicationStatus | "all">("all");
  const [analysisFilter, setAnalysisFilter] = useState<AnalysisFilter>("all");
  const [currentJob, setCurrentJob] = useState<JobDetail | null>(null);
  const [chatJob, setChatJob] = useState<JobDetail | null>(null);
  const [viewMode, setViewMode] = useState<"table" | "kanban">("kanban");
  const [selectedJobIds, setSelectedJobIds] = useState<React.Key[]>([]);
  const [batchAnalyzing, setBatchAnalyzing] = useState(false);
  const [batchDeleting, setBatchDeleting] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [syncMetaList, setSyncMetaList] = useState<PlatformSyncMeta[]>([]);
  const [messageApi, contextHolder] = message.useMessage();
  const jobsRequest = useRef(0);
  const analysesRequest = useRef(0);

  const loadSyncStatus = useCallback(async () => {
    try {
      const result = await invoke<CommandResult<PlatformSyncMeta[]>>("job_sync_status");
      if (result.success && result.data) {
        setSyncMetaList(result.data);
      }
    } catch {
      // ignore
    }
  }, []);

  const handleSyncStatus = useCallback(
    async (targetPlatform?: "boss" | "liepin") => {
      if (syncing) return;
      setSyncing(true);
      try {
        const result = await invoke<CommandResult<SyncStatusSummary>>("sync_platform_status", {
          platform: targetPlatform ?? null,
        });
        if (!result.success || !result.data) {
          messageApi.error(commandErrorMessage(result.error, "同步状态失败"));
          return;
        }
        const summary = result.data;
        const parts = [
          `扫描 ${summary.scanned} 个会话`,
          summary.updated > 0 ? `更新 ${summary.updated} 个岗位` : "状态均已最新",
          summary.unread_total > 0 ? `未读 ${summary.unread_total} 条（保留红点）` : "",
          summary.interview_detected > 0 ? `识别 ${summary.interview_detected} 条约面` : "",
        ].filter(Boolean);
        if (summary.warnings.length > 0) {
          messageApi.warning(`${parts.join(" · ")}（${summary.warnings.join("；")}）`);
        } else {
          messageApi.success(`零点击同步完成：${parts.join(" · ")}`);
        }
        await Promise.all([loadSyncStatus()]);
      } catch (error: unknown) {
        messageApi.error(error instanceof Error ? error.message : "同步状态失败");
      } finally {
        setSyncing(false);
      }
    },
    [loadSyncStatus, messageApi, syncing],
  );

  const loadJobs = useCallback(async (showLoading = true) => {
    const request = ++jobsRequest.current;
    if (showLoading) setLoading(true);
    try {
      const result = await invoke<CommandResult<JobListItem[]>>(
        "job_list_with_status",
      );
      if (request !== jobsRequest.current) return;
      if (!result.success || result.data === null) {
        messageApi.error(
          commandErrorMessage(result.error, "加载岗位数据失败"),
        );
        return;
      }
      const sorted = [...result.data].sort((a, b) =>
        b.created_at.localeCompare(a.created_at),
      );
      setJobs(sorted);
      const validIds = new Set(sorted.map((job) => job.id));
      setSelectedJobIds((prev) => {
        const next = prev.filter((key) => validIds.has(String(key)));
        return next.length === prev.length ? prev : next;
      });
      setCurrentJob(current => current ? sorted.find(job => job.id === current.id) ?? current : null);
    } catch (error: unknown) {
      if (request !== jobsRequest.current) return;
      messageApi.error(
        error instanceof Error ? error.message : "加载岗位数据失败",
      );
    } finally {
      if (request === jobsRequest.current) setLoading(false);
    }
  }, [messageApi]);

  /// 分析结果只是列表上的附加信息，取不到不该影响岗位管理本身
  const loadAnalyses = useCallback(async () => {
    const request = ++analysesRequest.current;
    try {
      const result = await invoke<CommandResult<InterviewJobAnalysis[]>>("analysis_list");
      if (request !== analysesRequest.current) return;
      if (!result.success || !result.data) return;
      setAnalyses(
        Object.fromEntries(result.data.map((item) => [item.job_id, item])),
      );
    } catch {
      if (request === analysesRequest.current) setAnalyses({});
    }
  }, []);

  useJobDataRefresh(useCallback(async () => {
    await Promise.all([loadJobs(false), loadAnalyses(), loadSyncStatus()]);
  }, [loadAnalyses, loadJobs, loadSyncStatus]));

  useEffect(() => {
    void loadJobs();
    void loadAnalyses();
    void loadSyncStatus();
  }, [loadAnalyses, loadJobs, loadSyncStatus]);

  useEffect(() => {
    if (!focusJobId || loading) return;
    const target = jobs.find((job) => job.id === focusJobId);
    if (target) {
      setCurrentJob(null);
      setChatJob(target);
    } else if (jobs.length > 0) {
      messageApi.warning("未找到该岗位，可能已被删除");
    }
    onFocusHandled?.();
  }, [focusJobId, jobs, loading, messageApi, onFocusHandled]);

  const handleDelete = useCallback(
    async (id: string) => {
      try {
        const result = await invoke<CommandResult<null>>("job_delete", { id });
        if (!result.success) {
          messageApi.error(commandErrorMessage(result.error, "删除失败"));
          return;
        }
        messageApi.success("删除成功");
        setSelectedJobIds((prev) => prev.filter((key) => String(key) !== id));
        void loadJobs();
      } catch (error: unknown) {
        messageApi.error(
          error instanceof Error ? error.message : "删除失败",
        );
      }
    },
    [loadJobs, messageApi],
  );

  const handleBatchDelete = useCallback(async () => {
    const ids = selectedJobIds.map(String);
    if (ids.length === 0) return;
    setBatchDeleting(true);
    try {
      const result = await invoke<CommandResult<number>>("job_batch_delete", { ids });
      if (!result.success) {
        messageApi.error(commandErrorMessage(result.error, "批量删除失败"));
        return;
      }
      messageApi.success(`已删除 ${result.data ?? ids.length} 条岗位记录`);
      setSelectedJobIds([]);
      void loadJobs();
    } catch (error: unknown) {
      messageApi.error(error instanceof Error ? error.message : "批量删除失败");
    } finally {
      setBatchDeleting(false);
    }
  }, [loadJobs, messageApi, selectedJobIds]);

  const handleChangeStage = useCallback(
    async (job: JobListItem, nextStage: JobPipelineStage) => {
      const {
        communication_status: _cs,
        latest_message: _lm,
        latest_message_at: _lma,
        latest_message_received: _lmr,
        ...detail
      } = job;
      const updated: JobDetail = { ...detail, pipeline_stage: nextStage };
      try {
        const result = await invoke<CommandResult<void>>("job_update", {
          id: job.id,
          job: updated,
        });
        if (!result.success) {
          messageApi.error(commandErrorMessage(result.error, "更新推进阶段失败"));
          return;
        }
        setJobs((prev) =>
          prev.map((item) =>
            item.id === job.id
              ? { ...item, pipeline_stage: nextStage, effective_stage: nextStage }
              : item,
          ),
        );
        messageApi.success(`已更新为「${STAGE_META[nextStage].label}」`);
      } catch (error: unknown) {
        messageApi.error(error instanceof Error ? error.message : "更新推进阶段失败");
      }
    },
    [messageApi],
  );

  const normalizedKeyword = keyword.trim().toLowerCase();
  const actionableCount = useMemo(
    () => jobs.filter((job) => ACTIONABLE_STAGES.has(resolveEffectiveStage(job))).length,
    [jobs],
  );
  const waitingCount = useMemo(
    () =>
      jobs.filter((job) => {
        const s = resolveEffectiveStage(job);
        return s === "pending" || s === "applied";
      }).length,
    [jobs],
  );

  const filteredJobs = useMemo(
    () =>
      jobs.filter((job) => {
        const effectiveStage = resolveEffectiveStage(job);
        if (focusFilter === "actionable" && !ACTIONABLE_STAGES.has(effectiveStage)) {
          return false;
        }
        if (focusFilter === "waiting" && effectiveStage !== "pending" && effectiveStage !== "applied") {
          return false;
        }
        const analysis = analyses[job.id];
        const matchesAnalysis =
          analysisFilter === "analyzed"
            ? !!analysis
            : analysisFilter === "not_analyzed"
              ? !analysis
              : analysisFilter === "high_match"
                ? !!analysis && !analysis.parse_error && analysis.match_score >= highMatchScore
                : true;
        return (
          (!normalizedKeyword ||
            job.title.toLowerCase().includes(normalizedKeyword) ||
            job.company_name.toLowerCase().includes(normalizedKeyword) ||
            (job.notes ?? "").toLowerCase().includes(normalizedKeyword)) &&
          (platformFilter === "all" || getJobPlatform(job) === platformFilter) &&
          (stageFilter === "all" || effectiveStage === stageFilter) &&
          (communicationStatusFilter === "all" ||
            job.communication_status === communicationStatusFilter) &&
          matchesAnalysis
        );
      }),
    [
      analyses,
      analysisFilter,
      communicationStatusFilter,
      focusFilter,
      highMatchScore,
      jobs,
      normalizedKeyword,
      platformFilter,
      stageFilter,
    ],
  );

  const handleExport = useCallback(
    async (format: "csv" | "markdown", scope: "selected" | "filtered" | "all") => {
      let targetIds: string[] | undefined;
      if (scope === "selected" && selectedJobIds.length > 0) {
        targetIds = selectedJobIds.map(String);
      } else if (scope === "filtered") {
        if (filteredJobs.length === 0) {
          messageApi.warning("当前筛选条件下没有可导出的岗位");
          return;
        }
        if (filteredJobs.length < jobs.length) {
          targetIds = filteredJobs.map((job) => job.id);
        }
      }
      const now = new Date();
      const stamp = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}`;
      const ext = format === "csv" ? "csv" : "md";
      const filePath = await save({
        defaultPath: `offerflow_jobs_${stamp}.${ext}`,
        filters: [
          {
            name: format === "csv" ? "CSV 表格 (*.csv)" : "Markdown 笔记 (*.md)",
            extensions: [ext],
          },
        ],
      });
      if (!filePath) return;
      setExporting(true);
      try {
        const result = await invoke<CommandResult<number>>("job_export", {
          path: filePath,
          format,
          jobIds: targetIds ?? null,
        });
        if (!result.success) {
          messageApi.error(commandErrorMessage(result.error, "导出失败"));
          return;
        }
        messageApi.success(`已成功导出 ${result.data ?? 0} 条岗位到 ${ext.toUpperCase()} 文件`);
      } catch (error: unknown) {
        messageApi.error(error instanceof Error ? error.message : "导出失败");
      } finally {
        setExporting(false);
      }
    },
    [filteredJobs, jobs.length, messageApi, selectedJobIds],
  );


  /// 批量分析在后端串行执行，这里只等一个总结果
  const handleBatchAnalyze = useCallback(async () => {
    if (!aiConfigured) {
      onConfigureAi();
      return;
    }
    setBatchAnalyzing(true);
    try {
      const result = await invoke<CommandResult<BatchAnalysisResult>>(
        "job_analyze_batch",
        { jobIds: selectedJobIds.map(String), skipAnalyzed: true },
      );
      if (!result.success || !result.data) {
        messageApi.error(commandErrorMessage(result.error, "批量分析失败"));
        return;
      }
      const { analyzed, skipped, failed, failures } = result.data;
      const summary = [
        `已分析 ${analyzed} 个`,
        skipped ? `跳过 ${skipped} 个已分析` : "",
        failed ? `失败 ${failed} 个` : "",
      ]
        .filter(Boolean)
        .join("，");
      if (failed > 0) {
        messageApi.warning(`${summary}。${failures.slice(0, 2).join("；")}`);
      } else {
        messageApi.success(summary);
      }
      setSelectedJobIds([]);
      void loadJobs();
      void loadAnalyses();
    } catch (error: unknown) {
      messageApi.error(error instanceof Error ? error.message : "批量分析失败");
    } finally {
      setBatchAnalyzing(false);
    }
  }, [aiConfigured, loadAnalyses, loadJobs, messageApi, onConfigureAi, selectedJobIds]);

  const defaultSyncPlatform: "boss" | "liepin" =
    platformFilter === "liepin" ? "liepin" : "boss";
  const isFilteredNarrowed = filteredJobs.length < jobs.length;

  /* ── table columns ── */
  const columns: ColumnsType<JobListItem> = [
    {
      title: "岗位名称",
      dataIndex: "title",
      key: "title",
      ellipsis: true,
      width: 210,
      render: (title: string, record: JobListItem) => (
        <Space size={6}>
          <span>{title}</span>
          {record.notes && <FileTextOutlined style={{ color: "#1677ff" }} title={`面试笔记：${record.notes}`} />}
        </Space>
      ),
    },
    {
      title: "公司",
      dataIndex: "company_name",
      key: "company_name",
      ellipsis: true,
      width: 150,
    },
    {
      title: "推进阶段",
      key: "pipeline_stage",
      width: 126,
      render: (_: unknown, record: JobListItem) => (
        <Select<JobPipelineStage>
          size="small"
          value={resolveEffectiveStage(record)}
          options={STAGE_OPTIONS}
          style={{ width: 112 }}
          onChange={(next) => void handleChangeStage(record, next)}
        />
      ),
    },
    {
      title: "匹配度",
      key: "match_score",
      width: 96,
      sorter: (a, b) =>
        (analyses[a.id]?.match_score ?? -1) - (analyses[b.id]?.match_score ?? -1),
      render: (_: unknown, record: JobListItem) => (
        <MatchScoreTag analysis={analyses[record.id]} />
      ),
    },
    {
      title: "沟通状态",
      dataIndex: "communication_status",
      key: "communication_status",
      width: 100,
      render: (status: CommunicationStatus) =>
        renderCommunicationStatus(status),
    },
    {
      title: "最新消息",
      key: "latest_message",
      width: 260,
      sorter: (a, b) => (a.latest_message_at ?? 0) - (b.latest_message_at ?? 0),
      render: (_: unknown, record: JobListItem) => renderLatestMessage(record),
    },
    {
      title: "平台",
      key: "platform",
      width: 84,
      render: (_: unknown, record: JobListItem) =>
        getJobPlatform(record) === "51job" ? <Tag color="orange">51job</Tag> : getJobPlatform(record) === "liepin" ? (
          <Tag color="purple">猎聘</Tag>
        ) : (
          <Tag color="green">BOSS</Tag>
        ),
    },
    {
      title: "薪资",
      dataIndex: "salary",
      key: "salary",
      width: 130,
      render: (text: string) => text || "-",
    },
    {
      title: "地点",
      dataIndex: "location",
      key: "location",
      width: 96,
      render: (text: string | null) => text || "-",
    },
    {
      title: "创建时间",
      dataIndex: "created_at",
      key: "created_at",
      width: 164,
      sorter: (a, b) => a.created_at.localeCompare(b.created_at),
      defaultSortOrder: "descend",
    },
    {
      title: "操作",
      key: "action",
      width: 196,
      fixed: "right",
      render: (_: unknown, record: JobListItem) => (
        <Space size={2}>
          <Button
            type="link"
            size="small"
            icon={<EyeOutlined />}
            onClick={() => setCurrentJob(record)}
          >
            详情
          </Button>
          <Button
            type="link"
            size="small"
            icon={<MessageOutlined />}
            onClick={() => setChatJob(record)}
          >
            沟通
          </Button>
          <Dropdown
            trigger={["click"]}
            menu={{
              items: [
                {
                  key: "source",
                  icon: <LinkOutlined />,
                  label: "打开原始 JD",
                  disabled: !record.source_url,
                  onClick: () =>
                    void invoke<CommandResult<void>>("job_open_source", { id: record.id })
                      .then((result) => {
                        if (!result.success) {
                          messageApi.error(commandErrorMessage(result.error, "打开原始JD失败"));
                        }
                      })
                      .catch((error) => messageApi.error(String(error))),
                },
                ...(onBlacklistCompany && record.company_name.trim()
                  ? [
                      {
                        key: "blacklist",
                        icon: <StopOutlined />,
                        label: `拉黑「${record.company_name}」`,
                        onClick: () => {
                          const result = onBlacklistCompany(record.company_name);
                          if (result === false) {
                            messageApi.warning(
                              "当前配置存在错误，暂无法自动保存公司黑名单，请先前往「求职设置」修正",
                            );
                          } else if (result && typeof result === "object" && !result.ok) {
                            if (result.warning) messageApi.warning(result.warning);
                          } else {
                            messageApi.success(`已将「${record.company_name}」加入公司黑名单`);
                          }
                        },
                      },
                    ]
                  : []),
                { type: "divider" as const },
                {
                  key: "delete",
                  danger: true,
                  icon: <DeleteOutlined />,
                  label: "删除该岗位",
                  onClick: () =>
                    Modal.confirm({
                      title: "确认删除",
                      content: `确定要删除「${record.title}」吗？此操作不可恢复。`,
                      okText: "确认删除",
                      cancelText: "取消",
                      okButtonProps: { danger: true },
                      onOk: () => handleDelete(record.id),
                    }),
                },
              ],
            }}
          >
            <Button type="text" size="small" icon={<EllipsisOutlined />} aria-label="更多操作" />
          </Dropdown>
        </Space>
      ),
    },
  ];

  return (
    <div
      className="job-data-page"
      style={{ display: "flex", flexDirection: "column", height: "100%", gap: 16 }}
    >
      {contextHolder}

      {/* ── header toolbar ── */}
      <div className="job-toolbar">
        <div className="job-toolbar-left">
          <Typography.Title level={5} style={{ margin: 0 }}>
            沟通与进度
          </Typography.Title>
          <Segmented<FocusFilter>
            size="small"
            value={focusFilter}
            onChange={setFocusFilter}
            options={[
              { label: `全部 (${jobs.length})`, value: "all" },
              { label: `待跟进 (${actionableCount})`, value: "actionable" },
              { label: `已投待回 (${waitingCount})`, value: "waiting" },
            ]}
          />
          <Segmented
            size="small"
            value={viewMode}
            onChange={(val) => setViewMode(val as "table" | "kanban")}
            options={[
              { label: "看板", value: "kanban" },
              { label: "表格", value: "table" },
            ]}
          />
          {viewMode === "table" && selectedJobIds.length > 0 && (
            <>
              <Button
                size="small"
                type="primary"
                icon={<ThunderboltOutlined />}
                loading={batchAnalyzing}
                onClick={() => void handleBatchAnalyze()}
              >
                批量分析（{selectedJobIds.length}）
              </Button>
              <Popconfirm
                title={`确定批量删除已选的 ${selectedJobIds.length} 条岗位？`}
                description="删除后无法恢复，但不会影响已导出的备份。"
                okText="确认删除"
                cancelText="取消"
                okButtonProps={{ danger: true }}
                onConfirm={() => void handleBatchDelete()}
              >
                <Button size="small" danger icon={<DeleteOutlined />} loading={batchDeleting}>
                  批量删除
                </Button>
              </Popconfirm>
            </>
          )}
        </div>
        <div className="job-toolbar-right">
          <Select
            size="small"
            value={platformFilter}
            style={{ width: 96 }}
            onChange={setPlatformFilter}
            options={[
              { label: "全部平台", value: "all" },
              { label: "BOSS", value: "boss" },
              { label: "猎聘", value: "liepin" },
              { label: "51job", value: "51job" },
            ]}
          />
          <Select
            size="small"
            value={stageFilter}
            style={{ width: 106 }}
            onChange={setStageFilter}
            options={[
              { label: "全部阶段", value: "all" },
              ...STAGE_OPTIONS,
            ]}
          />
          <Select
            size="small"
            value={communicationStatusFilter}
            style={{ width: 108 }}
            onChange={setCommunicationStatusFilter}
            options={[
              { label: "全部沟通", value: "all" },
              { label: "收到约面", value: "interview_invited" },
              { label: "已回复", value: "replied" },
              { label: "未回复", value: "no_reply" },
              { label: "明确拒绝", value: "rejected" },
            ]}
          />
          <Select
            size="small"
            value={analysisFilter}
            style={{ width: 122 }}
            onChange={setAnalysisFilter}
            options={[
              { label: "全部分析", value: "all" },
              { label: "已分析", value: "analyzed" },
              { label: "未分析", value: "not_analyzed" },
              { label: `高匹配≥${highMatchScore}`, value: "high_match" },
            ]}
          />
          <Input
            size="small"
            placeholder="搜索岗位/公司/笔记"
            prefix={<SearchOutlined />}
            allowClear
            style={{ width: 185 }}
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
          />
          <Dropdown.Button
            size="small"
            type="primary"
            loading={syncing}
            icon={<DownOutlined />}
            onClick={() => void handleSyncStatus(defaultSyncPlatform)}
            menu={{
              items: [
                {
                  key: "sync-boss",
                  label: "零点击同步 BOSS 状态（推荐 · 不消红点）",
                  onClick: () => void handleSyncStatus("boss"),
                },
                {
                  key: "sync-liepin",
                  label: "零点击同步猎聘状态（不消红点）",
                  onClick: () => void handleSyncStatus("liepin"),
                },
                {
                  key: "sync-all",
                  label: "同时同步 BOSS + 猎聘",
                  onClick: () => void handleSyncStatus(undefined),
                },
              ],
            }}
          >
            <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
              <IconRefreshSync size={13} spinning={syncing} />
              <span>
                {syncing
                  ? "同步中…"
                  : defaultSyncPlatform === "liepin"
                    ? "同步猎聘状态"
                    : "同步状态"}
              </span>
            </span>
          </Dropdown.Button>
          {(() => {
            const latestSync = syncMetaList
              .map((m) => m.last_synced_at)
              .filter((v): v is string => Boolean(v))
              .sort()
              .pop();
            return latestSync ? (
              <span className="job-sync-meta" title={`上次零点击同步：${latestSync}`}>
                上次同步 {latestSync.slice(11, 16)}
              </span>
            ) : null;
          })()}
          <Dropdown
            trigger={["click"]}
            menu={{
              items:
                selectedJobIds.length > 0
                  ? [
                      {
                        key: "csv-selected",
                        label: `导出已选 ${selectedJobIds.length} 条为 CSV (Excel)`,
                        onClick: () => void handleExport("csv", "selected"),
                      },
                      {
                        key: "md-selected",
                        label: `导出已选 ${selectedJobIds.length} 条为 Markdown (Obsidian/Notion)`,
                        onClick: () => void handleExport("markdown", "selected"),
                      },
                      { type: "divider" as const },
                      {
                        key: "csv-all",
                        label: "导出全部为 CSV (Excel 兼容)",
                        onClick: () => void handleExport("csv", "all"),
                      },
                      {
                        key: "md-all",
                        label: "导出全部为 Markdown (Obsidian/Notion)",
                        onClick: () => void handleExport("markdown", "all"),
                      },
                    ]
                  : isFilteredNarrowed
                    ? [
                        {
                          key: "csv-filtered",
                          label: `导出当前筛选 ${filteredJobs.length} 条为 CSV (Excel)`,
                          onClick: () => void handleExport("csv", "filtered"),
                        },
                        {
                          key: "md-filtered",
                          label: `导出当前筛选 ${filteredJobs.length} 条为 Markdown (Obsidian/Notion)`,
                          onClick: () => void handleExport("markdown", "filtered"),
                        },
                        { type: "divider" as const },
                        {
                          key: "csv-all",
                          label: "导出全部为 CSV (Excel 兼容)",
                          onClick: () => void handleExport("csv", "all"),
                        },
                        {
                          key: "md-all",
                          label: "导出全部为 Markdown (Obsidian/Notion)",
                          onClick: () => void handleExport("markdown", "all"),
                        },
                      ]
                    : [
                        {
                          key: "csv-all",
                          label: "导出全部为 CSV (Excel 兼容)",
                          onClick: () => void handleExport("csv", "filtered"),
                        },
                        {
                          key: "md-all",
                          label: "导出全部为 Markdown (Obsidian/Notion)",
                          onClick: () => void handleExport("markdown", "filtered"),
                        },
                      ],
            }}
          >
            <Button size="small" icon={<DownloadOutlined />} loading={exporting}>
              导出
            </Button>
          </Dropdown>
        </div>
      </div>

      {/* ── view body ── */}
      {viewMode === "kanban" ? (
        <KanbanView
          jobs={filteredJobs}
          analyses={analyses}
          onView={setCurrentJob}
          onChat={setChatJob}
          onDelete={(id) => void handleDelete(id)}
          onChangeStage={(job, nextStage) => void handleChangeStage(job, nextStage)}
        />
      ) : (
        <div
          style={{
            flex: "1 1 0",
            minHeight: 0,
            display: "flex",
            flexDirection: "column",
            overflow: "hidden",
          }}
        >
          <Table<JobListItem>
            className="job-data-table"
            rowKey="id"
            columns={columns}
            dataSource={filteredJobs}
            loading={loading || batchAnalyzing || batchDeleting}
            rowSelection={{
              selectedRowKeys: selectedJobIds,
              onChange: setSelectedJobIds,
              preserveSelectedRowKeys: true,
            }}
            size="middle"
            scroll={{ x: 1480, y: "calc(100vh - 290px)" }}
            pagination={{
              defaultPageSize: 15,
              showSizeChanger: true,
              showTotal: (t) => `共 ${t} 条`,
            }}
          />
        </div>
      )}

      {/* ── job detail modal ── */}
      {currentJob && (
        <Modal
          title={`${currentJob.company_name} · ${currentJob.title}`}
          open
          onCancel={() => setCurrentJob(null)}
          footer={null}
          width="min(1180px, 94vw)"
          centered
          destroyOnHidden
          styles={{ body: { height: "min(74vh, 860px)", overflowY: "auto", padding: "8px 24px 16px" } }}
        >
          {/* 岗位原文在前、AI 分析在后：先看清岗位本身，再看对它的判断 */}
          <Tabs
            defaultActiveKey="brief"
            items={[
              {
                key: "brief",
                label: "岗位详情",
                children: (
                  <JobBrief
                    job={currentJob}
                    analysis={analyses[currentJob.id]}
                    onDeliveryResolved={updated => { setCurrentJob(updated); void loadJobs(); }}
                    onJobUpdated={updated => { setCurrentJob(updated); void loadJobs(false); }}
                    onBlacklistCompany={onBlacklistCompany}
                  />
                ),
              },
              {
                key: "analysis",
                label: "面试分析报告",
                children: (
                  <AnalysisReport
                    job={currentJob}
                    aiConfigured={aiConfigured}
                    llmConfigured={llmConfigured}
                    onConfigureAi={onConfigureAi}
                    onAnalyzed={(analysis) =>
                      setAnalyses((current) => ({
                        ...current,
                        [analysis.job_id]: analysis,
                      }))
                    }
                  />
                ),
              },
            ]}
          />
        </Modal>
      )}

      {/* ── chat modal ── */}
      {chatJob && (
        <ChatThreadModal job={chatJob} open onClose={() => setChatJob(null)} />
      )}
    </div>
  );
};

export default JobDataPage;
