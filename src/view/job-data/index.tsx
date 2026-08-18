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

/** 与 Rust 侧 BatchAnalysisResult 对应 */
interface BatchAnalysisResult {
  analyzed: number;
  skipped: number;
  failed: number;
  failures: string[];
}
import AnalysisReport from "./AnalysisReport";

const getJobPlatform = (job: JobDetail): "boss" | "liepin" =>
  job.platform === "liepin" || job.id.startsWith("liepin:")
    ? "liepin"
    : "boss";

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
  bg: string;
  border: string;
  filter: (job: JobDetail) => boolean;
}

const KANBAN_LANES: KanbanLane[] = [
  {
    key: "not_sent",
    label: "未投递",
    color: "#64748b",
    bg: "#f8fafc",
    border: "#e2e8f0",
    filter: (j) => !j.is_send_resume,
  },
  {
    key: "sent",
    label: "已投递",
    color: "#1677ff",
    bg: "rgba(22,119,255,0.04)",
    border: "rgba(22,119,255,0.25)",
    filter: (j) => j.is_send_resume && !j.is_reply,
  },
  {
    key: "replied",
    label: "已回复",
    color: "#10b981",
    bg: "rgba(16,185,129,0.04)",
    border: "rgba(16,185,129,0.25)",
    filter: (j) => j.is_reply,
  },
];

/* ────────── Job card renderer ────────── */

function JobKanbanCard({
  job,
  onView,
  onChat,
  onDelete,
}: {
  job: JobListItem;
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
      style={{
        background: "#ffffff",
        borderRadius: 10,
        border: "1px solid #e2e8f0",
        padding: 12,
        display: "flex",
        flexDirection: "column",
        gap: 8,
        transition: "box-shadow 0.2s",
        cursor: "default",
      }}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLDivElement).style.boxShadow = "0 4px 12px rgba(0,0,0,0.06)";
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLDivElement).style.boxShadow = "none";
      }}
    >
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", gap: 8 }}>
        <Typography.Text strong style={{ fontSize: 14, lineHeight: 1.4, flex: 1 }}>
          {job.title}
        </Typography.Text>
        <Tag
          color={platform === "liepin" ? "purple" : "green"}
          style={{ margin: 0, flexShrink: 0, fontSize: 11 }}
        >
          {platform === "liepin" ? "猎聘" : "BOSS"}
        </Tag>
      </div>

      <Typography.Text type="secondary" style={{ fontSize: 12.5 }}>
        {job.company_name}
      </Typography.Text>

      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        {renderCommunicationStatus(job.communication_status)}
        {job.salary && (
          <Tag
            style={{
              margin: 0,
              fontSize: 11,
              background: "rgba(22,119,255,0.06)",
              color: "#1677ff",
              border: "none",
            }}
          >
            {job.salary}
          </Tag>
        )}
        {job.location && (
          <Tag
            style={{
              margin: 0,
              fontSize: 11,
              background: "#f8fafc",
              color: "#64748b",
              border: "none",
            }}
          >
            {job.location}
          </Tag>
        )}
      </div>

      <Typography.Text type="secondary" style={{ fontSize: 11 }}>
        {time}
      </Typography.Text>

      {/* actions */}
      <div style={{ display: "flex", gap: 4, marginTop: 2 }}>
        <Button size="small" type="text" icon={<EyeOutlined />} onClick={() => onView(job)}>
          详情
        </Button>
        <Button size="small" type="text" icon={<MessageOutlined />} onClick={() => onChat(job)}>
          沟通
        </Button>
        <Popconfirm
          title="确认删除"
          description={`确定要删除「${job.title}」吗？`}
          onConfirm={() => onDelete(job.id)}
          okText="确认删除"
          cancelText="取消"
          okButtonProps={{ danger: true }}
        >
          <Button size="small" type="text" danger icon={<DeleteOutlined />}>
            删除
          </Button>
        </Popconfirm>
      </div>
    </div>
  );
}

/* ────────── Kanban view ────────── */

function KanbanView({
  jobs,
  onView,
  onChat,
  onDelete,
}: {
  jobs: JobListItem[];
  onView: (job: JobDetail) => void;
  onChat: (job: JobDetail) => void;
  onDelete: (id: string) => void;
}) {
  const lanes = useMemo(
    () =>
      KANBAN_LANES.map((lane) => ({
        ...lane,
        jobs: jobs.filter(lane.filter),
      })),
    [jobs],
  );

  return (
    <div
      style={{
        flex: "1 1 0",
        minHeight: 0,
        display: "flex",
        gap: 16,
        overflowX: "auto",
        paddingBottom: 4,
      }}
    >
      {lanes.map((lane) => (
        <div
          key={lane.key}
          style={{
            flex: 1,
            minWidth: 260,
            display: "flex",
            flexDirection: "column",
            background: lane.bg,
            borderRadius: 14,
            border: `1px solid ${lane.border}`,
          }}
        >
          {/* lane header */}
          <div
            style={{
              padding: "10px 14px",
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
            }}
          >
            <Typography.Text strong style={{ color: lane.color, fontSize: 14 }}>
              {lane.label}
            </Typography.Text>
            <Tag color="default" style={{ margin: 0 }}>
              {lane.jobs.length}
            </Tag>
          </div>
          {/* lane cards */}
          <div
            style={{
              flex: 1,
              overflowY: "auto",
              padding: "0 10px 10px",
              display: "flex",
              flexDirection: "column",
              gap: 10,
            }}
          >
            {lane.jobs.length === 0 ? (
              <Typography.Text
                type="secondary"
                style={{ textAlign: "center", padding: 24, fontSize: 12 }}
              >
                暂无
              </Typography.Text>
            ) : (
              lane.jobs.map((job) => (
                <JobKanbanCard
                  key={job.id}
                  job={job}
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

const JobDataPage = ({ aiConfigured, onConfigureAi, focusJobId, onFocusHandled }: {
  aiConfigured: boolean;
  onConfigureAi: () => void;
  /** 从其他页面跳转过来时需要直接打开沟通记录的岗位 */
  focusJobId?: string;
  onFocusHandled?: () => void;
}) => {
  const [jobs, setJobs] = useState<JobListItem[]>([]);
  // 初始即为加载中，避免首次挂载时把待跳转的岗位当成「不存在」
  const [loading, setLoading] = useState(true);
  const [keyword, setKeyword] = useState("");
  const [communicationStatusFilter, setCommunicationStatusFilter] =
    useState<CommunicationStatus | "all">("all");
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

  useEffect(() => {
    void loadJobs();
  }, [loadJobs]);

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

  const handleBackFromReport = useCallback(() => {
    setCurrentJob(null);
  }, []);

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
    } catch (error: unknown) {
      messageApi.error(error instanceof Error ? error.message : "批量分析失败");
    } finally {
      setBatchAnalyzing(false);
    }
  }, [aiConfigured, loadJobs, messageApi, onConfigureAi, selectedJobIds]);

  /* ── detail fallback ── */
  if (currentJob) {
    return (
      <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
        {contextHolder}
        <AnalysisReport job={currentJob} onBack={handleBackFromReport} aiConfigured={aiConfigured} onConfigureAi={onConfigureAi} />
      </div>
    );
  }

  const normalizedKeyword = keyword.trim().toLowerCase();
  const filteredJobs = jobs.filter(
    (job) =>
      (!normalizedKeyword ||
        job.title.toLowerCase().includes(normalizedKeyword) ||
        job.company_name.toLowerCase().includes(normalizedKeyword)) &&
      (communicationStatusFilter === "all" ||
        job.communication_status === communicationStatusFilter),
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
