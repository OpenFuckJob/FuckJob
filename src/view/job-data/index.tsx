import { useCallback, useEffect, useState } from "react";
import {
  Button,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Space,
  Table,
  Tag,
  Typography,
  message,
} from "antd";
import {
  CloudDownloadOutlined,
  DeleteOutlined,
  EyeOutlined,
  MessageOutlined,
  SearchOutlined,
} from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import type { ColumnsType } from "antd/es/table";
import type { CommandResult } from "../../types/command";
import { commandErrorMessage } from "../../types/command";
import type { ChatMessageRecord, JobDetail } from "../../types/job-detail";
import AnalysisReport from "./AnalysisReport";

const getJobPlatform = (job: JobDetail): "boss" | "liepin" =>
  job.platform === "liepin" || job.id.startsWith("liepin:")
    ? "liepin"
    : "boss";

interface CollectCommunicatedJobsResult {
  inserted: number;
  updated: number;
  skipped: number;
  messages_inserted: number;
  total: number;
}

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

const JobDataPage = ({ aiConfigured, onConfigureAi }: { aiConfigured: boolean; onConfigureAi: () => void }) => {
  const [jobs, setJobs] = useState<JobDetail[]>([]);
  const [loading, setLoading] = useState(false);
  const [keyword, setKeyword] = useState("");
  const [pageLimit, setPageLimit] = useState(1);
  const [collectingCommunicated, setCollectingCommunicated] = useState(false);
  const [currentJob, setCurrentJob] = useState<JobDetail | null>(null);
  const [chatJob, setChatJob] = useState<JobDetail | null>(null);
  const [messageApi, contextHolder] = message.useMessage();

  const loadJobs = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<CommandResult<JobDetail[]>>("job_list");
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

  const handleCollectCommunicated = useCallback(async () => {
    setCollectingCommunicated(true);
    try {
      const result = await invoke<CommandResult<CollectCommunicatedJobsResult>>(
        "job_collect_communicated",
        { pageLimit },
      );
      if (!result.success || result.data === null) {
        messageApi.error(
          commandErrorMessage(result.error, "抓取已沟通过岗位失败"),
        );
        return;
      }

      const { inserted, updated, skipped, messages_inserted, total } = result.data;
      messageApi.success(
        `抓取完成：读取 ${total} 个，新增 ${inserted} 个，更新 ${updated} 个，新增沟通记录 ${messages_inserted} 条${
          skipped > 0 ? `，跳过 ${skipped} 个` : ""
        }`,
      );
      void loadJobs();
    } catch (error: unknown) {
      messageApi.error(
        error instanceof Error ? error.message : "抓取已沟通过岗位失败",
      );
    } finally {
      setCollectingCommunicated(false);
    }
  }, [loadJobs, messageApi, pageLimit]);

  const handleBackFromReport = useCallback(() => {
    setCurrentJob(null);
  }, []);

  if (currentJob) {
    return (
      <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
        {contextHolder}
        <AnalysisReport job={currentJob} onBack={handleBackFromReport} aiConfigured={aiConfigured} onConfigureAi={onConfigureAi} />
      </div>
    );
  }

  const filteredJobs = keyword.trim()
    ? jobs.filter((j) =>
      j.title.toLowerCase().includes(keyword.trim().toLowerCase()),
    )
    : jobs;

  const columns: ColumnsType<JobDetail> = [
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
      title: "平台",
      key: "platform",
      width: 90,
      render: (_: unknown, record: JobDetail) =>
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
      render: (_: unknown, record: JobDetail) =>
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
      render: (_: unknown, record: JobDetail) => (
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
          <Space.Compact>
            <InputNumber
              min={1}
              max={20}
              precision={0}
              value={pageLimit}
              onChange={(value) =>
                setPageLimit(Math.min(20, Math.max(1, Number(value) || 1)))
              }
              style={{ width: 92 }}
            />
            <Button
              type="primary"
              icon={<CloudDownloadOutlined />}
              loading={collectingCommunicated}
              onClick={() => void handleCollectCommunicated()}
            >
              抓取已沟通过
            </Button>
          </Space.Compact>
          <Input
            placeholder="搜索岗位名称"
            prefix={<SearchOutlined />}
            allowClear
            style={{ width: 260 }}
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
          />
        </Space>
      </div>

      <div
        style={{
          flex: "1 1 0",
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        <Table<JobDetail>
          className="job-data-table"
          rowKey="id"
          columns={columns}
          dataSource={filteredJobs}
          loading={loading}
          size="middle"
          scroll={{ x: 1300, y: "calc(100vh - 290px)" }}
          pagination={{
            defaultPageSize: 15,
            showSizeChanger: true,
            showTotal: (t) => `共 ${t} 条`,
          }}
        />
      </div>

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
