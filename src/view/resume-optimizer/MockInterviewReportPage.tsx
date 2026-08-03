import { useEffect, useState } from "react";
import { Alert, Button, Result, Space, Spin, Steps, Tag, Typography } from "antd";
import { ArrowLeftOutlined, FileSearchOutlined, ReloadOutlined } from "@ant-design/icons";
import type { MockInterviewQuestionReview } from "@/types/analysis";
import {
  generateInterviewReport,
  getInterviewSession,
  subscribeInterviewSessions,
} from "./interview-store";
import type { InterviewSession } from "./interview-types";
import { MockInterviewReportView } from "./MockInterviewReportView";

interface MockInterviewReportPageProps {
  sessionId: string;
  sessions: InterviewSession[];
  initialTab?: "summary" | "abilities" | "questions" | "transcript";
  onBack: () => void;
  onRestart: () => void;
  onPracticeQuestion: (review: MockInterviewQuestionReview) => void;
}

export function MockInterviewReportPage(props: MockInterviewReportPageProps) {
  const [session, setSession] = useState(() => getInterviewSession(props.sessionId));
  useEffect(() => subscribeInterviewSessions(() => setSession(getInterviewSession(props.sessionId))), [props.sessionId]);

  if (!session) return <Result status="404" title="未找到面试记录" extra={<Button onClick={props.onBack}>返回历史记录</Button>} />;

  const comparisonSessions = props.sessions.filter((item) =>
    item.settings.jobTitle === session.settings.jobTitle && item.status === "report_completed" && item.report,
  );

  if (props.initialTab === "transcript" && !session.report) {
    return (
      <div className="mi-page mi-report-page">
        <div className="mi-report-page-header">
          <Button type="text" icon={<ArrowLeftOutlined />} onClick={props.onBack}>返回历史记录</Button>
          <div className="mi-report-title">
            <Typography.Title level={3}>完整对话</Typography.Title>
            <Typography.Text type="secondary">{session.settings.jobTitle || "通用岗位面试"} · {session.settings.companyName || "未指定公司"}</Typography.Text>
          </div>
          {(session.status === "paused" || session.status === "in_progress") && <Tag color="gold">未完成</Tag>}
        </div>
        <div className="mi-transcript">
          {session.modules.map((module) => {
            const messages = session.messages.filter((item) => item.moduleId === module.id);
            if (!messages.length) return null;
            return (
              <section key={module.id}>
                <Typography.Title level={5}>{module.name}</Typography.Title>
                {messages.map((item) => (
                  <div key={item.id} className={`mi-transcript-message ${item.role === "candidate" ? "is-candidate" : ""}`}>
                    <b>{item.role === "candidate" ? "我" : "面试官"}</b>
                    <p>{item.content}</p>
                  </div>
                ))}
              </section>
            );
          })}
        </div>
      </div>
    );
  }

  if (session.status === "report_failed") {
    return (
      <div className="mi-page mi-report-state-page">
        <Button type="text" icon={<ArrowLeftOutlined />} onClick={props.onBack}>返回历史记录</Button>
        <Result
          status="error"
          title="报告生成失败"
          subTitle="面试记录和所有回答均已安全保存，可以重新生成报告，无需重新面试。"
          extra={[
            <Button key="retry" type="primary" icon={<ReloadOutlined />} onClick={() => void generateInterviewReport(session.id)}>重新生成报告</Button>,
            <Button key="transcript" icon={<FileSearchOutlined />} onClick={props.onBack}>返回模拟面试</Button>,
          ]}
        >
          {session.reportError && <Alert type="warning" message={session.reportError} />}
        </Result>
      </div>
    );
  }

  if (session.status !== "report_completed" || !session.report) {
    return (
      <div className="mi-page mi-report-state-page">
        <Button type="text" icon={<ArrowLeftOutlined />} onClick={props.onBack}>返回模拟面试首页</Button>
        <div className="mi-report-generating">
          <Spin size="large" />
          <Typography.Title level={3}>面试已完成，正在生成报告</Typography.Title>
          <Typography.Paragraph type="secondary">正在分析{session.mainQuestionCount}个核心问题和{session.followUpCount}次动态追问</Typography.Paragraph>
          <Steps
            direction="vertical"
            current={1}
            items={[
              { title: "面试记录已保存", status: "finish" },
              { title: "分析岗位匹配与回答表现", description: "当前正在处理", status: "process" },
              { title: "生成能力评价与改进建议", status: "wait" },
            ]}
          />
          <Alert type="info" showIcon message="你可以离开此页面" description="报告会继续生成，完成后可在历史记录中查看。" />
          <Space>
            <Button onClick={props.onBack}>返回模拟面试首页</Button>
          </Space>
        </div>
      </div>
    );
  }

  return (
    <div className="mi-page mi-report-page">
      <div className="mi-report-page-header">
        <Button type="text" icon={<ArrowLeftOutlined />} onClick={props.onBack}>返回历史记录</Button>
        <div className="mi-report-title">
          <Typography.Title level={3}>{session.settings.jobTitle || "模拟面试报告"}</Typography.Title>
          <Space wrap>
            <Typography.Text type="secondary">{session.settings.companyName || "未指定公司"}</Typography.Text>
            <Tag>{session.settings.interviewType}</Tag>
            <Tag>{session.settings.difficulty}</Tag>
            <Typography.Text type="secondary">{new Date(session.createdAt).toLocaleString("zh-CN")}</Typography.Text>
          </Space>
        </div>
        <Button type="primary" onClick={props.onRestart}>使用相同配置重新面试</Button>
      </div>
      <MockInterviewReportView
        report={session.report}
        session={session}
        comparisonSessions={comparisonSessions}
        initialTab={props.initialTab}
        onPracticeQuestion={props.onPracticeQuestion}
        onRestart={props.onRestart}
      />
    </div>
  );
}
