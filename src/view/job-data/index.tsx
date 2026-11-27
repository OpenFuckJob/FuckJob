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
  DownloadOutlined,
  EllipsisOutlined,
  EyeOutlined,
  FileTextOutlined,
  InboxOutlined,
  LinkOutlined,
  MessageOutlined,
  RobotOutlined,
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
} from "../../types/job-detail";
import type { InterviewJobAnalysis } from "../../types/analysis";
import { DEFAULT_HIGH_MATCH_SCORE } from "../../types/app-config";
import AnalysisReport from "./AnalysisReport";
import ChatThreadModal from "./ChatThread";
import JobBrief from "./JobBrief";
import { useJobDataRefresh } from "../../hooks/useJobDataRefresh";
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
  if (!job.latest_message?.trim() || !job.latest_message_at) {
    return "-";
  }

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
      <Tag color={job.latest_message_received ? "green" : "blue"} style={{ margin: 0 }}>
        {job.latest_message_received ? "招聘方" : "我"}
      </Tag>
      <Typography.Text type="secondary" style={{ flex: "0 0 auto", fontSize: 12 }}>
        {formatLatestMessageTime(job.latest_message_at)}
      </Typography.Text>
      <Typography.Text ellipsis title={job.latest_message} style={{ minWidth: 0, flex: 1 }}>
        {job.latest_message}
      </Typography.Text>
    </div>
  );
};
/* ────────── CRM stage & Kanban lane config ────────── */

const STAGE_META: Record<JobPipelineStage, { label: string; color: string }> = {
  pending: { label: "待投递", color: "default" },
  applied: { label: "已投递", color: "blue" },
  replied: { label: "已回复", color: "green" },
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
  if (job.pipeline_stage) return job.pipeline_stage;
  if (job.communication_status === "rejected") return "rejected";
  if (job.communication_status === "replied" || job.is_reply) return "replied";
  if (job.is_send_resume) return "applied";
  return "pending";
}

interface KanbanLane {
  key: string;
  label: string;
  color: string;
  hint: string;
  stages: JobPipelineStage[];
}

