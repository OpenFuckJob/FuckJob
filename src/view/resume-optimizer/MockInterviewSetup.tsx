import { Card, Col, Input, Row, Select, Typography } from "antd";

export interface MockInterviewSettings {
  jobContext: string;
  interviewType: string;
  difficulty: string;
}

interface MockInterviewSetupProps {
  value: MockInterviewSettings;
  onChange: (value: MockInterviewSettings) => void;
  disabled?: boolean;
}

export function MockInterviewSetup({ value, onChange, disabled }: MockInterviewSetupProps) {
  const patch = (next: Partial<MockInterviewSettings>) => onChange({ ...value, ...next });

  return (
    <Card size="small" title="面试设置" style={{ width: "100%" }}>
      <Row gutter={[12, 12]}>
        <Col xs={24} sm={12}>
          <Typography.Text type="secondary">面试类型</Typography.Text>
          <Select
            value={value.interviewType}
            disabled={disabled}
            onChange={(interviewType) => patch({ interviewType })}
            style={{ width: "100%", marginTop: 4 }}
            options={[
              { value: "技术面", label: "技术面" },
              { value: "项目深挖", label: "项目深挖" },
              { value: "综合面", label: "综合面" },
            ]}
          />
        </Col>
        <Col xs={24} sm={12}>
          <Typography.Text type="secondary">难度</Typography.Text>
          <Select
            value={value.difficulty}
            disabled={disabled}
            onChange={(difficulty) => patch({ difficulty })}
            style={{ width: "100%", marginTop: 4 }}
            options={[
              { value: "初级", label: "初级" },
              { value: "中级", label: "中级" },
              { value: "高级", label: "高级" },
            ]}
          />
        </Col>
        <Col span={24}>
          <Typography.Text type="secondary">目标岗位 / JD</Typography.Text>
          <div style={{ marginBottom: 18 }}>
            <Input.TextArea
              value={value.jobContext}
              disabled={disabled}
              onChange={(event) => patch({ jobContext: event.target.value })}
              placeholder="粘贴目标岗位名称和 JD。留空时将基于简历进行通用技术面试。"
              autoSize={{ minRows: 3, maxRows: 7 }}
              maxLength={6000}
              showCount
              style={{ marginTop: 4 }}
            />
          </div>
        </Col>
      </Row>
    </Card>
  );
}
