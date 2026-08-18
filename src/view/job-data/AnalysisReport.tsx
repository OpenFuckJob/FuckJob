import { useCallback, useEffect, useState } from "react";
import {
  Button,
  Card,
  Descriptions,
  Empty,
  Modal,
  Progress,
  Row,
  Col,
  Space,
  Spin,
  Table,
  Tabs,
  Tag,
  Typography,
  message,
} from "antd";
import {
  ArrowLeftOutlined,
  ThunderboltOutlined,
} from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import type { CommandResult } from "../../types/command";
import { commandErrorMessage } from "../../types/command";
import type {
  InterviewJobAnalysis,
  InterviewQuestion,
  SkillEvidence,
} from "../../types/analysis";
import type { JobDetail } from "../../types/job-detail";
import { AiFeatureGate } from "@/components/AiFeatureGate";

const scoreColor = (s: number): string => {
  if (s >= 80) return "#52c41a";
  if (s >= 60) return "#faad14";
  return "#ff4d4f";
};

interface AnalysisReportProps {
  job: JobDetail;
  /** 整页形态下的返回入口。放在弹窗里时不传，标题与关闭都交给弹窗 */
  onBack?: () => void;
  aiConfigured: boolean;
  onConfigureAi: () => void;
  /** 分析完成后通知外部刷新列表上的匹配度 */
  onAnalyzed?: (analysis: InterviewJobAnalysis) => void;
}

const AnalysisReport = ({ job, onBack, aiConfigured, onConfigureAi, onAnalyzed }: AnalysisReportProps) => {
  const [analysis, setAnalysis] = useState<InterviewJobAnalysis | null>(null);
  const [analysisLoading, setAnalysisLoading] = useState(false);
  const [analysisChecking, setAnalysisChecking] = useState(false);
  const [messageApi, contextHolder] = message.useMessage();

  const loadAnalysis = useCallback(async (jobId: string) => {
    setAnalysisChecking(true);
    try {
      const result = await invoke<CommandResult<InterviewJobAnalysis>>(
        "analysis_get_by_job_id",
        { jobId },
      );
      if (result.success && result.data) {
        setAnalysis(result.data);
      } else {
        setAnalysis(null);
      }
    } catch {
      setAnalysis(null);
    } finally {
      setAnalysisChecking(false);
    }
  }, []);

  useEffect(() => {
    void loadAnalysis(job.id);
  }, [job.id, loadAnalysis]);

  const handleRunAnalyze = useCallback(async () => {
    if (!aiConfigured) return;
    setAnalysisLoading(true);
    try {
      const result = await invoke<CommandResult<InterviewJobAnalysis>>(
        "job_analyze",
        { jobId: job.id },
      );
      if (!result.success || result.data === null) {
        messageApi.error(commandErrorMessage(result.error, "分析失败"));
        return;
      }
      setAnalysis(result.data);
      onAnalyzed?.(result.data);
      if (result.data.parse_error) {
        messageApi.warning("分析完成，但结果解析不完整");
      } else {
        messageApi.success("分析完成");
      }
    } catch (error: unknown) {
      messageApi.error(
        error instanceof Error ? error.message : "分析失败",
      );
    } finally {
      setAnalysisLoading(false);
    }
  }, [aiConfigured, job.id, messageApi, onAnalyzed]);

  const analyzeButton = (
    <AiFeatureGate configured={aiConfigured} onConfigure={onConfigureAi}>
      <Button
        type="primary"
        icon={<ThunderboltOutlined />}
        loading={analysisLoading}
        onClick={() => void handleRunAnalyze()}
      >
        {analysis ? "重新分析" : "开始分析"}
      </Button>
    </AiFeatureGate>
  );

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: onBack ? "100%" : undefined,
        overflowY: onBack ? "auto" : undefined,
        gap: 16,
      }}
    >
      {contextHolder}

      {/* Header：弹窗形态下标题与关闭都由弹窗负责，这一行整体省掉 */}
      {onBack && (
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexShrink: 0 }}>
          <Space>
            <Button icon={<ArrowLeftOutlined />} onClick={onBack}>返回</Button>
            <Typography.Title level={5} style={{ margin: 0 }}>
              {job.title} - 面试分析报告
            </Typography.Title>
          </Space>
          <Space>{analyzeButton}</Space>
        </div>
      )}

      {/* Job Info Card */}
      <Card size="small" style={{ flexShrink: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
          <Descriptions size="small" column={onBack ? 4 : 3} style={{ flex: 1 }}>
            <Descriptions.Item label="公司">{job.company_name}</Descriptions.Item>
            <Descriptions.Item label="薪资">{job.salary || "-"}</Descriptions.Item>
            <Descriptions.Item label="地点">{job.location || "-"}</Descriptions.Item>
            <Descriptions.Item label="状态">
              {job.is_send_resume ? <Tag color="blue">已投递</Tag> : <Tag>未投递</Tag>}
            </Descriptions.Item>
          </Descriptions>
          {!onBack && analyzeButton}
        </div>
      </Card>

      {/* Analysis Content */}
      {analysisChecking ? (
        <div style={{ textAlign: "center", padding: "120px 0" }}>
          <Spin size="large" />
        </div>
      ) : analysis ? (
        <Tabs
          defaultActiveKey="overview"
          items={[
            {
              key: "overview",
              label: "分析概览",
              children: <OverviewTab a={analysis} />,
            },
            {
              key: "skills",
              label: "技能匹配矩阵",
              children: <SkillMatrixTab a={analysis} />,
            },
            {
              key: "questions",
              label: "面试问题预测",
              children: <InterviewQuestionsTab a={analysis} />,
            },
          ]}
        />
      ) : (
        <div style={{ textAlign: "center", padding: "120px 0" }}>
          <Empty description="暂未进行分析，点击右上角按钮开始" />
        </div>
      )}
    </div>
  );
};

