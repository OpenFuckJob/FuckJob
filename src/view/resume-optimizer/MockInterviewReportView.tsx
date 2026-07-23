import { useState } from "react";
import { Alert, Button, Card, Col, Modal, Progress, Row, Space, Tag, Typography } from "antd";
import { CheckOutlined, EyeOutlined } from "@ant-design/icons";
import type { MockInterviewReport, MockResumeOptimization } from "@/types/analysis";

interface MockInterviewReportViewProps {
  report: MockInterviewReport;
  applying: boolean;
  onApply: (optimization: MockResumeOptimization) => Promise<void>;
}

const markdownStyle = {
  margin: 0,
  maxHeight: 360,
  overflow: "auto",
  whiteSpace: "pre-wrap" as const,
  overflowWrap: "anywhere" as const,
  fontSize: 12,
  lineHeight: 1.7,
  background: "#f8fafc",
  borderRadius: 8,
  padding: 12,
};

export function MockInterviewReportView({ report, applying, onApply }: MockInterviewReportViewProps) {
  const [selected, setSelected] = useState<MockResumeOptimization | null>(null);
  const applySelected = async () => {
    if (!selected) return;
    try {
      await onApply(selected);
      setSelected(null);
    } catch {
      // The parent presents the actionable error and the comparison remains open.
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <Card size="small">
        <Row gutter={16} align="middle">
          <Col><Progress type="circle" size={76} percent={report.overallScore} /></Col>
          <Col flex={1}>
            <Typography.Text strong>总体评价</Typography.Text>
            <Typography.Paragraph style={{ margin: "6px 0 0", whiteSpace: "pre-wrap" }}>
              {report.overallSummary}
            </Typography.Paragraph>
          </Col>
        </Row>
      </Card>

      <Typography.Text strong>能力维度</Typography.Text>
      <Row gutter={[10, 10]}>
        {report.dimensions.map((item) => (
          <Col xs={24} lg={12} key={item.dimension}>
            <Card size="small" title={<Space><span>{item.dimension}</span><Tag color={item.score >= 80 ? "green" : item.score >= 60 ? "orange" : "red"}>{item.score}</Tag></Space>}>
              {item.strengths.length > 0 && <Typography.Paragraph style={{ marginBottom: 6 }}><b>优势：</b>{item.strengths.join("；")}</Typography.Paragraph>}
              {item.weaknesses.length > 0 && <Typography.Paragraph style={{ marginBottom: 6 }}><b>薄弱点：</b>{item.weaknesses.join("；")}</Typography.Paragraph>}
              {item.evidence.length > 0 && <Typography.Paragraph type="secondary" style={{ marginBottom: 0 }}><b>依据：</b>{item.evidence.join("；")}</Typography.Paragraph>}
            </Card>
          </Col>
        ))}
      </Row>

      {report.risks.length > 0 && <Alert type="warning" showIcon message="需要关注" description={report.risks.join("；")} />}

      <Typography.Text strong>简历优化建议</Typography.Text>
      {report.optimizations.length === 0 ? (
        <Alert type="info" message="本次面试没有生成可安全采纳的简历修改" />
      ) : report.optimizations.map((item) => (
        <Card
          key={`${item.sectionTitle}-${item.rationale}`}
          size="small"
          title={<Space><span>{item.sectionTitle}</span>{item.needsEvidence && <Tag color="orange">需要补充证据</Tag>}</Space>}
          extra={<Button size="small" icon={<EyeOutlined />} onClick={() => setSelected(item)}>查看修改对比</Button>}
        >
          <Typography.Text>{item.rationale}</Typography.Text>
          {item.evidence.length > 0 && <Typography.Paragraph type="secondary" style={{ margin: "6px 0 0" }}>面试依据：{item.evidence.join("；")}</Typography.Paragraph>}
        </Card>
      ))}

      <Modal
        title={selected ? `简历修改对比 · ${selected.sectionTitle}` : "简历修改对比"}
        open={selected !== null}
        onCancel={() => setSelected(null)}
        width={960}
        footer={selected ? [
          <Button key="cancel" onClick={() => setSelected(null)}>暂不采纳</Button>,
          <Button
            key="apply"
            type="primary"
            icon={<CheckOutlined />}
            loading={applying}
            disabled={selected.needsEvidence}
            onClick={() => void applySelected()}
          >
            确认采纳
          </Button>,
        ] : null}
      >
        {selected?.needsEvidence && <Alert type="warning" showIcon message="这项修改缺少事实依据，补充证据后才能采纳" style={{ marginBottom: 12 }} />}
        {selected && (
          <Row gutter={[12, 12]}>
            <Col xs={24} md={12}><Typography.Text strong>修改前</Typography.Text><pre style={{ ...markdownStyle, marginTop: 6 }}>{selected.originalMarkdown}</pre></Col>
            <Col xs={24} md={12}><Typography.Text strong>修改后</Typography.Text><pre style={{ ...markdownStyle, marginTop: 6 }}>{selected.optimizedMarkdown}</pre></Col>
          </Row>
        )}
      </Modal>
    </div>
  );
}
