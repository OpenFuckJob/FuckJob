import { useMemo, useState } from "react";
import {
  Button,
  Card,
  Empty,
  Progress,
  Segmented,
  Space,
  Tabs,
  Tag,
  Typography,
} from "antd";
import { ArrowRightOutlined, CheckCircleFilled, MessageOutlined, RiseOutlined } from "@ant-design/icons";
import type { MockInterviewQuestionReview, MockInterviewReport } from "@/types/analysis";
import type { InterviewSession } from "./interview-types";

interface MockInterviewReportViewProps {
  report: MockInterviewReport;
  session: InterviewSession;
  comparisonSessions: InterviewSession[];
  initialTab?: "summary" | "abilities" | "questions" | "transcript";
  onPracticeQuestion: (review: MockInterviewQuestionReview) => void;
  onRestart: () => void;
}

function scoreLabel(score: number): string {
  if (score >= 85) return "表现突出";
  if (score >= 75) return "表现良好";
  if (score >= 60) return "基本达到要求";
  return "需要重点提升";
}

function scoreColor(score: number): string {
  if (score >= 80) return "#16a34a";
  if (score >= 60) return "#d97706";
  return "#dc2626";
}

export function MockInterviewReportView(props: MockInterviewReportViewProps) {
  const [questionFilter, setQuestionFilter] = useState("all");
  const [selectedQuestion, setSelectedQuestion] = useState(0);
  const dimensions = [...props.report.dimensions].sort((left, right) => right.score - left.score);
  const strongest = dimensions[0];
  const weakest = dimensions[dimensions.length - 1];
  const questionReviews = props.report.questionReviews ?? [];
  const filteredQuestions = useMemo(() => questionReviews.filter((item) => {
    if (questionFilter === "strong") return item.score >= 80;
    if (questionFilter === "improve") return item.score < 80;
    if (questionFilter === "skipped") return !item.answer.trim() || /跳过|不会/.test(item.answer);
    return true;
  }), [questionFilter, questionReviews]);
  const activeQuestion = filteredQuestions[Math.min(selectedQuestion, Math.max(0, filteredQuestions.length - 1))];

  const summary = (
    <div className="mi-report-stack">
      <Card className="mi-report-hero">
        <div className="mi-report-score">
          <Progress
            type="circle"
            size={104}
            percent={props.report.overallScore}
            strokeColor={scoreColor(props.report.overallScore)}
            format={(value) => <span><strong>{value}</strong><small>综合得分</small></span>}
          />
        </div>
        <div className="mi-report-conclusion">
          <Tag color={props.report.overallScore >= 75 ? "success" : "warning"}>{scoreLabel(props.report.overallScore)}</Tag>
          <Typography.Title level={4}>本次面试总体评价</Typography.Title>
          <Typography.Paragraph>{props.report.overallSummary}</Typography.Paragraph>
          <div className="mi-report-highlights">
            <div><span>最强能力</span><strong>{strongest?.dimension || "--"}</strong></div>
            <div><span>优先提升</span><strong>{weakest?.dimension || "--"}</strong></div>
            <div><span>有效覆盖</span><strong>{props.session.modules.filter((item) => item.completedQuestions > 0).length}/{props.session.modules.length} 模块</strong></div>
          </div>
        </div>
      </Card>

      <div className="mi-report-columns">
        <Card title="突出表现" className="mi-report-positive">
          {(strongest?.strengths.length ? strongest.strengths : ["本次报告暂无明确优势项"]).map((item) => (
            <div className="mi-report-list-item" key={item}><CheckCircleFilled /> <span>{item}</span></div>
          ))}
        </Card>
        <Card title="优先改进" className="mi-report-improve">
          {(weakest?.weaknesses.length ? weakest.weaknesses : props.report.risks).slice(0, 4).map((item) => (
            <div className="mi-report-list-item" key={item}><ArrowRightOutlined /> <span>{item}</span></div>
          ))}
        </Card>
      </div>

      <Card title={<Space><RiseOutlined />下一步建议</Space>}>
        <div className="mi-next-actions">
          <Button type="primary" onClick={props.onRestart}>使用相同岗位重新面试</Button>
          {questionReviews.length > 0 && <Button onClick={() => props.onPracticeQuestion(questionReviews.find((item) => item.score === Math.min(...questionReviews.map((entry) => entry.score))) || questionReviews[0])}>重新练习薄弱问题</Button>}
        </div>
      </Card>

      {props.comparisonSessions.length > 1 && (
        <Card title="同岗位最近表现">
          <div className="mi-score-trend">
            {props.comparisonSessions.slice(0, 3).reverse().map((session) => (
              <div key={session.id}>
                <span>{new Date(session.createdAt).toLocaleDateString("zh-CN")}</span>
                <strong>{session.report?.overallScore ?? "--"}</strong>
                <Progress percent={session.report?.overallScore ?? 0} showInfo={false} />
              </div>
            ))}
          </div>
        </Card>
      )}
    </div>
  );

  const abilities = (
    <div className="mi-ability-list">
      {props.report.dimensions.map((item) => (
        <Card key={item.dimension} className="mi-ability-card">
          <div className="mi-ability-heading">
            <div><Typography.Title level={5}>{item.dimension}</Typography.Title><Typography.Text type="secondary">{scoreLabel(item.score)}</Typography.Text></div>
            <strong style={{ color: scoreColor(item.score) }}>{item.score}</strong>
          </div>
          <Progress percent={item.score} showInfo={false} strokeColor={scoreColor(item.score)} />
          <div className="mi-ability-details">
            {!!item.strengths.length && <div><b>做得好的部分</b><ul>{item.strengths.map((value) => <li key={value}>{value}</li>)}</ul></div>}
            {!!item.weaknesses.length && <div><b>可以改进</b><ul>{item.weaknesses.map((value) => <li key={value}>{value}</li>)}</ul></div>}
            {!!item.evidence.length && <div className="mi-evidence"><b>判断依据</b>{item.evidence.map((value) => <p key={value}>{value}</p>)}</div>}
          </div>
        </Card>
      ))}
    </div>
  );

  const questions = questionReviews.length ? (
    <div className="mi-question-review">
      <aside>
        <Segmented
          block
          value={questionFilter}
          onChange={(value) => { setQuestionFilter(String(value)); setSelectedQuestion(0); }}
          options={[
            { label: "全部", value: "all" },
            { label: "较好", value: "strong" },
            { label: "待提升", value: "improve" },
            { label: "跳过", value: "skipped" },
          ]}
        />
        <div className="mi-question-list">
          {filteredQuestions.map((item, index) => (
            <button type="button" key={`${item.questionIndex}-${item.question}`} className={index === selectedQuestion ? "is-active" : ""} onClick={() => setSelectedQuestion(index)}>
              <span>问题 {item.questionIndex}</span>
              <strong>{item.question}</strong>
              <Tag color={item.score >= 80 ? "success" : item.score >= 60 ? "warning" : "error"}>{item.score}</Tag>
            </button>
          ))}
        </div>
      </aside>
      <main>
        {activeQuestion ? (
          <div className="mi-question-detail">
            <Tag>{activeQuestion.module}</Tag>
            <Typography.Title level={4}>{activeQuestion.question}</Typography.Title>
            <section><Typography.Text type="secondary">你的回答</Typography.Text><p>{activeQuestion.answer || "本题未作答"}</p></section>
            <section><Typography.Text type="secondary">本题评价</Typography.Text><p>{activeQuestion.summary}</p></section>
            <div className="mi-report-columns">
              <div><b>做得好的部分</b><ul>{activeQuestion.strengths.map((item) => <li key={item}>{item}</li>)}</ul></div>
              <div><b>可以改进</b><ul>{activeQuestion.improvements.map((item) => <li key={item}>{item}</li>)}</ul></div>
            </div>
            <section className="mi-answer-outline"><b>建议回答结构</b><ol>{activeQuestion.answerOutline.map((item) => <li key={item}>{item}</li>)}</ol></section>
            <Button type="primary" onClick={() => props.onPracticeQuestion(activeQuestion)}>重新练习这道题</Button>
          </div>
        ) : <Empty description="当前筛选下没有问题" />}
      </main>
    </div>
  ) : <Empty description="当前模型未返回逐题复盘，可在能力评估中查看本次反馈" />;

  const transcript = (
    <div className="mi-transcript">
      {props.session.modules.map((module) => {
        const messages = props.session.messages.filter((item) => item.moduleId === module.id);
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
  );

  return (
    <Tabs
      className="mi-report-tabs"
      defaultActiveKey={props.initialTab || "summary"}
      items={[
        { key: "summary", label: "总体结论", children: summary },
        { key: "abilities", label: "能力评估", children: abilities },
        { key: "questions", label: `逐题复盘${questionReviews.length ? ` (${questionReviews.length})` : ""}`, children: questions },
        { key: "transcript", label: <span><MessageOutlined /> 完整对话</span>, children: transcript },
      ]}
    />
  );
}
