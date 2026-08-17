import { useEffect, useMemo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Alert, Button, Card, Col, Empty, Row, Segmented, Skeleton, Table, Tooltip, Typography } from "antd";
import {
  BarChartOutlined,
  CalendarOutlined,
  CaretDownOutlined,
  CaretUpOutlined,
  CommentOutlined,
  ExclamationCircleFilled,
  FileDoneOutlined,
  InfoCircleOutlined,
  RightOutlined,
  RiseOutlined,
  StarFilled,
  StarOutlined,
  ThunderboltFilled,
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
  communicated: number;
  resume_sent: number;
  high_match: number;
}

interface SourceSlice {
  source: string;
  count: number;
}

interface ActiveConversation {
  job_id: string;
  company_name: string;
  title: string;
  last_message: string;
  last_message_at: number;
  received: boolean;
  has_reply: boolean;
  message_count: number;
}

interface JobSearchOverview {
  days: number;
  metrics: OverviewMetrics;
  previous_metrics: OverviewMetrics;
  daily_activity: DailyActivity[];
  source_distribution: SourceSlice[];
  active_conversations: ActiveConversation[];
}

type MetricKey = "total_jobs" | "communicated_jobs" | "reply_rate" | "resume_sent_jobs" | "high_match_jobs";

interface MetricCard {
  key: MetricKey;
  label: string;
  icon: ReactNode;
  color: string;
  suffix?: string;
  /** 卡片右下角迷你趋势线取数 */
  series: (item: DailyActivity) => number;
}

const METRIC_CARDS: MetricCard[] = [
  { key: "total_jobs", label: "活跃岗位", icon: <BarChartOutlined />, color: "#1677ff", series: (item) => item.jobs },
  { key: "communicated_jobs", label: "有效沟通", icon: <CommentOutlined />, color: "#722ed1", series: (item) => item.communicated },
  {
    key: "reply_rate",
    label: "HR 回复率",
    icon: <RiseOutlined />,
    color: "#13a8a8",
    suffix: "%",
    series: (item) => (item.communicated ? (item.replies * 100) / item.communicated : 0),
  },
  { key: "resume_sent_jobs", label: "简历交换", icon: <FileDoneOutlined />, color: "#fa8c16", series: (item) => item.resume_sent },
  { key: "high_match_jobs", label: "高匹配岗位", icon: <StarOutlined />, color: "#52c41a", series: (item) => item.high_match },
];

const RANGES = [
  { label: "今日", value: 0 },
  { label: "近 7 天", value: 7 },
  { label: "近 30 天", value: 30 },
];

const SOURCE_COLORS = ["#1677ff", "#722ed1", "#13c2c2", "#fa8c16", "#2f54eb"];
const OTHER_SOURCE_COLOR = "#94a3b8";
const AVATAR_COLORS = ["#1677ff", "#722ed1", "#13c2c2", "#fa8c16", "#52c41a", "#eb2f96"];

function formatMessageTime(timestamp: number) {
  if (!timestamp) return "-";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp));
}

function formatPercent(value: number) {
  return Number.isFinite(value) ? value.toFixed(1) : "0.0";
}

function avatarColor(name: string) {
  let hash = 0;
  for (const character of name) {
    hash = (hash * 31 + (character.codePointAt(0) ?? 0)) >>> 0;
  }
  return AVATAR_COLORS[hash % AVATAR_COLORS.length];
}

/** 把轴上限抬到易读的整刻度 */
function niceMax(value: number, ticks = 4) {
  if (value <= 0) return ticks;
  const rough = value / ticks;
  const magnitude = 10 ** Math.floor(Math.log10(rough));
  const normalized = rough / magnitude;
  const step = (normalized <= 1 ? 1 : normalized <= 2 ? 2 : normalized <= 5 ? 5 : 10) * magnitude;
  return step * ticks;
}