/** 报告里所有条目列表共用的行距，逐条读起来不至于挤在一起 */
const bulletListStyle = { margin: 0, paddingLeft: 20, lineHeight: 1.9 } as const;

const OverviewTab = ({ a }: { a: InterviewJobAnalysis }) => (
  <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
    <Card size="small">
      <Row gutter={20} align="middle" wrap={false}>
        <Col flex="none">
          <Progress
            type="circle"
            percent={a.match_score}
            size={116}
            strokeColor={scoreColor(a.match_score)}
            format={(p) => `${p}`}
          />
        </Col>
        <Col flex="auto">
          <Typography.Text strong style={{ fontSize: 15 }}>匹配度总结</Typography.Text>
          <div style={{ marginTop: 6, lineHeight: 1.9 }}>{a.fit_summary}</div>
          <Typography.Text type="secondary" style={{ display: "block", marginTop: 8, fontSize: 12 }}>
            分析时间：{a.analyzed_at}
          </Typography.Text>
        </Col>
      </Row>
    </Card>

    {/* 优势与风险是对照关系，宽屏下并排看更有信息量 */}
    {(a.strengths.length > 0 || a.risks.length > 0) && (
      <Row gutter={[16, 16]}>
        {a.strengths.length > 0 && (
          <Col xs={24} lg={a.risks.length > 0 ? 12 : 24}>
            <Card
              size="small"
              style={{ height: "100%" }}
              title={<Typography.Text strong style={{ color: "#52c41a" }}>优势项</Typography.Text>}
            >
              <ul style={bulletListStyle}>
                {a.strengths.map((s, i) => <li key={i}>{s}</li>)}
              </ul>
            </Card>
          </Col>
        )}
        {a.risks.length > 0 && (
          <Col xs={24} lg={a.strengths.length > 0 ? 12 : 24}>
            <Card
              size="small"
              style={{ height: "100%" }}
              title={<Typography.Text strong style={{ color: "#ff4d4f" }}>风险项</Typography.Text>}
            >
              <ul style={bulletListStyle}>
                {a.risks.map((r, i) => <li key={i}>{r}</li>)}
              </ul>
            </Card>
          </Col>
        )}
      </Row>
    )}

    {a.questions_to_ask_interviewer.length > 0 && (
      <Card size="small" title={<Typography.Text strong>建议反问面试官</Typography.Text>}>
        <ul style={bulletListStyle}>
          {a.questions_to_ask_interviewer.map((q, i) => <li key={i}>{q}</li>)}
        </ul>
      </Card>
    )}

    {(a.search_summary || a.search_sources.length > 0) && (
      <Card size="small" title={<Typography.Text strong>联网搜索资料</Typography.Text>}>
        {a.search_summary && (
          <div style={{ whiteSpace: "pre-wrap", lineHeight: 1.8 }}>
            {a.search_summary}
          </div>
        )}
        {a.search_sources.length > 0 && (
          <ul style={{ margin: "12px 0 0", paddingLeft: 20 }}>
            {a.search_sources.map((source, i) => (
              <li key={`${source.url}-${i}`}>
                {source.url ? (
                  <a href={source.url} target="_blank" rel="noreferrer">
                    {source.title || source.url}
                  </a>
                ) : (
                  <span>{source.title || source.snippet}</span>
                )}
                {source.snippet && (
                  <Typography.Text type="secondary">
                    {" "}
                    {source.snippet}
                  </Typography.Text>
                )}
              </li>
            ))}
          </ul>
        )}
      </Card>
    )}

    {a.chat_context && (
      <Card size="small" title={<Typography.Text strong>沟通上下文</Typography.Text>}>
        <div style={{ whiteSpace: "pre-wrap", lineHeight: 1.8 }}>
          {a.chat_context}
        </div>
      </Card>
    )}

    {a.parse_error && (
      <Card size="small" title={<Typography.Text strong type="warning">解析错误：{a.parse_error}</Typography.Text>}>
        <div style={{
          padding: 12, background: "#fafafa",
          borderRadius: 8, maxHeight: 300, overflowY: "auto",
          whiteSpace: "pre-wrap", fontSize: 12,
        }}>
          {a.raw_response}
        </div>
      </Card>
    )}
  </div>
);

