import { useMemo, useState } from "react";
import {
  Button,
  Card,
  Dropdown,
  Empty,
  Input,
  Modal,
  Progress,
  Segmented,
  Space,
  Table,
  Tag,
  Typography,
} from "antd";
import {
  DeleteOutlined,
  EllipsisOutlined,
  FileSearchOutlined,
  HistoryOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  RedoOutlined,
  SearchOutlined,
} from "@ant-design/icons";
import type { ColumnsType } from "antd/es/table";
import { DURATION_META, type InterviewSession, type InterviewSessionStatus } from "./interview-types";

interface MockInterviewHomeProps {
  sessions: InterviewSession[];
  canStart: boolean;
  onCreate: () => void;
  onContinue: (id: string) => void;
  onOpenReport: (id: string) => void;
  onOpenTranscript: (id: string) => void;
  onRestart: (id: string) => void;
  onDelete: (id: string) => void;
  onRetryReport: (id: string) => void;
}
type FilterKey = "all" | "unfinished" | "generating" | "completed";

const STATUS_META: Record<InterviewSessionStatus, { label: string; color: string }> = {
  paused: { label: "未完成", color: "gold" },
  in_progress: { label: "进行中", color: "blue" },
  report_queued: { label: "等待生成", color: "processing" },
  report_generating: { label: "报告生成中", color: "processing" },
  report_completed: { label: "已完成", color: "success" },
  report_failed: { label: "报告生成失败", color: "error" },
};

function formatDate(value: string): string {
  const date = new Date(value);
  const today = new Date();
  const sameDay = date.toDateString() === today.toDateString();
  return sameDay
    ? `今天 ${date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })}`
    : date.toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" });
}

function sessionProgress(session: InterviewSession): number {
  const total = session.modules.reduce((sum, item) => sum + item.weight, 0);
  const completed = session.modules.reduce((sum, item, index) => {
    if (index < session.currentModuleIndex) return sum + item.weight;
    if (index > session.currentModuleIndex) return sum;
    return sum + item.weight * Math.min(1, item.completedQuestions / Math.max(1, item.targetQuestions));
  }, 0);
  return Math.round((completed / total) * 100);
}