/** 逐段三次贝塞尔平滑，用于迷你趋势线与折线图 */
function smoothPath(points: Array<[number, number]>) {
  if (points.length === 0) return "";
  if (points.length === 1) return `M${points[0][0]},${points[0][1]}`;
  return points.reduce((path, [x, y], index) => {
    if (index === 0) return `M${x},${y}`;
    const [prevX, prevY] = points[index - 1];
    const midX = (prevX + x) / 2;
    return `${path} C${midX},${prevY} ${midX},${y} ${x},${y}`;
  }, "");
}

function Sparkline({ values, color }: { values: number[]; color: string }) {
  const width = 92;
  const height = 30;
  const recent = values.slice(-14);
  if (recent.length < 2) return <svg className="metric-spark" viewBox={`0 0 ${width} ${height}`} />;
  const max = Math.max(...recent);
  const min = Math.min(...recent);
  const span = max - min || 1;
  const step = width / (recent.length - 1);
  const points: Array<[number, number]> = recent.map((value, index) => [
    Number((index * step).toFixed(2)),
    Number((height - 3 - ((value - min) / span) * (height - 6)).toFixed(2)),
  ]);
  const line = smoothPath(points);
  return (
    <svg className="metric-spark" viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none">
      <path d={`${line} L${width},${height} L0,${height} Z`} fill={color} opacity={0.08} />
      <path d={line} fill="none" stroke={color} strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

interface MetricDelta {
  text: string;
  up: boolean;
  flat: boolean;
  title: string;
}

function metricDelta(card: MetricCard, current: OverviewMetrics, previous: OverviewMetrics): MetricDelta {
  const now = current[card.key];
  const before = previous[card.key];
  // 回复率按百分点差比较，其余按相对涨跌幅
  if (card.key === "reply_rate") {
    const diff = now - before;
    const flat = Math.abs(diff) < 0.05;
    return {
      text: flat ? "持平" : `${Math.abs(diff).toFixed(1)}%`,
      up: diff >= 0,
      flat,
      title: `上一周期 ${formatPercent(before)}% → 当前 ${formatPercent(now)}%`,
    };
  }
  const title = `上一周期 ${before} → 当前 ${now}`;
  if (before === 0) {
    return { text: now > 0 ? "新增" : "持平", up: now > 0, flat: now === 0, title };
  }
  const diff = ((now - before) * 100) / before;
  const flat = Math.abs(diff) < 0.05;
  return { text: flat ? "持平" : `${Math.abs(diff).toFixed(1)}%`, up: diff >= 0, flat, title };
}

interface FunnelStep {
  label: string;
  desc: string;
  value: number;
  from: string;
  to: string;
}

function FunnelChart({ steps }: { steps: FunnelStep[] }) {
  const width = 300;
  const height = 208;
  const layerHeight = height / steps.length;
  const head = steps[0]?.value ?? 0;
  // 漏斗宽度 = 均匀收窄基线与数值占比的折中，既保持漏斗形态又能体现落差
  const widths = steps.map((step, index) => {
    const uniform = 1 - (index * 0.66) / Math.max(steps.length - 1, 1);
    const scaled = head > 0 ? Math.min(0.28 + 0.72 * (step.value / head) ** 0.65, 1) : uniform;
    return (uniform + scaled) / 2;
  });
  widths.forEach((value, index) => {
    if (index > 0) widths[index] = Math.min(value, widths[index - 1]);
  });
  return (
    <svg className="funnel-chart" viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="xMidYMid meet">
      <defs>
        {steps.map((step, index) => (
          <linearGradient id={`funnel-grad-${index}`} key={step.label} x1="0" y1="0" x2="1" y2="0">
            <stop offset="0%" stopColor={step.from} />
            <stop offset="100%" stopColor={step.to} />
          </linearGradient>
        ))}
      </defs>
      {steps.map((step, index) => {
        const topWidth = widths[index] * width;
        const bottomWidth = (widths[index + 1] ?? widths[index] * 0.62) * width;
        const top = index * layerHeight;
        const bottom = top + layerHeight - 3;
        const center = width / 2;
        const points = [
          `${center - topWidth / 2},${top}`,
          `${center + topWidth / 2},${top}`,
          `${center + bottomWidth / 2},${bottom}`,
          `${center - bottomWidth / 2},${bottom}`,
        ].join(" ");
        return <polygon key={step.label} points={points} fill={`url(#funnel-grad-${index})`} />;
      })}
    </svg>
  );
}

function TrendChart({ data }: { data: DailyActivity[] }) {
  const width = 470;
  const height = 236;
  const padLeft = 32;
  const padRight = 30;
  const padTop = 14;
  const padBottom = 28;
  const innerWidth = width - padLeft - padRight;
  const innerHeight = height - padTop - padBottom;
  const jobsMax = niceMax(Math.max(...data.map((item) => item.jobs), 0));
  const repliesMax = niceMax(Math.max(...data.map((item) => item.replies), 0));
  const slot = innerWidth / Math.max(data.length, 1);
  const barWidth = Math.max(3, Math.min(18, slot * 0.38));
  const centerX = (index: number) => padLeft + slot * index + slot / 2;
  const jobsY = (value: number) => padTop + innerHeight - (value / jobsMax) * innerHeight;
  const repliesY = (value: number) => padTop + innerHeight - (value / repliesMax) * innerHeight;
  const labelStep = Math.max(1, Math.ceil(data.length / 7));
  const linePoints: Array<[number, number]> = data.map((item, index) => [centerX(index), repliesY(item.replies)]);

  return (
    <svg className="trend-chart" viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="xMidYMid meet">
      <defs>
        <linearGradient id="trend-bar" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#5aa9ff" />
          <stop offset="100%" stopColor="#1677ff" />
        </linearGradient>
      </defs>
      {[0, 1, 2, 3, 4].map((tick) => {
        const y = padTop + innerHeight - (innerHeight * tick) / 4;
        return (
          <g key={tick}>
            <line x1={padLeft} y1={y} x2={width - padRight} y2={y} stroke="#eef2f7" strokeWidth={1} />
            <text className="axis-label" x={padLeft - 7} y={y + 3} textAnchor="end">
              {Math.round((jobsMax * tick) / 4)}
            </text>
            <text className="axis-label" x={width - padRight + 7} y={y + 3} textAnchor="start">
              {Math.round((repliesMax * tick) / 4)}
            </text>
          </g>
        );
      })}
      {data.map((item, index) => {
        const y = jobsY(item.jobs);
        const barHeight = Math.max(item.jobs > 0 ? 2 : 0, padTop + innerHeight - y);
        return (
          <rect
            key={item.date}
            className="trend-bar"
            x={centerX(index) - barWidth / 2}
            y={padTop + innerHeight - barHeight}
            width={barWidth}
            height={barHeight}
            rx={Math.min(3, barWidth / 2)}
          >
            <title>{`${item.date} 新增岗位 ${item.jobs}`}</title>
          </rect>
        );
      })}
      <path d={smoothPath(linePoints)} fill="none" stroke="#722ed1" strokeWidth={2} strokeLinecap="round" />
      {data.map((item, index) => (
        <circle key={item.date} cx={centerX(index)} cy={repliesY(item.replies)} r={3} fill="#ffffff" stroke="#722ed1" strokeWidth={1.8}>
          <title>{`${item.date} 收到回复 ${item.replies}`}</title>
        </circle>
      ))}
      {data.map((item, index) =>
        index % labelStep === 0 || index === data.length - 1 ? (
          <text className="axis-label" key={item.date} x={centerX(index)} y={height - 9} textAnchor="middle">
            {item.date}
          </text>
        ) : null,
      )}
    </svg>
  );
}

function DonutChart({ slices, total }: { slices: SourceSlice[]; total: number }) {
  const size = 156;
  const radius = 58;
  const stroke = 19;
  const circumference = 2 * Math.PI * radius;
  const visible = slices.filter((slice) => slice.count > 0);
  let consumed = 0;
  return (
    <svg className="donut-chart" viewBox={`0 0 ${size} ${size}`} preserveAspectRatio="xMidYMid meet">
      <circle cx={size / 2} cy={size / 2} r={radius} fill="none" stroke="#f1f5f9" strokeWidth={stroke} />
      <g transform={`rotate(-90 ${size / 2} ${size / 2})`}>
        {visible.map((slice, index) => {
          const length = total > 0 ? (slice.count / total) * circumference : 0;
          const gap = visible.length > 1 ? Math.min(6, length * 0.35) : 0;
          const drawn = Math.max(length - gap, 0.5);
          const node = (
            <circle
              key={slice.source}
              cx={size / 2}
              cy={size / 2}
              r={radius}
              fill="none"
              stroke={sourceColor(slice.source, index)}
              strokeWidth={stroke}
              strokeLinecap={visible.length > 1 ? "round" : "butt"}
              strokeDasharray={`${drawn} ${Math.max(circumference - drawn, 0)}`}
              strokeDashoffset={-(consumed + gap / 2)}
            />
          );
          consumed += length;
          return node;
        })}
      </g>
      <text className="donut-total" x={size / 2} y={size / 2 + 2} textAnchor="middle">
        {total.toLocaleString()}
      </text>
      <text className="donut-caption" x={size / 2} y={size / 2 + 20} textAnchor="middle">
        活跃岗位
      </text>
    </svg>
  );
}

function sourceColor(source: string, index: number) {
  return source === "其他" ? OTHER_SOURCE_COLOR : SOURCE_COLORS[index % SOURCE_COLORS.length];
}

/** 建议卡片的跳转目标：岗位管理页，或配置中心的某个分组 */
type SuggestionTarget = "job-data" | "greet" | "job";

interface Suggestion {
  key: string;
  tone: "danger" | "warning" | "success" | "primary";
  icon: ReactNode;
  title: string;
  detail: string;
  target?: SuggestionTarget;
}

export default function JobOverviewPage({ onNavigate }: { onNavigate?: (target: SuggestionTarget) => void } = {}) {
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

  const suggestions = useMemo<Suggestion[]>(() => {
    if (!overview) return [];
    const metrics = overview.metrics;
    const pendingReplies = overview.active_conversations.filter((item) => item.received).length;
    const items: Suggestion[] = [];
    if (pendingReplies > 0) {
      items.push({
        key: "pending",
        tone: "warning",
        icon: <CommentOutlined />,
        title: `${pendingReplies} 个会话等待你回复`,
        detail: "招聘方最后发言的会话优先处理，避免错过推进窗口。",
        target: "job-data",
      });
    }
    if (metrics.communicated_jobs >= 5 && metrics.reply_rate < 20) {
      items.push({
        key: "reply-rate",
        tone: "danger",
        icon: <ExclamationCircleFilled />,
        title: "当前回复率偏低",
        detail: "建议收紧岗位筛选并优化首轮沟通话术，提升回复率。",
        target: "greet",
      });
    }
    if (metrics.high_match_jobs < 5) {
      items.push({
        key: "high-match",
        tone: "primary",
        icon: <StarFilled />,
        title: "高匹配岗位偏少",
        detail: "建议提升技能标签完整度与关键词匹配，增加高匹配机会。",
        target: "job",
      });
    }
    if (metrics.replied_jobs > metrics.resume_sent_jobs) {
      items.push({
        key: "resume",
        tone: "success",
        icon: <RiseOutlined />,
        title: "简历交换转化仍有提升空间",
        detail: "可优先跟进已回复岗位，提高简历交换成功率。",
        target: "job-data",
      });
    }
    if (items.length === 0) {
      items.push({
        key: "healthy",
        tone: "success",
        icon: <ThunderboltFilled />,
        title: "当前求职节奏良好",
        detail: `回复率 ${formatPercent(metrics.reply_rate)}%，保持现有投递与沟通策略即可。`,
      });
    }
    return items.slice(0, 3);
  }, [overview]);

  if (loading && !overview) {
    return <Skeleton active paragraph={{ rows: 10 }} />;
  }

  if (error) {
    return <Alert showIcon type="error" message="求职数据加载失败" description={error} />;
  }

  if (!overview) return <Empty description="暂无求职数据" />;

  const metrics = overview.metrics;
  const funnelSteps: FunnelStep[] = [
    { label: "活跃岗位", desc: "浏览或收藏的岗位数", value: metrics.total_jobs, from: "#4c8dfd", to: "#6f6cf5" },
    { label: "有效沟通", desc: "成功发起沟通数", value: metrics.communicated_jobs, from: "#7c62f8", to: "#9b5cf6" },
    { label: "收到回复", desc: "企业或 HR 的回复数", value: metrics.replied_jobs, from: "#1fb6a6", to: "#34d3bd" },
    { label: "简历交换", desc: "与企业完成简历交换", value: metrics.resume_sent_jobs, from: "#fb923c", to: "#fbbf24" },
  ];
  const sourceTotal = overview.source_distribution.reduce((sum, item) => sum + item.count, 0);
  const rangeLabel = RANGES.find((item) => item.value === days)?.label ?? `近 ${days} 天`;

  return (
    <div className={`job-overview${loading ? " is-loading" : ""}`}>
      <div className="overview-header">
        <div className="overview-heading">
          <Typography.Title level={3}>求职数据概览</Typography.Title>
          <Typography.Text type="secondary">
            从活跃岗位到沟通交流，全面跟踪求职进展，助你更高效地拿到心仪 Offer。
          </Typography.Text>
        </div>
        <Segmented
          className="overview-range"
          value={days}
          options={RANGES.map((item) => ({
            value: item.value,
            label: (
              <span className="range-option">
                {days === item.value ? <CalendarOutlined /> : null}
                {item.label}
              </span>
            ),
          }))}
          onChange={(value) => setDays(Number(value))}
        />
      </div>

      <Row gutter={[14, 14]} className="overview-metrics">
        {METRIC_CARDS.map((card) => {
          const raw = metrics[card.key];
          const value = card.key === "reply_rate" ? formatPercent(Number(raw)) : Number(raw).toLocaleString();
          const delta = metricDelta(card, metrics, overview.previous_metrics);
          return (
            <Col flex="1 1 200px" key={card.key}>
              <Card className="metric-card">
                <div className="metric-top">
                  <span className="metric-icon" style={{ color: card.color, background: `${card.color}14` }}>
                    {card.icon}
                  </span>
                  <Typography.Text type="secondary" className="metric-label">
                    {card.label}
                  </Typography.Text>
                </div>
                <div className="metric-value">
                  {value}
                  {card.suffix ?? ""}
                </div>
                <div className="metric-foot">
                  <span
                    className={`metric-delta ${delta.flat ? "flat" : delta.up ? "up" : "down"}`}
                    title={delta.title}
                  >
                    <span>较上周期</span>
                    {delta.flat ? null : delta.up ? <CaretUpOutlined /> : <CaretDownOutlined />}
                    <span>{delta.text}</span>
                  </span>
                  <Sparkline values={overview.daily_activity.map(card.series)} color={card.color} />
                </div>
              </Card>
            </Col>
          );
        })}
      </Row>

      <Row gutter={[14, 14]} className="overview-mid">
        <Col xs={24} xl={13}>
          <Card
            className="panel-card"
            title={
              <span className="card-title-with-tip">
                求职转化漏斗
                <Tooltip title="百分比为该环节相对上一环节的转化率">
                  <InfoCircleOutlined />
                </Tooltip>
              </span>
            }
            extra={<Typography.Text type="secondary">{rangeLabel}</Typography.Text>}
          >
            <div className="funnel-layout">
              <FunnelChart steps={funnelSteps} />
              <div className="funnel-legend">
                {funnelSteps.map((step, index) => {
                  const previous = index > 0 ? funnelSteps[index - 1].value : 0;
                  const rate = index > 0 && previous > 0 ? (step.value * 100) / previous : null;
                  return (
                    <div className="funnel-legend-row" key={step.label}>
                      <span className="funnel-dot" style={{ background: step.to }} />
                      <div className="funnel-legend-text">
                        <strong>{step.label}</strong>
                        <span>{step.desc}</span>
                      </div>
                      <div className="funnel-legend-value">
                        <b>{step.value.toLocaleString()}</b>
                        {rate === null ? null : (
                          <em title={`较上一环节 ${funnelSteps[index - 1].label}`}>{formatPercent(rate)}%</em>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          </Card>
        </Col>
        <Col xs={24} xl={11}>
          <Card className="panel-card suggestion-card" title="行动建议">
            <div className="overview-suggestions">
              {suggestions.map((item) => (
                <button
                  type="button"
                  className={`suggestion-item ${item.tone}${item.target ? " clickable" : ""}`}
                  key={item.key}
                  onClick={item.target ? () => onNavigate?.(item.target!) : undefined}
                >
                  <span className="suggestion-icon">{item.icon}</span>
                  <span className="suggestion-text">
                    <strong>{item.title}</strong>
                    <span>{item.detail}</span>
                  </span>
                  {item.target ? <RightOutlined className="suggestion-arrow" /> : null}
                </button>
              ))}
            </div>
          </Card>
        </Col>
      </Row>

      <Row gutter={[14, 14]} className="overview-bottom">
        <Col xs={24} lg={12} xl={8}>
          <Card className="panel-card" title="每日活动趋势">
            <div className="chart-legend">
              <span>
                <i className="legend-dot jobs" />
                新增岗位
              </span>
              <span>
                <i className="legend-dot replies" />
                收到回复
              </span>
            </div>
            <TrendChart data={overview.daily_activity} />
          </Card>
        </Col>
        <Col xs={24} lg={12} xl={5}>
          <Card className="panel-card" title="岗位来源分布">
            {sourceTotal === 0 ? (
              <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="暂无岗位来源数据" />
            ) : (
              <div className="source-layout">
                <DonutChart slices={overview.source_distribution} total={sourceTotal} />
                <div className="source-legend">
                  {overview.source_distribution.map((slice, index) => (
                    <div className="source-legend-row" key={slice.source}>
                      <span className="source-dot" style={{ background: sourceColor(slice.source, index) }} />
                      <span className="source-name">{slice.source}</span>
                      <span className="source-value">
                        {formatPercent((slice.count * 100) / sourceTotal)}%
                        <em>（{slice.count}）</em>
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </Card>
        </Col>
        <Col xs={24} xl={11}>
          <Card
            className="panel-card conversation-card"
            title="活跃沟通"
            extra={
              <Button type="link" size="small" className="card-link" onClick={() => onNavigate?.("job-data")}>
                查看全部
              </Button>
            }
          >
            <Table
              rowKey="job_id"
              size="small"
              className="conversation-table"
              pagination={false}
              dataSource={overview.active_conversations}
              locale={{ emptyText: "当前时间范围内暂无沟通记录" }}
              columns={[
                {
                  title: "公司 / 岗位",
                  width: 210,
                  render: (_, item: ActiveConversation) => {
                    const name = item.company_name || "未知公司";
                    return (
                      <div className="conversation-company">
                        <span className="company-avatar" style={{ background: avatarColor(name) }}>
                          {name.slice(0, 1)}
                        </span>
                        <div className="conversation-company-text">
                          <strong title={name}>{name}</strong>
                          <span title={item.title}>{item.title || "未知岗位"}</span>
                        </div>
                      </div>
                    );
                  },
                },
                {
                  title: "最近消息",
                  dataIndex: "last_message",
                  ellipsis: true,
                },
                {
                  title: "状态",
                  width: 86,
                  render: (_, item: ActiveConversation) => {
                    const status = item.received
                      ? { label: "待跟进", tone: "pending" }
                      : item.has_reply
                        ? { label: "已沟通", tone: "done" }
                        : { label: "等待回复", tone: "waiting" };
                    return <span className={`status-chip ${status.tone}`}>{status.label}</span>;
                  },
                },
                {
                  title: "更新时间",
                  width: 108,
                  dataIndex: "last_message_at",
                  render: formatMessageTime,
                },
              ]}
            />
          </Card>
        </Col>
      </Row>

      <div className="overview-footnote">
        <InfoCircleOutlined />
        <span>数据统计基于所选时间范围内的活动记录，趋势图固定展示最近 {overview.daily_activity.length} 天走势。</span>
      </div>
    </div>
  );
}