/** 6 列交互式求职 CRM 泳道，按推进漏斗顺序互斥分布 */
const KANBAN_LANES: KanbanLane[] = [
  {
    key: "pending",
    label: "待投递",
    color: "#94a3b8",
    hint: "已建联但还没投出简历",
    stages: ["pending"],
  },
  {
    key: "applied",
    label: "已投递",
    color: "#1677ff",
    hint: "已投简历，等待 HR 回应",
    stages: ["applied"],
  },
  {
    key: "replied",
    label: "已回复",
    color: "#10b981",
    hint: "对方已回应，优先推进约面",
    stages: ["replied"],
  },
  {
    key: "interview",
    label: "约面与面试",
    color: "#8b5cf6",
    hint: "已约面或面试流程进行中",
    stages: ["interview_scheduled", "interviewing"],
  },
  {
    key: "offer",
    label: "Offer",
    color: "#eab308",
    hint: "已拿到 Offer 或谈薪阶段",
    stages: ["offer"],
  },
  {
    key: "closed",
    label: "已婉拒 / 归档",
    color: "#f97316",
    hint: "明确不合适或主动归档",
    stages: ["rejected", "archived"],
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
  onView,
  onChat,
  onDelete,
  onChangeStage,
  onStartInterview,
}: {
  job: JobListItem;
  analysis?: InterviewJobAnalysis;
  onView: (job: JobDetail) => void;
  onChat: (job: JobDetail) => void;
  onDelete: (id: string) => void;
  onChangeStage: (job: JobListItem, stage: JobPipelineStage) => void;
  onStartInterview?: (job: JobDetail) => void;
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
      className="job-card"
      role="button"
      tabIndex={0}
      title="查看岗位详情、面试笔记与分析报告"
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
        <MatchScoreTag analysis={analysis} />
      </div>

      <div className="job-card-company">
        <span
          className="platform-dot"
          style={{
            background:
              platform === "51job"
                ? "#ff6000"
                : platform === "liepin"
                  ? "#722ed1"
                  : "#52c41a",
          }}
          title={platform === "51job" ? "前程无忧" : platform === "liepin" ? "猎聘" : "BOSS 直聘"}
        />
        <span>{job.company_name}</span>
      </div>

      <div className="job-card-tags">
        {job.salary && <span className="job-card-chip salary">{job.salary}</span>}
        {job.location && <span className="job-card-chip">{job.location}</span>}
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

      <div
        onClick={(event) => event.stopPropagation()}
        onKeyDown={(event) => event.stopPropagation()}
      >
        <Select<JobPipelineStage>
          size="small"
          value={stage}
          options={STAGE_OPTIONS}
          style={{ width: "100%" }}
          onChange={(next) => onChangeStage(job, next)}
        />
      </div>

      <div className="job-card-foot">
        <span className="job-card-time">{time}</span>
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
          {onStartInterview && (
            <Button
              size="small"
              type="text"
              icon={<RobotOutlined />}
              title="用这个岗位做模拟面试"
              onClick={() => onStartInterview(job)}
            />
          )}
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
  onStartInterview,
}: {
  jobs: JobListItem[];
  analyses: Record<string, InterviewJobAnalysis>;
  onView: (job: JobDetail) => void;
  onChat: (job: JobDetail) => void;
  onDelete: (id: string) => void;
  onChangeStage: (job: JobListItem, stage: JobPipelineStage) => void;
  onStartInterview?: (job: JobDetail) => void;
}) {
  const lanes = useMemo(() => groupByLane(jobs), [jobs]);

  return (
    <div className="kanban-board">
      {lanes.map((lane) => (
        <div className="kanban-lane" key={lane.key}>
          <div className="kanban-lane-head" title={lane.hint}>
            <span className="kanban-lane-dot" style={{ background: lane.color }} />
            <span className="kanban-lane-title">{lane.label}</span>
            <span className="kanban-lane-count">{lane.jobs.length}</span>
          </div>
          <div className="kanban-lane-body">
            {lane.jobs.length === 0 ? (
              <div className="kanban-empty">
                <InboxOutlined />
                <span>{lane.hint}</span>
              </div>
            ) : (
              lane.jobs.map((job) => (
                <JobKanbanCard
                  key={job.id}
                  job={job}
                  analysis={analyses[job.id]}
                  onView={onView}
                  onChat={onChat}
                  onDelete={onDelete}
                  onChangeStage={onChangeStage}
                  onStartInterview={onStartInterview}
                />
              ))
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

/* ────────── Page ────────── */

const JobDataPage = ({ aiConfigured, llmConfigured, onConfigureAi, focusJobId, onFocusHandled, highMatchScore = DEFAULT_HIGH_MATCH_SCORE, onStartInterview, onBlacklistCompany }: {
  aiConfigured: boolean;
  llmConfigured: boolean;
  onConfigureAi: () => void;
  /** 从其他页面跳转过来时需要直接打开沟通记录的岗位 */
  focusJobId?: string;
  onFocusHandled?: () => void;
  /** 高匹配的判定分数线，与求职方案的岗位分析配置一致 */
  highMatchScore?: number;
  /** 带着岗位信息跳到模拟面试 */
  onStartInterview?: (job: JobDetail) => void;
  /** 一键将公司名追加到当前求职方案的排除公司列表 */
  onBlacklistCompany?: (companyName: string) => void;
}) => {
  const [jobs, setJobs] = useState<JobListItem[]>([]);
  // 初始即为加载中，避免首次挂载时把待跳转的岗位当成「不存在」
  const [loading, setLoading] = useState(true);
  const [keyword, setKeyword] = useState("");
  const [analyses, setAnalyses] = useState<Record<string, InterviewJobAnalysis>>({});
  const [platformFilter, setPlatformFilter] = useState<JobPlatform | "all">("all");
  const [stageFilter, setStageFilter] = useState<JobPipelineStage | "all">("all");
  const [communicationStatusFilter, setCommunicationStatusFilter] =
    useState<CommunicationStatus | "all">("all");
  const [analysisFilter, setAnalysisFilter] = useState<AnalysisFilter>("all");
  const [currentJob, setCurrentJob] = useState<JobDetail | null>(null);
  const [chatJob, setChatJob] = useState<JobDetail | null>(null);
  const [viewMode, setViewMode] = useState<"table" | "kanban">("table");
  const [selectedJobIds, setSelectedJobIds] = useState<React.Key[]>([]);
  const [batchAnalyzing, setBatchAnalyzing] = useState(false);
  const [batchDeleting, setBatchDeleting] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [messageApi, contextHolder] = message.useMessage();
  const jobsRequest = useRef(0);
  const analysesRequest = useRef(0);

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
    await Promise.all([loadJobs(false), loadAnalyses()]);
  }, [loadAnalyses, loadJobs]));

  useEffect(() => {
    void loadJobs();
    void loadAnalyses();
  }, [loadAnalyses, loadJobs]);

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
            item.id === job.id ? { ...item, pipeline_stage: nextStage } : item,
          ),
        );
        messageApi.success(`已更新为「${STAGE_META[nextStage].label}」`);
      } catch (error: unknown) {
        messageApi.error(error instanceof Error ? error.message : "更新推进阶段失败");
      }
    },
    [messageApi],
  );

  const handleExport = useCallback(
    async (format: "csv" | "markdown", scope: "selected" | "filtered") => {
      const targetIds =
        scope === "selected" && selectedJobIds.length > 0
          ? selectedJobIds.map(String)
          : undefined;
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
    [messageApi, selectedJobIds],
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

  const normalizedKeyword = keyword.trim().toLowerCase();
  const matchesAnalysisFilter = (job: JobListItem) => {
    const analysis = analyses[job.id];
    switch (analysisFilter) {
      case "analyzed":
        return !!analysis;
      case "not_analyzed":
        return !analysis;
      case "high_match":
        return !!analysis && !analysis.parse_error && analysis.match_score >= highMatchScore;
      default:
        return true;
    }
  };
  const filteredJobs = jobs.filter(
    (job) =>
      (!normalizedKeyword ||
        job.title.toLowerCase().includes(normalizedKeyword) ||
        job.company_name.toLowerCase().includes(normalizedKeyword) ||
        (job.notes ?? "").toLowerCase().includes(normalizedKeyword)) &&
      (platformFilter === "all" || getJobPlatform(job) === platformFilter) &&
      (stageFilter === "all" || resolveEffectiveStage(job) === stageFilter) &&
      (communicationStatusFilter === "all" ||
        job.communication_status === communicationStatusFilter) &&
      matchesAnalysisFilter(job),
  );

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
                ...(onStartInterview
                  ? [
                      {
                        key: "interview",
                        icon: <RobotOutlined />,
                        label: "用此岗位做模拟面试",
                        onClick: () => onStartInterview(record),
                      },
                    ]
                  : []),
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
                          onBlacklistCompany(record.company_name);
                          messageApi.success(`已将「${record.company_name}」加入公司黑名单`);
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
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          gap: 12,
          flexWrap: "wrap",
        }}
      >
        <Typography.Title level={5} style={{ margin: 0 }}>
          岗位数据
        </Typography.Title>
        <Space wrap>
          {viewMode === "table" && selectedJobIds.length > 0 && (
            <>
              <Button
                type="primary"
                icon={<ThunderboltOutlined />}
                loading={batchAnalyzing}
                onClick={() => void handleBatchAnalyze()}
              >
                批量 AI 分析（{selectedJobIds.length}）
              </Button>
              <Popconfirm
                title={`确定批量删除已选的 ${selectedJobIds.length} 条岗位？`}
                description="删除后无法恢复，但不会影响已导出的备份。"
                okText="确认删除"
                cancelText="取消"
                okButtonProps={{ danger: true }}
                onConfirm={() => void handleBatchDelete()}
              >
                <Button danger icon={<DeleteOutlined />} loading={batchDeleting}>
                  批量删除
                </Button>
              </Popconfirm>
            </>
          )}
          <Dropdown
            trigger={["click"]}
            menu={{
              items: [
                {
                  key: "csv-all",
                  label:
                    selectedJobIds.length > 0
                      ? `导出已选 ${selectedJobIds.length} 条为 CSV (Excel)`
                      : "导出全部为 CSV (Excel 兼容)",
                  onClick: () =>
                    void handleExport("csv", selectedJobIds.length > 0 ? "selected" : "filtered"),
                },
                {
                  key: "md-all",
                  label:
                    selectedJobIds.length > 0
                      ? `导出已选 ${selectedJobIds.length} 条为 Markdown (Obsidian/Notion)`
                      : "导出全部为 Markdown (Obsidian/Notion)",
                  onClick: () =>
                    void handleExport(
                      "markdown",
                      selectedJobIds.length > 0 ? "selected" : "filtered",
                    ),
                },
              ],
            }}
          >
            <Button icon={<DownloadOutlined />} loading={exporting}>
              导出
            </Button>
          </Dropdown>
          <Segmented
            value={viewMode}
            onChange={(val) => setViewMode(val as "table" | "kanban")}
            options={[
              { label: "表格", value: "table" },
              { label: "看板", value: "kanban" },
            ]}
          />
          <Select
            value={platformFilter}
            style={{ width: 112 }}
            onChange={setPlatformFilter}
            options={[
              { label: "全部平台", value: "all" },
              { label: "BOSS", value: "boss" },
              { label: "猎聘", value: "liepin" },
              { label: "前程无忧", value: "51job" },
            ]}
          />
          <Select
            value={stageFilter}
            style={{ width: 120 }}
            onChange={setStageFilter}
            options={[
              { label: "全部阶段", value: "all" },
              ...STAGE_OPTIONS,
            ]}
          />
          <Select
            value={communicationStatusFilter}
            style={{ width: 124 }}
            onChange={setCommunicationStatusFilter}
            options={[
              { label: "全部沟通", value: "all" },
              { label: "明确拒绝", value: "rejected" },
              { label: "未回复", value: "no_reply" },
              { label: "已回复", value: "replied" },
            ]}
          />
          <Select
            value={analysisFilter}
            style={{ width: 140 }}
            onChange={setAnalysisFilter}
            options={[
              { label: "全部分析", value: "all" },
              { label: "已分析", value: "analyzed" },
              { label: "未分析", value: "not_analyzed" },
              { label: `高匹配（≥${highMatchScore}）`, value: "high_match" },
            ]}
          />
          <Input
            placeholder="搜索岗位、公司或面试笔记"
            prefix={<SearchOutlined />}
            allowClear
            style={{ width: 220 }}
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
          />
        </Space>
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
          onStartInterview={onStartInterview}
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