export function MockInterviewHome(props: MockInterviewHomeProps) {
  const [filter, setFilter] = useState<FilterKey>("all");
  const [query, setQuery] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<InterviewSession>();
  const unfinished = props.sessions
    .filter((item) => item.status === "paused" || item.status === "in_progress")
    .slice(0, 2);

  const tableData = useMemo(() => props.sessions.filter((session) => {
    const matchesQuery = `${session.settings.jobTitle} ${session.settings.companyName}`.toLowerCase().includes(query.trim().toLowerCase());
    if (!matchesQuery) return false;
    if (filter === "unfinished") return session.status === "paused" || session.status === "in_progress";
    if (filter === "generating") return session.status === "report_generating" || session.status === "report_queued" || session.status === "report_failed";
    if (filter === "completed") return session.status === "report_completed";
    return true;
  }), [filter, props.sessions, query]);

  const columns: ColumnsType<InterviewSession> = [
    {
      title: "岗位",
      key: "job",
      width: 220,
      render: (_, session) => (
        <div className="mi-table-job">
          <Typography.Text strong>{session.settings.jobTitle || "通用岗位面试"}</Typography.Text>
          <Typography.Text type="secondary">{session.settings.companyName || "未指定公司"}</Typography.Text>
        </div>
      ),
    },
    {
      title: "面试配置",
      key: "config",
      width: 180,
      render: (_, session) => (
        <div className="mi-table-config">
          <span>{session.settings.interviewType} · {session.settings.difficulty}</span>
          <Typography.Text type="secondary">{DURATION_META[session.settings.duration].label}</Typography.Text>
        </div>
      ),
    },
    {
      title: "更新时间",
      dataIndex: "updatedAt",
      width: 150,
      render: formatDate,
    },
    {
      title: "完成情况",
      key: "questions",
      width: 150,
      render: (_, session) => `${session.mainQuestionCount}个核心问题 · ${session.followUpCount}次追问`,
    },
    {
      title: "状态",
      dataIndex: "status",
      width: 125,
      render: (status: InterviewSessionStatus) => <Tag color={STATUS_META[status].color}>{STATUS_META[status].label}</Tag>,
    },
    {
      title: "得分",
      key: "score",
      width: 72,
      align: "center",
      render: (_, session) => session.report ? <strong>{session.report.overallScore}</strong> : <Typography.Text type="secondary">--</Typography.Text>,
    },
    {
      title: "操作",
      key: "actions",
      width: 170,
      fixed: "right",
      render: (_, session) => {
        const unfinishedSession = session.status === "paused" || session.status === "in_progress";
        const primaryAction = unfinishedSession
          ? <Button type="link" size="small" onClick={() => props.onContinue(session.id)}>继续面试</Button>
          : session.status === "report_completed"
            ? <Button type="link" size="small" onClick={() => props.onOpenReport(session.id)}>查看报告</Button>
            : session.status === "report_failed"
              ? <Button type="link" size="small" onClick={() => props.onRetryReport(session.id)}>重新生成</Button>
              : <Button type="link" size="small" loading>生成中</Button>;
        return (
          <Space size={0}>
            {primaryAction}
            <Dropdown
              trigger={["click"]}
              menu={{
                items: [
                  { key: "transcript", icon: <FileSearchOutlined />, label: "查看完整对话", onClick: () => props.onOpenTranscript(session.id) },
                  { key: "restart", icon: <RedoOutlined />, label: "使用相同配置重新面试", onClick: () => props.onRestart(session.id) },
                  { type: "divider" },
                  { key: "delete", danger: true, icon: <DeleteOutlined />, label: "删除记录", onClick: () => setDeleteTarget(session) },
                ],
              }}
            >
              <Button type="text" size="small" icon={<EllipsisOutlined />} aria-label="更多操作" />
            </Dropdown>
          </Space>
        );
      },
    },
  ];

  return (
    <div className="mi-page mi-home-page">
      <div className="mi-page-header">
        <div>
          <Typography.Title level={3}>AI 模拟面试</Typography.Title>
          <Typography.Paragraph type="secondary">基于目标岗位、简历和回答动态追问，形成可追溯的面试报告</Typography.Paragraph>
        </div>
        <Button type="primary" size="large" icon={<PlusOutlined />} disabled={!props.canStart} onClick={props.onCreate}>
          开始新的模拟面试
        </Button>
      </div>

      {unfinished.length > 0 && (
        <section className="mi-home-section">
          <div className="mi-section-title-row">
            <Typography.Title level={5}><PlayCircleOutlined /> 继续上次面试</Typography.Title>
            {props.sessions.filter((item) => item.status === "paused" || item.status === "in_progress").length > 2 && (
              <Typography.Text type="secondary">仅展示最近2条，其他记录可在下方查看</Typography.Text>
            )}
          </div>
          <div className={`mi-resume-grid ${unfinished.length === 1 ? "is-single" : ""}`}>
            {unfinished.map((session) => {
              const currentModule = session.modules[session.currentModuleIndex];
              return (
                <Card key={session.id} className="mi-resume-card">
                  <div className="mi-resume-card-main">
                    <div>
                      <Typography.Title level={5}>{session.settings.jobTitle || "通用岗位面试"}</Typography.Title>
                      <Typography.Text type="secondary">{session.settings.companyName || "未指定公司"}</Typography.Text>
                    </div>
                    <Tag color="blue">{session.settings.interviewType} · {session.settings.difficulty}</Tag>
                  </div>
                  <div className="mi-resume-meta">
                    <span>当前模块：{currentModule?.name || "准备开始"}</span>
                    <span>{session.mainQuestionCount}个核心问题 · {session.followUpCount}次追问</span>
                    <span>上次面试：{formatDate(session.updatedAt)}</span>
                  </div>
                  <div className="mi-resume-progress">
                    <Progress percent={sessionProgress(session)} showInfo={false} />
                    <Typography.Text type="secondary">{sessionProgress(session)}%</Typography.Text>
                    <Button type="primary" onClick={() => props.onContinue(session.id)}>继续面试</Button>
                  </div>
                </Card>
              );
            })}
          </div>
        </section>
      )}

      <section className="mi-home-section mi-history-section">
        <div className="mi-section-title-row">
          <Typography.Title level={5}><HistoryOutlined /> 历史面试</Typography.Title>
          <Input
            allowClear
            prefix={<SearchOutlined />}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="搜索岗位或公司"
            className="mi-history-search"
          />
        </div>
        <Segmented
          value={filter}
          onChange={(value) => setFilter(value as FilterKey)}
          options={[
            { label: "全部", value: "all" },
            { label: "未完成", value: "unfinished" },
            { label: "报告生成中", value: "generating" },
            { label: "已完成", value: "completed" },
          ]}
        />
        <Table
          className="mi-history-table"
          rowKey="id"
          columns={columns}
          dataSource={tableData}
          scroll={{ x: 1120 }}
          pagination={{ pageSize: 8, hideOnSinglePage: true }}
          locale={{ emptyText: <Empty description="暂无模拟面试记录" image={Empty.PRESENTED_IMAGE_SIMPLE} /> }}
        />
      </section>

      <Modal
        title="删除模拟面试记录？"
        open={!!deleteTarget}
        okText="确认删除"
        okButtonProps={{ danger: true }}
        cancelText="取消"
        onCancel={() => setDeleteTarget(undefined)}
        onOk={() => {
          if (deleteTarget) props.onDelete(deleteTarget.id);
          setDeleteTarget(undefined);
        }}
      >
        <Typography.Paragraph>删除后，该次对话和报告将无法恢复，不会影响岗位与简历数据。</Typography.Paragraph>
      </Modal>
    </div>
  );
}
