import { useCallback, useEffect, useState } from "react";
import {
  Button,
  Card,
  Descriptions,
  Empty,
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
import type { InterviewJobAnalysis } from "../../types/analysis";
import type { JobDetail } from "../../types/job-detail";
import { AiFeatureGate } from "@/components/AiFeatureGate";

const scoreColor = (s: number): string => {
  if (s >= 80) return "#52c41a";
  if (s >= 60) return "#faad14";
  return "#ff4d4f";
};

interface AnalysisReportProps {
  job: JobDetail;
  onBack: () => void;
  aiConfigured: boolean;
  onConfigureAi: () => void;
}

const AnalysisReport = ({ job, onBack, aiConfigured, onConfigureAi }: AnalysisReportProps) => {
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
  }, [aiConfigured, job.id, messageApi]);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", overflowY: "auto", gap: 16 }}>
      {contextHolder}

      {/* Header */}
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexShrink: 0 }}>
        <Space>
          <Button icon={<ArrowLeftOutlined />} onClick={onBack}>返回</Button>
          <Typography.Title level={5} style={{ margin: 0 }}>
            {job.title} - 面试分析报告
          </Typography.Title>
        </Space>
        <Space>
          <AiFeatureGate configured={aiConfigured} onConfigure={onConfigureAi}><Button
            type="primary"
            icon={<ThunderboltOutlined />}
            loading={analysisLoading}
            onClick={() => void handleRunAnalyze()}
          >
            {analysis ? "重新分析" : "开始分析"}
          </Button></AiFeatureGate>
        </Space>
      </div>

      {/* Job Info Card */}
      <Card size="small" style={{ flexShrink: 0 }}>
        <Descriptions size="small" column={4}>
          <Descriptions.Item label="公司">{job.company_name}</Descriptions.Item>
          <Descriptions.Item label="薪资">{job.salary || "-"}</Descriptions.Item>
          <Descriptions.Item label="地点">{job.location || "-"}</Descriptions.Item>
          <Descriptions.Item label="状态">
            {job.is_send_resume ? <Tag color="blue">已投递</Tag> : <Tag>未投递</Tag>}
          </Descriptions.Item>
        </Descriptions>
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

const OverviewTab = ({ a }: { a: InterviewJobAnalysis }) => (
  <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
    <Row gutter={16} align="middle">
      <Col>
        <Progress
          type="circle"
          percent={a.match_score}
          size={100}
          strokeColor={scoreColor(a.match_score)}
          format={(p) => `${p}`}
        />
      </Col>
      <Col flex={1}>
        <Typography.Text strong style={{ fontSize: 15 }}>匹配度总结</Typography.Text>
        <div style={{ marginTop: 4, lineHeight: 1.8 }}>{a.fit_summary}</div>
      </Col>
    </Row>

    {a.strengths.length > 0 && (
      <Card size="small" title={<Typography.Text strong style={{ color: "#52c41a" }}>优势项</Typography.Text>}>
        <ul style={{ margin: 0, paddingLeft: 20 }}>
          {a.strengths.map((s, i) => <li key={i}>{s}</li>)}
        </ul>
      </Card>
    )}

    {a.risks.length > 0 && (
      <Card size="small" title={<Typography.Text strong style={{ color: "#ff4d4f" }}>风险项</Typography.Text>}>
        <ul style={{ margin: 0, paddingLeft: 20 }}>
          {a.risks.map((r, i) => <li key={i}>{r}</li>)}
        </ul>
      </Card>
    )}

    {a.questions_to_ask_interviewer.length > 0 && (
      <Card size="small" title={<Typography.Text strong>建议反问面试官</Typography.Text>}>
        <ul style={{ margin: 0, paddingLeft: 20 }}>
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

const SkillMatrixTab = ({ a }: { a: InterviewJobAnalysis }) => (
  <div>
    {a.skill_matrix.length > 0 ? (
      <Table
        size="small"
        pagination={false}
        dataSource={a.skill_matrix}
        rowKey={(_, i) => String(i)}
        columns={[
          { title: "JD 要求", dataIndex: "requirement", ellipsis: true },
          { title: "简历证据", dataIndex: "resume_evidence", ellipsis: true },
          { title: "差距", dataIndex: "gap", ellipsis: true },
          { title: "补强建议", dataIndex: "prep_action", ellipsis: true },
        ]}
      />
    ) : (
      <Empty description="暂无技能匹配数据" />
    )}
  </div>
);

const InterviewQuestionsTab = ({ a }: { a: InterviewJobAnalysis }) => (
  <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
    {a.likely_questions.length > 0 ? (
      a.likely_questions.map((q, i) => (
        <Card key={i} size="small" title={
          <Space><Tag>{q.category}</Tag>{q.question}</Space>
        }>
          <Typography.Text type="secondary">提问意图：{q.why}</Typography.Text>
          <div style={{ marginTop: 4, lineHeight: 1.8 }}>{q.answer_outline}</div>
        </Card>
      ))
    ) : (
      <Empty description="暂无面试问题预测" />
    )}
  </div>
);

export default AnalysisReport;
