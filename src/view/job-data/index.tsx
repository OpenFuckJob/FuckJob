import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Button,
  Input,
  Modal,
  Popconfirm,
  Segmented,
  Select,
  Space,
  Table,
  Tag,
  Typography,
  message,
} from "antd";
import {
  DeleteOutlined,
  EyeOutlined,
  InboxOutlined,
  MessageOutlined,
  SearchOutlined,
  ThunderboltOutlined,
} from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import type { ColumnsType } from "antd/es/table";
import type { CommandResult } from "../../types/command";
import { commandErrorMessage } from "../../types/command";
import type {
  ChatMessageRecord,
  CommunicationStatus,
  JobDetail,
  JobListItem,
} from "../../types/job-detail";
import type { InterviewJobAnalysis } from "../../types/analysis";
import { DEFAULT_HIGH_MATCH_SCORE } from "../../types/app-config";
import AnalysisReport from "./AnalysisReport";
import "./style.css";

/** 与 Rust 侧 BatchAnalysisResult 对应 */
interface BatchAnalysisResult {
  analyzed: number;
  skipped: number;
  failed: number;
  failures: string[];
}

const getJobPlatform = (job: JobDetail): "boss" | "liepin" =>
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

/* ────────── Chat messages modal ────────── */

const ChatMessagesModal = ({
  job,
  open,
  onClose,
}: {
  job: JobDetail;
  open: boolean;
  onClose: () => void;
}) => {
  const [messages, setMessages] = useState<ChatMessageRecord[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open) return;
    setLoading(true);
    invoke<CommandResult<ChatMessageRecord[]>>("chat_messages_by_job", {
      jobId: job.id,
    })
      .then((result) => {
        if (result.success && result.data) {
          setMessages(
            [...result.data].sort((a, b) => a.time - b.time),
          );
        } else {
          setMessages([]);
        }
      })
      .catch(() => setMessages([]))
      .finally(() => setLoading(false));
  }, [job.id, open]);

  return (
    <Modal
      title={`${job.title} - 沟通记录`}
      open={open}
      onCancel={onClose}
      footer={null}
      width={560}
      styles={{ body: { maxHeight: 480, overflowY: "auto", padding: "16px 24px" } }}
    >
      {loading ? (
        <div style={{ textAlign: "center", padding: 24, color: "#999" }}>
          加载中...
        </div>
      ) : messages.length === 0 ? (
        <div style={{ textAlign: "center", padding: 24, color: "#999" }}>
          暂无沟通记录
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          {messages.map((msg) => {
            const isMine = !msg.received;
            const time = new Date(msg.time).toLocaleString("zh-CN", {
              month: "2-digit",
              day: "2-digit",
              hour: "2-digit",
              minute: "2-digit",
            });
            return (
              <div
                key={msg.id}
                style={{
                  display: "flex",
                  flexDirection: isMine ? "row-reverse" : "row",
                  alignItems: "flex-start",
                  gap: 8,
                }}
              >
                <div
                  style={{
                    maxWidth: "75%",
                    display: "flex",
                    flexDirection: "column",
                    alignItems: isMine ? "flex-end" : "flex-start",
                  }}
                >
                  <div
                    style={{
                      padding: "8px 12px",
                      borderRadius: 12,
                      backgroundColor: isMine ? "#1677ff" : "#f0f0f0",
                      color: isMine ? "#fff" : "#333",
                      fontSize: 13,
                      lineHeight: 1.5,
                      wordBreak: "break-word",
                      ...(isMine
                        ? { borderBottomRightRadius: 4 }
                        : { borderBottomLeftRadius: 4 }),
                    }}
                  >
                    {msg.text}
                  </div>
                  <span
                    style={{
                      fontSize: 11,
                      color: "#999",
                      marginTop: 2,
                    }}
                  >
                    {msg.from_name} · {time}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </Modal>
  );
};

/* ────────── Kanban lane config ────────── */

interface KanbanLane {
  key: string;
  label: string;
  color: string;
  hint: string;
  filter: (job: JobListItem) => boolean;
}

/// 泳道按求职推进顺序排列，且互斥：一个岗位只会落在其中一列
const KANBAN_LANES: KanbanLane[] = [
  {
    key: "not_sent",
    label: "待投递",
    color: "#94a3b8",
    hint: "还没投出简历的岗位",
    filter: (job) => !job.is_send_resume && !job.is_reply,
  },
  {
    key: "sent",
    label: "已投递",
    color: "#1677ff",
    hint: "等待招聘方回应",
    filter: (job) => job.is_send_resume && job.communication_status !== "replied" && job.communication_status !== "rejected",
  },
  {
    key: "replied",
    label: "已回复",
    color: "#10b981",
    hint: "招聘方已回应，优先跟进",
    filter: (job) => job.communication_status === "replied",
  },
  {
    key: "rejected",
    label: "已婉拒",
    color: "#f97316",
    hint: "对方明确表示不合适",
    filter: (job) => job.communication_status === "rejected",
  },
];

/** 没被前面的泳道接住的岗位归到「待投递」，避免看板漏掉数据 */
function groupByLane(jobs: JobListItem[]): Array<KanbanLane & { jobs: JobListItem[] }> {
  const lanes = KANBAN_LANES.map((lane) => ({ ...lane, jobs: [] as JobListItem[] }));
  for (const job of jobs) {
    const lane = lanes.find((candidate) => candidate.filter(job)) ?? lanes[0];
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
}: {
  job: JobListItem;
  analysis?: InterviewJobAnalysis;
  onView: (job: JobDetail) => void;
  onChat: (job: JobDetail) => void;
  onDelete: (id: string) => void;
}) {
  const platform = getJobPlatform(job);
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
      title="查看面试分析报告"
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
          style={{ background: platform === "liepin" ? "#722ed1" : "#52c41a" }}
          title={platform === "liepin" ? "猎聘" : "BOSS 直聘"}
        />
        <span>{job.company_name}</span>
      </div>

      <div className="job-card-tags">
        {job.salary && <span className="job-card-chip salary">{job.salary}</span>}
        {job.location && <span className="job-card-chip">{job.location}</span>}
        {job.is_send_resume && <span className="job-card-chip">已投简历</span>}
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
}: {
  jobs: JobListItem[];
  analyses: Record<string, InterviewJobAnalysis>;
  onView: (job: JobDetail) => void;
  onChat: (job: JobDetail) => void;
  onDelete: (id: string) => void;
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

const JobDataPage = ({ aiConfigured, onConfigureAi, focusJobId, onFocusHandled, highMatchScore = DEFAULT_HIGH_MATCH_SCORE }: {
  aiConfigured: boolean;
  onConfigureAi: () => void;
  /** 从其他页面跳转过来时需要直接打开沟通记录的岗位 */
  focusJobId?: string;
  onFocusHandled?: () => void;
  /** 高匹配的判定分数线，与求职方案的岗位分析配置一致 */
  highMatchScore?: number;
}) => {
  const [jobs, setJobs] = useState<JobListItem[]>([]);
  // 初始即为加载中，避免首次挂载时把待跳转的岗位当成「不存在」
  const [loading, setLoading] = useState(true);
  const [keyword, setKeyword] = useState("");
  const [analyses, setAnalyses] = useState<Record<string, InterviewJobAnalysis>>({});
  const [communicationStatusFilter, setCommunicationStatusFilter] =
    useState<CommunicationStatus | "all">("all");
  const [analysisFilter, setAnalysisFilter] = useState<AnalysisFilter>("all");
  const [currentJob, setCurrentJob] = useState<JobDetail | null>(null);
  const [chatJob, setChatJob] = useState<JobDetail | null>(null);
  const [viewMode, setViewMode] = useState<"table" | "kanban">("table");
  const [selectedJobIds, setSelectedJobIds] = useState<React.Key[]>([]);
  const [batchAnalyzing, setBatchAnalyzing] = useState(false);
  const [messageApi, contextHolder] = message.useMessage();

  const loadJobs = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<CommandResult<JobListItem[]>>(
        "job_list_with_status",
      );
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
    } catch (error: unknown) {
      messageApi.error(
        error instanceof Error ? error.message : "加载岗位数据失败",
      );
    } finally {
      setLoading(false);
    }
  }, [messageApi]);

  /// 分析结果只是列表上的附加信息，取不到不该影响岗位管理本身
  const loadAnalyses = useCallback(async () => {
    try {
      const result = await invoke<CommandResult<InterviewJobAnalysis[]>>("analysis_list");
      if (!result.success || !result.data) return;
      setAnalyses(
        Object.fromEntries(result.data.map((item) => [item.job_id, item])),
      );
    } catch {
      setAnalyses({});
    }
  }, []);

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
        job.company_name.toLowerCase().includes(normalizedKeyword)) &&
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
      width: 220,
    },
    {
      title: "公司",
      dataIndex: "company_name",
      key: "company_name",
      ellipsis: true,
      width: 160,
    },
    {
      title: "匹配度",
      key: "match_score",
      width: 100,
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
      width: 110,
      render: (status: CommunicationStatus) =>
        renderCommunicationStatus(status),
    },
    {
      title: "平台",
      key: "platform",
      width: 90,
      render: (_: unknown, record: JobListItem) =>
        getJobPlatform(record) === "liepin" ? (
          <Tag color="purple">猎聘</Tag>
        ) : (
          <Tag color="green">BOSS</Tag>
        ),
    },
    {
      title: "薪资",
      dataIndex: "salary",
      key: "salary",
      width: 150,
      render: (text: string) => text || "-",
    },
    {
      title: "地点",
      dataIndex: "location",
      key: "location",
      width: 100,
      render: (text: string | null) => text || "-",
    },
    {
      title: "是否投递简历",
      key: "is_send_resume",
      width: 130,
      render: (_: unknown, record: JobListItem) =>
        record.is_send_resume ? (
          <Tag color="blue">已投递</Tag>
        ) : (
          <Tag color="default">未投递</Tag>
        ),
    },
    {
      title: "创建时间",
      dataIndex: "created_at",
      key: "created_at",
      width: 180,
      sorter: (a, b) => a.created_at.localeCompare(b.created_at),
      defaultSortOrder: "descend",
    },
    {
      title: "操作",
      key: "action",
      width: 270,
      fixed: "right",
      render: (_: unknown, record: JobListItem) => (
        <Space size={4}>
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
            沟通记录
          </Button>
          <Popconfirm
            title="确认删除"
            description={`确定要删除「${record.title}」吗？此操作不可恢复。`}
            onConfirm={() => void handleDelete(record.id)}
            okText="确认删除"
            cancelText="取消"
            okButtonProps={{ danger: true }}
          >
            <Button type="link" size="small" danger icon={<DeleteOutlined />}>
              删除
            </Button>
          </Popconfirm>
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
            <Button
              type="primary"
              icon={<ThunderboltOutlined />}
              loading={batchAnalyzing}
              onClick={() => void handleBatchAnalyze()}
            >
              批量 AI 分析（{selectedJobIds.length}）
            </Button>
          )}
          <Segmented
            value={viewMode}
            onChange={(val) => setViewMode(val as "table" | "kanban")}
            options={[
              { label: "表格", value: "table" },
              { label: "看板", value: "kanban" },
            ]}
          />
          <Select
            value={communicationStatusFilter}
            style={{ width: 140 }}
            onChange={setCommunicationStatusFilter}
            options={[
              { label: "全部沟通状态", value: "all" },
              { label: "明确拒绝", value: "rejected" },
              { label: "未回复", value: "no_reply" },
              { label: "已回复", value: "replied" },
            ]}
          />
          <Select
            value={analysisFilter}
            style={{ width: 150 }}
            onChange={setAnalysisFilter}
            options={[
              { label: "全部分析状态", value: "all" },
              { label: "已分析", value: "analyzed" },
              { label: "未分析", value: "not_analyzed" },
              { label: `高匹配（≥${highMatchScore}）`, value: "high_match" },
            ]}
          />
          <Input
            placeholder="搜索岗位或公司"
            prefix={<SearchOutlined />}
            allowClear
            style={{ width: 260 }}
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
            loading={loading || batchAnalyzing}
            rowSelection={{
              selectedRowKeys: selectedJobIds,
              onChange: setSelectedJobIds,
              preserveSelectedRowKeys: true,
            }}
            size="middle"
            scroll={{ x: 1300, y: "calc(100vh - 290px)" }}
            pagination={{
              defaultPageSize: 15,
              showSizeChanger: true,
              showTotal: (t) => `共 ${t} 条`,
            }}
          />
        </div>
      )}

      {/* ── analysis modal ── */}
      {currentJob && (
        <Modal
          title={`${currentJob.title} - 面试分析报告`}
          open
          onCancel={() => setCurrentJob(null)}
          footer={null}
          width="min(1180px, 94vw)"
          centered
          destroyOnHidden
          styles={{ body: { height: "min(74vh, 860px)", overflowY: "auto", padding: "16px 24px" } }}
        >
          <AnalysisReport
            job={currentJob}
            aiConfigured={aiConfigured}
            onConfigureAi={onConfigureAi}
            onAnalyzed={(analysis) =>
              setAnalyses((current) => ({ ...current, [analysis.job_id]: analysis }))
            }
          />
        </Modal>
      )}

      {/* ── chat modal ── */}
      {chatJob && (
        <ChatMessagesModal
          job={chatJob}
          open
          onClose={() => setChatJob(null)}
        />
      )}
    </div>
  );
};

export default JobDataPage;
