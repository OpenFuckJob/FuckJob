import { Alert, Button, Card, Space, Typography } from "antd";
import { ArrowLeftOutlined, CheckCircleFilled, RocketOutlined } from "@ant-design/icons";
import { MockInterviewSetup } from "./MockInterviewSetup";
import {
  DURATION_META,
  createInterviewModules,
  type MockInterviewSettings,
} from "./interview-types";

interface MockInterviewSetupPageProps {
  value: MockInterviewSettings;
  resumeReady: boolean;
  aiReady: boolean;
  llmConfigured: boolean;
  /** 是否由岗位管理带着岗位跳转过来 */
  fromJob?: boolean;
  onChange: (settings: MockInterviewSettings) => void;
  onBack: () => void;
  onStart: () => void;
  onConfigureAi: () => void;
}
export function MockInterviewSetupPage(props: MockInterviewSetupPageProps) {
  const modules = createInterviewModules(props.value);
  const canStart = props.aiReady && props.resumeReady && !!props.value.jobTitle.trim() && !!props.value.jobContext.trim();
  const duration = DURATION_META[props.value.duration];
  const aiHint = props.llmConfigured ? "大模型已停用" : "尚未配置AI模型";

  return (
    <div className="mi-page mi-setup-page">
      <div className="mi-subpage-header">
        <Button type="text" icon={<ArrowLeftOutlined />} onClick={props.onBack}>返回</Button>
        <div>
          <Typography.Title level={3}>创建模拟面试</Typography.Title>
          <Typography.Text type="secondary">
            {props.fromJob && props.value.jobTitle
              ? `已带入「${props.value.jobTitle}」的岗位信息，选好参数即可开始`
              : "选择目标岗位并设置本次面试重点"}
          </Typography.Text>
        </div>
      </div>

      {!props.aiReady && (
        <Alert
          type="warning"
          showIcon
          message={aiHint}
          description={props.llmConfigured ? "配置会保留，启用后才能开始模拟面试。" : "完成模型配置后才能开始模拟面试。"}
          action={<Button type="primary" size="small" onClick={props.onConfigureAi}>{props.llmConfigured ? "去启用" : "去配置"}</Button>}
        />
      )}
      {!props.resumeReady && (
        <Alert type="info" showIcon message="简历内容为空" description="请先在配置中心完善简历，模拟面试会保存开始时的简历快照。" />
      )}

      <div className="mi-setup-layout">
        <MockInterviewSetup value={props.value} onChange={props.onChange} />
        <aside className="mi-plan-aside">
          <Card className="mi-plan-card">
            <div className="mi-plan-scroll">
              <Typography.Title level={5}>本次面试计划</Typography.Title>
              <div className="mi-plan-job">
                <Typography.Text strong>{props.value.jobTitle || "请先选择目标岗位"}</Typography.Text>
                <Typography.Text type="secondary">{props.value.companyName || "未指定公司"}</Typography.Text>
              </div>
              <Space wrap size={[6, 6]}>
                <span className="mi-plan-pill">{props.value.interviewType}</span>
                <span className="mi-plan-pill">{props.value.difficulty}</span>
                <span className="mi-plan-pill">{duration.label}</span>
              </Space>

              <div className="mi-plan-stat-grid">
                <div><strong>{duration.targetQuestions}</strong><span>预计核心问题</span></div>
                <div><strong>{duration.minutes}</strong><span>预计时长</span></div>
              </div>

              {/* 用题数而不是权重百分比：开始面试前真正想知道的是每块会问几题 */}
              <Typography.Text className="mi-field-label">考察范围</Typography.Text>
              <div className="mi-plan-modules">
                {modules.map((module) => (
                  <span key={module.id} className="mi-plan-module-chip" title={module.description}>
                    {module.name}
                    <em>{module.targetQuestions} 题</em>
                  </span>
                ))}
              </div>

              {!!props.value.focusAreas.length && (
                <div className="mi-plan-focus">
                  <Typography.Text type="secondary">重点考察</Typography.Text>
                  <Typography.Text>{props.value.focusAreas.join("、")}</Typography.Text>
                </div>
              )}

              <div className="mi-readiness-list">
                <span className={props.aiReady ? "is-ready" : ""}><CheckCircleFilled /> AI模型</span>
                <span className={props.resumeReady ? "is-ready" : ""}><CheckCircleFilled /> 简历快照</span>
                <span className={props.value.jobContext.trim() ? "is-ready" : ""}><CheckCircleFilled /> 岗位信息</span>
              </div>
            </div>

            {/* 表单再长，开始按钮也始终停在卡片底部 */}
            <div className="mi-plan-actions">
              <Button
                block
                type="primary"
                size="large"
                icon={<RocketOutlined />}
                disabled={!canStart}
                onClick={props.onStart}
              >
                开始模拟面试
              </Button>
              {!props.value.jobContext.trim() && <Typography.Text type="secondary" className="mi-plan-hint">请选择岗位或填写完整的自定义岗位信息</Typography.Text>}
            </div>
          </Card>
        </aside>
      </div>
    </div>
  );
}