const detailTextStyle = {
  whiteSpace: "pre-wrap" as const,
  overflowWrap: "anywhere" as const,
  lineHeight: 1.8,
};

const SkillMatrixTab = ({ a }: { a: InterviewJobAnalysis }) => {
  const [selected, setSelected] = useState<SkillEvidence | null>(null);

  return (
    <div>
      {a.skill_matrix.length > 0 ? (
        <>
          <Typography.Text type="secondary" style={{ display: "block", marginBottom: 8 }}>
            点击任意一行查看完整内容
          </Typography.Text>
          <Table
            size="small"
            pagination={false}
            dataSource={a.skill_matrix}
            rowKey={(_, i) => String(i)}
            onRow={(record) => ({
              onClick: () => setSelected(record),
              style: { cursor: "pointer" },
            })}
            columns={[
              { title: "JD 要求", dataIndex: "requirement", ellipsis: true, width: "26%" },
              { title: "简历证据", dataIndex: "resume_evidence", ellipsis: true, width: "28%" },
              { title: "差距", dataIndex: "gap", ellipsis: true, width: "20%" },
              { title: "补强建议", dataIndex: "prep_action", ellipsis: true, width: "26%" },
            ]}
          />
          <Modal
            title="技能匹配详情"
            open={selected !== null}
            onCancel={() => setSelected(null)}
            footer={null}
            width={760}
          >
            {selected && (
              <Descriptions column={1} bordered size="small">
                <Descriptions.Item label="JD 要求"><div style={detailTextStyle}>{selected.requirement}</div></Descriptions.Item>
                <Descriptions.Item label="简历证据"><div style={detailTextStyle}>{selected.resume_evidence}</div></Descriptions.Item>
                <Descriptions.Item label="差距"><div style={detailTextStyle}>{selected.gap}</div></Descriptions.Item>
                <Descriptions.Item label="补强建议"><div style={detailTextStyle}>{selected.prep_action}</div></Descriptions.Item>
              </Descriptions>
            )}
          </Modal>
        </>
      ) : (
        <Empty description="暂无技能匹配数据" />
      )}
    </div>
  );
};

const InterviewQuestionsTab = ({ a }: { a: InterviewJobAnalysis }) => {
  const [selected, setSelected] = useState<InterviewQuestion | null>(null);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      {a.likely_questions.length > 0 ? (
        <>
          <Typography.Text type="secondary">点击问题卡片查看完整内容</Typography.Text>
          {a.likely_questions.map((q, i) => (
            // 问题本身可能很长，放进 title 会被压成一行；这里统一在正文里排版
            <Card key={i} size="small" hoverable onClick={() => setSelected(q)}>
              <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
                <Tag style={{ margin: 0, flexShrink: 0 }}>{q.category}</Tag>
                <Typography.Text strong style={{ lineHeight: 1.7 }}>{q.question}</Typography.Text>
              </div>
              <Typography.Text type="secondary" style={{ display: "block", marginTop: 8 }}>
                提问意图：{q.why}
              </Typography.Text>
              <div style={{ marginTop: 4, lineHeight: 1.9 }}>{q.answer_outline}</div>
            </Card>
          ))}
          <Modal
            title={<Space><Tag>{selected?.category}</Tag><span>面试问题详情</span></Space>}
            open={selected !== null}
            onCancel={() => setSelected(null)}
            footer={null}
            width={760}
          >
            {selected && (
              <Descriptions column={1} bordered size="small">
                <Descriptions.Item label="预测问题"><div style={detailTextStyle}>{selected.question}</div></Descriptions.Item>
                <Descriptions.Item label="提问意图"><div style={detailTextStyle}>{selected.why}</div></Descriptions.Item>
                <Descriptions.Item label="回答思路"><div style={detailTextStyle}>{selected.answer_outline}</div></Descriptions.Item>
              </Descriptions>
            )}
          </Modal>
        </>
      ) : (
        <Empty description="暂无面试问题预测" />
      )}
    </div>
  );
};

export default AnalysisReport;
