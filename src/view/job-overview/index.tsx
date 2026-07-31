import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Card,
  Col,
  Empty,
  Progress,
  Row,
  Segmented,
  Skeleton,
  Space,
  Table,
  Tag,
  Typography,
} from "antd";
import {
  BarChartOutlined,
  CheckCircleOutlined,
  CommentOutlined,
  FileDoneOutlined,
  RiseOutlined,
  StarOutlined,
} from "@ant-design/icons";
import type { CommandResult } from "@/types/command";
import "./style.css";

interface OverviewMetrics {
  total_jobs: number;
  communicated_jobs: number;
  replied_jobs: number;
  reply_rate: number;
  resume_sent_jobs: number;
  high_match_jobs: number;
}

interface DailyActivity {
  date: string;
  jobs: number;
  replies: number;
}

interface ActiveConversation {
  job_id: string;
  company_name: string;
  title: string;
  last_message: string;
  last_message_at: number;
  received: boolean;
  message_count: number;
}

interface JobSearchOverview {
  days: number;
  metrics: OverviewMetrics;
  daily_activity: DailyActivity[];
  active_conversations: ActiveConversation[];
}

const metricCards = [
  { key: "total_jobs", label: "活跃岗位", icon: <BarChartOutlined />, color: "#1677ff" },
  { key: "communicated_jobs", label: "有效沟通", icon: <CommentOutlined />, color: "#722ed1" },
  { key: "reply_rate", label: "BOSS 回复率", icon: <RiseOutlined />, color: "#13a8a8", suffix: "%" },
  { key: "resume_sent_jobs", label: "简历交换", icon: <FileDoneOutlined />, color: "#fa8c16" },
  { key: "high_match_jobs", label: "高匹配岗位", icon: <StarOutlined />, color: "#52c41a" },
] as const;

function formatMessageTime(timestamp: number) {
  if (!timestamp) return "-";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp));
}

