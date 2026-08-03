import { useEffect, useMemo, useRef, useState } from "react";
import {
  Alert,
  Button,
  Input,
  Modal,
  Progress,
  Tag,
  Typography,
  message,
} from "antd";
import {
  ArrowDownOutlined,
  ArrowLeftOutlined,
  CheckCircleFilled,
  ClockCircleOutlined,
  EyeOutlined,
  PauseCircleOutlined,
  RedoOutlined,
  SendOutlined,
  StopOutlined,
} from "@ant-design/icons";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { streamMockInterviewQuestion } from "@/lib/mock-interview";
import type { MockInterviewChatMessage, MockInterviewStreamPayload } from "@/types/analysis";
import {
  generateInterviewReport,
  getInterviewSession,
  saveInterviewSession,
  subscribeInterviewSessions,
  updateInterviewSession,
} from "./interview-store";
import { DURATION_META, type InterviewMessage, type InterviewSession } from "./interview-types";

export interface MockInterviewPanelProps {
  sessionId: string;
  onBack: () => void;
  onReport: () => void;
}

type QuestionState = "idle" | "generating" | "waiting" | "error";
const SSE_DATA_ARTIFACT_RE = /(?:^|\r?\n)\s*data:\s*|data:(?=[㐀-鿿A-Za-z0-9{[""])/g;

function cleanStreamText(content: string): string {
  return content.replace(SSE_DATA_ARTIFACT_RE, "");
}

function createMessage(
  role: InterviewMessage["role"],
  content: string,
  extra: Partial<InterviewMessage> = {},
): InterviewMessage {
  return {
    id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    role,
    content,
    createdAt: new Date().toISOString(),
    ...extra,
  };
}

function toHistory(messages: InterviewMessage[]): MockInterviewChatMessage[] {
  return messages.filter((item) => item.content.trim()).map(({ role, content }) => ({ role, content }));
}

function overallProgress(session: InterviewSession): number {
  const value = session.modules.reduce((sum, module, index) => {
    if (index < session.currentModuleIndex) return sum + module.weight;
    if (index > session.currentModuleIndex) return sum;
    return sum + module.weight * Math.min(1, module.completedQuestions / Math.max(1, module.targetQuestions));
  }, 0);
  return Math.min(100, Math.round(value));
}

function shouldFollowUp(session: InterviewSession, answer: InterviewMessage): boolean {
  if (answer.skipped || answer.content.length > 260) return false;
  const module = session.modules[session.currentModuleIndex];
  if (!module || module.followUpQuestions >= Math.min(2, module.completedQuestions)) return false;
  const hasEvidence = /\d|%|负责|主导|设计|实现|结果|提升|降低|解决/.test(answer.content);
  return answer.content.length < 130 || !hasEvidence;
}

export function MockInterviewPanel({ sessionId, onBack, onReport }: MockInterviewPanelProps) {
  const [session, setSession] = useState(() => getInterviewSession(sessionId));
  const [questionState, setQuestionState] = useState<QuestionState>("idle");
  const [streamingMessageId, setStreamingMessageId] = useState<string>();
  const [endOpen, setEndOpen] = useState(false);
  const [skipOpen, setSkipOpen] = useState(false);
  const [viewingModuleId, setViewingModuleId] = useState<string>();
  const [showNewMessage, setShowNewMessage] = useState(false);
  const [messageApi, contextHolder] = message.useMessage();
  const listRef = useRef<HTMLDivElement>(null);
  const nearBottomRef = useRef(true);
  const activeRef = useRef(true);
  const streamMessageIdRef = useRef<string | undefined>(undefined);
  const startedRef = useRef(false);

  useEffect(() => {
    activeRef.current = true;
    const unsubscribe = subscribeInterviewSessions(() => setSession(getInterviewSession(sessionId)));
    updateInterviewSession(sessionId, (current) => ({ ...current, status: "in_progress" }));
    return () => {
      activeRef.current = false;
      unsubscribe();
    };
  }, [sessionId]);

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    void listen<MockInterviewStreamPayload>("mock_interview:delta", (event) => {
      if (event.payload.sessionId !== sessionId || event.payload.kind !== "question") return;
      const id = streamMessageIdRef.current;
      if (!id) return;
      const delta = cleanStreamText(event.payload.content);
      updateInterviewSession(sessionId, (current) => ({
        ...current,
        messages: current.messages.map((item) => item.id === id ? { ...item, content: cleanStreamText(item.content + delta) } : item),
      }));
    }).then((unlisten) => unlisteners.push(unlisten));
    return () => unlisteners.forEach((unlisten) => unlisten());
  }, [sessionId]);

  const askNextQuestion = async (source?: InterviewSession, forceCore = false) => {
    let current = source ?? getInterviewSession(sessionId);
    if (!current || questionState === "generating") return;

    let nextModuleIndex = current.currentModuleIndex;
    while (
      nextModuleIndex < current.modules.length &&
      current.modules[nextModuleIndex].completedQuestions >= current.modules[nextModuleIndex].targetQuestions
    ) nextModuleIndex += 1;

    if (nextModuleIndex >= current.modules.length) {
      await finishInterview(current);
      return;
    }
    if (nextModuleIndex !== current.currentModuleIndex) {
      current = saveInterviewSession({ ...current, currentModuleIndex: nextModuleIndex });
    }

    const lastCandidate = [...current.messages].reverse().find((item) => item.role === "candidate");
    const questionKind = !forceCore && lastCandidate && shouldFollowUp(current, lastCandidate) ? "followup" : "core";
    const module = current.modules[current.currentModuleIndex];
    const placeholder = createMessage("interviewer", "", { moduleId: module.id, questionKind });
    streamMessageIdRef.current = placeholder.id;
    setStreamingMessageId(placeholder.id);
    setQuestionState("generating");
    current = saveInterviewSession({ ...current, messages: [...current.messages, placeholder], status: "in_progress" });

    try {
      const content = await streamMockInterviewQuestion({
        sessionId,
        resumeContent: current.resumeSnapshot,
        history: toHistory(current.messages.filter((item) => item.id !== placeholder.id)),
        round: current.mainQuestionCount + current.followUpCount + 1,
        jobContext: current.settings.jobContext,
        interviewType: current.settings.interviewType,
        difficulty: current.settings.difficulty,
        moduleName: module.name,
        moduleDescription: module.description,
        questionKind,
        focusAreas: [...current.settings.focusAreas, current.settings.customFocus].filter(Boolean),
        moduleQuestion: module.completedQuestions + 1,
        moduleTargetQuestions: module.targetQuestions,
      });
      updateInterviewSession(sessionId, (latest) => ({
        ...latest,
        mainQuestionCount: latest.mainQuestionCount + (questionKind === "core" ? 1 : 0),
        followUpCount: latest.followUpCount + (questionKind === "followup" ? 1 : 0),
        modules: latest.modules.map((item, index) => index === latest.currentModuleIndex ? {
          ...item,
          completedQuestions: item.completedQuestions + (questionKind === "core" ? 1 : 0),
          followUpQuestions: item.followUpQuestions + (questionKind === "followup" ? 1 : 0),
        } : item),
        messages: latest.messages.map((item) => item.id === placeholder.id ? { ...item, content: cleanStreamText(content) } : item),
      }));
      if (activeRef.current) setQuestionState("waiting");
    } catch (error) {
      updateInterviewSession(sessionId, (latest) => ({
        ...latest,
        messages: latest.messages.filter((item) => item.id !== placeholder.id),
      }));
      if (activeRef.current) {
        setQuestionState("error");
        messageApi.error(error instanceof Error ? error.message : "问题生成失败");
      }
    } finally {
      streamMessageIdRef.current = undefined;
      if (activeRef.current) setStreamingMessageId(undefined);
    }
  };

  useEffect(() => {
    if (!session || startedRef.current) return;
    startedRef.current = true;
    const last = session.messages[session.messages.length - 1];
    if (!last || last.role === "candidate") void askNextQuestion(session);
    else setQuestionState("waiting");
  }, [session?.id]);

  useEffect(() => {
    if (!session || viewingModuleId) return;
    if (nearBottomRef.current && listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
      setShowNewMessage(false);
    } else if (session.messages.length) {
      setShowNewMessage(true);
    }
  }, [session?.messages, viewingModuleId]);

  const currentModule = session?.modules[session.currentModuleIndex];
  const estimatedRemaining = useMemo(() => {
    if (!session) return "";
    const meta = DURATION_META[session.settings.duration];
    const maxMinutes = Number(meta.minutes.match(/(\d+)(?=分钟)/)?.[1] || 30);
    return `${Math.max(2, Math.ceil(maxMinutes * (1 - overallProgress(session) / 100)))}分钟`;
  }, [session]);

  if (!session) return <Alert type="error" showIcon message="未找到模拟面试记录" action={<Button onClick={onBack}>返回</Button>} />;

  const setDraft = (draft: string) => {
    setSession((current) => current ? { ...current, draft } : current);
    window.setTimeout(() => updateInterviewSession(sessionId, (current) => ({ ...current, draft })), 0);
  };

  const sendAnswer = async (skipped = false) => {
    const current = getInterviewSession(sessionId);
    if (!current || questionState !== "waiting" || (!skipped && !current.draft.trim())) return;
    const candidate = createMessage(
      "candidate",
      skipped ? "本题暂时不会，选择跳过。" : current.draft.trim(),
      { moduleId: current.modules[current.currentModuleIndex]?.id, skipped },
    );
    const next = saveInterviewSession({ ...current, draft: "", messages: [...current.messages, candidate] });
    setQuestionState("idle");
    await askNextQuestion(next);
  };

  const finishInterview = async (source?: InterviewSession) => {
    const current = source ?? getInterviewSession(sessionId);
    if (!current) return;
    saveInterviewSession({
      ...current,
      completedAt: new Date().toISOString(),
      status: "report_queued",
      draft: "",
    });
    setEndOpen(false);
    onReport();
    void generateInterviewReport(sessionId);
  };

  const saveAndExit = () => {
    updateInterviewSession(sessionId, (current) => ({ ...current, status: "paused" }));
    onBack();
  };

  const jumpToModule = (moduleId: string, completed: boolean) => {
    if (!completed && moduleId !== currentModule?.id) return;
    if (moduleId === currentModule?.id) {
      setViewingModuleId(undefined);
      listRef.current?.scrollTo({ top: listRef.current.scrollHeight, behavior: "smooth" });
      return;
    }
    const element = listRef.current?.querySelector(`[data-module-id="${moduleId}"]`);
    if (element) {
      setViewingModuleId(moduleId);
      element.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  };

  const enoughForReport = session.mainQuestionCount >= 5 || (Date.now() - new Date(session.createdAt).getTime()) >= 10 * 60 * 1000;

  return (
    <div className="mi-session-page">
      {contextHolder}
      <header className="mi-session-header">
        <div className="mi-session-header-top">
          <Button type="text" icon={<ArrowLeftOutlined />} onClick={saveAndExit}>保存并退出</Button>
          <div className="mi-session-title">
            <Typography.Text strong>{session.settings.jobTitle || "通用岗位面试"}</Typography.Text>
            <Typography.Text type="secondary">{session.settings.interviewType} · {session.settings.difficulty}</Typography.Text>
          </div>
          <div className="mi-save-status"><CheckCircleFilled /> 已自动保存</div>
        </div>
        <div className="mi-session-header-progress">
          <div>
            <strong>{currentModule?.name || "面试总结"}</strong>
            <span>整体进度 {overallProgress(session)}%</span>
          </div>
          <Progress percent={overallProgress(session)} showInfo={false} />
          <span><ClockCircleOutlined /> 预计剩余{estimatedRemaining}</span>
          <Button icon={<StopOutlined />} onClick={() => setEndOpen(true)}>提前结束面试</Button>
        </div>
      </header>

      <div className="mi-session-body">
        <aside className="mi-outline">
          <Typography.Text strong>面试大纲</Typography.Text>
          <div className="mi-outline-list">
            {session.modules.map((module, index) => {
              const completed = index < session.currentModuleIndex || module.completedQuestions >= module.targetQuestions;
              const active = index === session.currentModuleIndex;
              return (
                <button
                  type="button"
                  key={module.id}
                  className={`mi-outline-item ${completed ? "is-completed" : ""} ${active ? "is-active" : ""} ${viewingModuleId === module.id ? "is-viewing" : ""}`}
                  onClick={() => jumpToModule(module.id, completed)}
                >
                  <span className="mi-outline-marker">{completed ? <CheckCircleFilled /> : index + 1}</span>
                  <span><strong>{module.name}</strong><small>核心题 {module.completedQuestions}/{module.targetQuestions}</small></span>
                </button>
              );
            })}
          </div>
          <div className="mi-outline-summary">
            <span>{session.mainQuestionCount}个核心问题</span>
            <span>{session.followUpCount}次动态追问</span>
          </div>
        </aside>

        <main className="mi-conversation">
          <div
            className="mi-message-list"
            ref={listRef}
            onScroll={(event) => {
              const element = event.currentTarget;
              nearBottomRef.current = element.scrollHeight - element.scrollTop - element.clientHeight < 80;
              if (nearBottomRef.current) setShowNewMessage(false);
            }}
          >
            <div className="mi-interview-intro">
              <Typography.Text strong>模拟面试已开始</Typography.Text>
              <Typography.Text type="secondary">面试官会根据你的回答动态追问，过程中不会展示评分与参考答案。</Typography.Text>
            </div>
            {session.messages.map((item) => {
              const module = session.modules.find((entry) => entry.id === item.moduleId);
              if (item.role === "system") return <div key={item.id} className="mi-system-line">{item.content}</div>;
              const candidate = item.role === "candidate";
              return (
                <article
                  key={item.id}
                  data-module-id={!candidate ? item.moduleId : undefined}
                  className={`mi-message ${candidate ? "is-candidate" : "is-interviewer"}`}
                >
                  <div className="mi-message-meta">
                    <span>{candidate ? "我" : "面试官"}</span>
                    {!candidate && <Tag>{item.questionKind === "followup" ? "深入追问" : module?.name || "核心问题"}</Tag>}
                    {item.skipped && <Tag color="gold">已跳过</Tag>}
                  </div>
                  <div className="mi-message-content">
                    {item.content || (streamingMessageId === item.id ? "面试官正在结合你的回答组织问题…" : "")}
                    {streamingMessageId === item.id && <span className="mi-stream-cursor" />}
                  </div>
                </article>
              );
            })}
            {questionState === "error" && (
              <Alert
                type="error"
                showIcon
                message="本题生成失败"
                description="面试记录已保存，可以重新生成当前问题。"
                action={<Button icon={<RedoOutlined />} onClick={() => void askNextQuestion(undefined, true)}>重新生成</Button>}
              />
            )}
          </div>

          {showNewMessage && (
            <Button
              className="mi-new-message"
              icon={<ArrowDownOutlined />}
              onClick={() => {
                listRef.current?.scrollTo({ top: listRef.current.scrollHeight, behavior: "smooth" });
                setShowNewMessage(false);
              }}
            >
              返回最新消息
            </Button>
          )}

          <div className="mi-composer">
            {viewingModuleId ? (
              <div className="mi-history-viewing">
                <EyeOutlined />
                <span>正在查看“{session.modules.find((item) => item.id === viewingModuleId)?.name}”的历史内容</span>
                <Button type="primary" onClick={() => jumpToModule(currentModule?.id || "", true)}>返回当前问题</Button>
              </div>
            ) : (
              <>
                <Input.TextArea
                  value={session.draft}
                  autoSize={{ minRows: 3, maxRows: 8 }}
                  maxLength={3000}
                  disabled={questionState !== "waiting"}
                  placeholder={questionState === "generating" ? "面试官正在结合你的回答生成问题…" : "输入你的回答，Enter换行，Ctrl/⌘ + Enter发送"}
                  onChange={(event) => setDraft(event.target.value)}
                  onKeyDown={(event) => {
                    if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
                      event.preventDefault();
                      void sendAnswer();
                    }
                  }}
                />
                <div className="mi-composer-actions">
                  <Button type="text" icon={<PauseCircleOutlined />} disabled={questionState !== "waiting"} onClick={() => setSkipOpen(true)}>暂时不会</Button>
                  <Typography.Text type="secondary">已输入 {session.draft.length} 字</Typography.Text>
                  <Button type="primary" icon={<SendOutlined />} disabled={questionState !== "waiting" || !session.draft.trim()} onClick={() => void sendAnswer()}>发送回答</Button>
                </div>
              </>
            )}
          </div>
        </main>
      </div>

      <Modal
        title="提前结束面试？"
        open={endOpen}
        onCancel={() => setEndOpen(false)}
        footer={enoughForReport ? [
          <Button key="continue" onClick={() => setEndOpen(false)}>继续面试</Button>,
          <Button key="finish" type="primary" onClick={() => void finishInterview()}>结束并生成报告</Button>,
        ] : [
          <Button key="continue" onClick={() => setEndOpen(false)}>继续面试</Button>,
          <Button key="save" onClick={saveAndExit}>保存为未完成并退出</Button>,
        ]}
      >
        {enoughForReport
          ? <Typography.Paragraph>报告将根据目前完成的{session.mainQuestionCount}个核心问题生成，未覆盖能力会标记为“本次未充分考察”。</Typography.Paragraph>
          : <Alert type="warning" showIcon message="当前回答较少，暂时不足以形成有效报告" description="至少完成5个核心问题或进行10分钟后，才能生成正式报告。" />}
      </Modal>

      <Modal
        title="确定跳过这个问题？"
        open={skipOpen}
        okText="确认跳过"
        cancelText="继续回答"
        onCancel={() => setSkipOpen(false)}
        onOk={() => {
          setSkipOpen(false);
          void sendAnswer(true);
        }}
      >
        <Typography.Paragraph>报告中会记录该能力点未充分回答，并继续进入后续问题。</Typography.Paragraph>
      </Modal>
    </div>
  );
}