export default function JobOverviewPage() {
  const [days, setDays] = useState(30);
  const [overview, setOverview] = useState<JobSearchOverview | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    invoke<CommandResult<JobSearchOverview>>("job_search_overview", { days })
      .then((result) => {
        if (!active) return;
        if (!result.success || !result.data) {
          throw new Error(result.error?.message || "加载求职数据失败");
        }
        setOverview(result.data);
      })
      .catch((reason: unknown) => {
        if (active) setError(reason instanceof Error ? reason.message : "加载求职数据失败");
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [days]);

  const suggestions = useMemo(() => {
    if (!overview) return [];
    const pendingReplies = overview.active_conversations.filter((item) => item.received).length;
    const values = [];
    if (pendingReplies > 0) {
      values.push({
        title: `${pendingReplies} 个活跃会话等待回复`,
        detail: "优先处理招聘方最后发言的会话，避免错过推进窗口。",
        tone: "warning",
      });
    }
    if (overview.metrics.reply_rate < 20 && overview.metrics.communicated_jobs >= 5) {
      values.push({
        title: "当前回复率偏低",
        detail: "建议收紧岗位筛选条件，并优化首次沟通话术。",
        tone: "danger",
      });
    } else {
      values.push({
        title: "沟通转化保持稳定",
        detail: `当前回复率 ${overview.metrics.reply_rate.toFixed(1)}%，继续关注高匹配岗位。`,
        tone: "success",
      });
    }
    if (overview.metrics.high_match_jobs > 0) {
      values.push({
        title: `${overview.metrics.high_match_jobs} 个高匹配岗位`,
        detail: "建议优先跟进匹配度 80 分以上的岗位并准备针对性面试内容。",
        tone: "primary",
      });
    }
    return values;
  }, [overview]);

  if (loading && !overview) {
    return <Skeleton active paragraph={{ rows: 10 }} />;
  }

  if (error) {
    return <Alert showIcon type="error" message="求职数据加载失败" description={error} />;
  }

  if (!overview) return <Empty description="暂无求职数据" />;

  const metrics = overview.metrics;
  const chartMax = Math.max(
    1,
    ...overview.daily_activity.flatMap((item) => [item.jobs, item.replies]),
  );
  const funnel = [
    { label: "活跃岗位", value: metrics.total_jobs, color: "#1677ff" },
    { label: "有效沟通", value: metrics.communicated_jobs, color: "#2f54eb" },
    { label: "收到回复", value: metrics.replied_jobs, color: "#722ed1" },
    { label: "简历交换", value: metrics.resume_sent_jobs, color: "#13a8a8" },
  ];

  return (
    <div className="job-overview">
      <div className="overview-header">
        <div>
          <Typography.Title level={3}>求职数据概览</Typography.Title>
          <Typography.Text type="secondary">
            从岗位、沟通到简历交换，快速了解当前求职进展
          </Typography.Text>
        </div>
        <Segmented
          value={days}
          options={[
            { label: "近 7 天", value: 7 },
            { label: "近 30 天", value: 30 },
          ]}
          onChange={(value) => setDays(Number(value))}
        />
      </div>

      <Row gutter={[12, 12]} className="overview-metrics">
        {metricCards.map((item) => {
          const raw = metrics[item.key];
          const value =
            item.key === "reply_rate" ? Number(raw).toFixed(1) : Number(raw).toLocaleString();
          return (
            <Col flex="1 1 160px" key={item.key}>
              <Card className="metric-card">
                <div className="metric-icon" style={{ color: item.color, background: `${item.color}14` }}>
                  {item.icon}
                </div>
                <div>
                  <Typography.Text type="secondary">{item.label}</Typography.Text>
                  <div className="metric-value">
                    {value}
                    {"suffix" in item ? item.suffix : ""}
                  </div>
                </div>
              </Card>
            </Col>
          );
        })}
      </Row>

      <Row gutter={[14, 14]}>
        <Col xs={24} xl={15}>
          <Card title="求职转化漏斗" extra={<Typography.Text type="secondary">近 {days} 天</Typography.Text>}>
            <div className="overview-funnel">
              {funnel.map((item) => (
                <div className="funnel-row" key={item.label}>
                  <div className="funnel-label">
                    <span>{item.label}</span>
                    <strong>{item.value}</strong>
                  </div>
                  <Progress
                    percent={metrics.total_jobs ? Math.round((item.value / metrics.total_jobs) * 100) : 0}
                    showInfo={false}
                    strokeColor={item.color}
                    trailColor="#f1f5f9"
                  />
                </div>
              ))}
            </div>
          </Card>
        </Col>
        <Col xs={24} xl={9}>
          <Card title="行动建议" className="suggestion-card">
            <div className="overview-suggestions">
              {suggestions.map((item) => (
                <div className={`suggestion-item ${item.tone}`} key={item.title}>
                  <CheckCircleOutlined />
                  <div>
                    <strong>{item.title}</strong>
                    <span>{item.detail}</span>
                  </div>
                </div>
              ))}
            </div>
          </Card>
        </Col>
      </Row>

      <Row gutter={[14, 14]} className="overview-bottom">
        <Col xs={24} xl={10}>
          <Card title="每日活动趋势">
            <div className="activity-chart">
              {overview.daily_activity.map((item, index) => (
                <div className="activity-column" key={item.date}>
                  <div className="activity-bars">
                    <div
                      className="activity-bar jobs"
                      style={{ height: `${Math.max(3, (item.jobs / chartMax) * 100)}%` }}
                      title={`新增岗位 ${item.jobs}`}
                    />
                    <div
                      className="activity-bar replies"
                      style={{ height: `${Math.max(3, (item.replies / chartMax) * 100)}%` }}
                      title={`收到回复 ${item.replies}`}
                    />
                  </div>
                  {(days === 7 || index % 5 === 0 || index === overview.daily_activity.length - 1) && (
                    <span>{item.date}</span>
                  )}
                </div>
              ))}
            </div>
            <Space className="chart-legend">
              <span><i className="legend-dot jobs" />新增岗位</span>
              <span><i className="legend-dot replies" />收到回复</span>
            </Space>
          </Card>
        </Col>
        <Col xs={24} xl={14}>
          <Card title="活跃沟通" extra={<Typography.Text type="secondary">最近更新</Typography.Text>}>
            <Table
              rowKey="job_id"
              size="small"
              pagination={false}
              dataSource={overview.active_conversations}
              locale={{ emptyText: "当前时间范围内暂无沟通记录" }}
              columns={[
                {
                  title: "公司 / 岗位",
                  render: (_, item: ActiveConversation) => (
                    <div>
                      <strong>{item.company_name || "未知公司"}</strong>
                      <div className="conversation-job">{item.title || "未知岗位"}</div>
                    </div>
                  ),
                },
                {
                  title: "最近消息",
                  dataIndex: "last_message",
                  ellipsis: true,
                },
                {
                  title: "状态",
                  width: 90,
                  render: (_, item: ActiveConversation) =>
                    item.received ? <Tag color="orange">待回复</Tag> : <Tag color="blue">等待回复</Tag>,
                },
                {
                  title: "更新时间",
                  width: 105,
                  dataIndex: "last_message_at",
                  render: formatMessageTime,
                },
              ]}
            />
          </Card>
        </Col>
      </Row>
    </div>
  );
}
