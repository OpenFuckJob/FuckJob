import { useEffect, useMemo, useRef, useState } from "react";
import { Alert, Button, Drawer, Input, Space, Tag, Typography, message } from "antd";
import { CheckOutlined, RobotOutlined, SendOutlined, ThunderboltOutlined } from "@ant-design/icons";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  streamMockInterviewQuestion,
  streamMockInterviewSummary,
} from "@/lib/mock-interview";
import type {
  MockInterviewChatMessage,
  MockInterviewStreamPayload,
} from "@/types/analysis";
import { AiFeatureGate } from "@/components/AiFeatureGate";

interface MockInterviewDrawerProps {
  open: boolean;
  resumeContent: string;
  onClose: () => void;
  onApply: (sectionTitle: string, optimizedMarkdown: string) => Promise<void>;
  aiConfigured?: boolean;
  onConfigureAi?: () => void;
}

type InterviewStatus =
  | "idle"
  | "streaming_question"
  | "waiting_answer"
  | "streaming_summary"
  | "completed"
  | "error";

interface UiMessage extends MockInterviewChatMessage {
  id: string;
  streaming?: boolean;
}

const MAX_ROUNDS = 5;
const SSE_DATA_ARTIFACT_RE = /(?:^|\r?\n)\s*data:\s*|data:(?=[\u3400-\u9fffA-Za-z0-9{["“])/g;
const QUESTION_FOCUS_LABELS = ["技术深度", "个人贡献", "量化结果", "问题处理", "表达可信度"];

function createSessionId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function createMessage(
  role: UiMessage["role"],
  content: string,
  streaming = false,
): UiMessage {
  return {
    id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    role,
    content,
    streaming,
  };
}

function cleanStreamText(content: string): string {
  return content.replace(SSE_DATA_ARTIFACT_RE, "");
}

function toHistory(messages: UiMessage[]): MockInterviewChatMessage[] {
  return messages
    .filter((message) => message.content.trim())
    .map(({ role, content }) => ({ role, content: cleanStreamText(content) }));
}

function extractFirstMarkdownSection(markdown: string): {
  title: string;
  content: string;
} | null {
  const sectionMatches = [...markdown.matchAll(/^##\s+(.+)$/gm)];
  const ignoredTitles = new Set(["面试总结", "可补充到简历的事实点", "优化后的简历章节"]);
  const match = sectionMatches.find((item) => !ignoredTitles.has(item[1].trim()));
  if (!match || match.index === undefined) return null;

  const start = match.index;
  const nextMatch = sectionMatches.find((item) => (item.index ?? 0) > start);
  const end = nextMatch?.index ?? markdown.length;
  return {
    title: match[1].trim(),
    content: markdown.slice(start, end).trim(),
  };
}

export function MockInterviewDrawer({
  open,
  resumeContent,
  onClose,
  onApply,
  aiConfigured = true,
  onConfigureAi = () => {},
}: MockInterviewDrawerProps) {
  const [sessionId, setSessionId] = useState(createSessionId);
  const [messages, setMessages] = useState<UiMessage[]>([]);
  const [answer, setAnswer] = useState("");
  const [round, setRound] = useState(0);
  const [status, setStatus] = useState<InterviewStatus>("idle");
  const [summaryMarkdown, setSummaryMarkdown] = useState("");
  const [applying, setApplying] = useState(false);
  const [messageApi, contextHolder] = message.useMessage();
  const messagesRef = useRef<UiMessage[]>([]);
  const sessionIdRef = useRef(sessionId);
  const streamMessageIdRef = useRef<string | null>(null);

  const hasResume = resumeContent.trim().length > 0;
  const busy = status === "streaming_question" || status === "streaming_summary";
  const canAnswer = status === "waiting_answer" && answer.trim().length > 0;
  const currentFocus = round > 0 ? QUESTION_FOCUS_LABELS[Math.min(round, MAX_ROUNDS) - 1] : "";
  const progressText = round > 0
    ? `第 ${Math.min(round, MAX_ROUNDS)}/${MAX_ROUNDS} 轮：${currentFocus}`
    : "未开始";

  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);

  useEffect(() => {
    sessionIdRef.current = sessionId;
  }, [sessionId]);

  useEffect(() => {
    if (!open) return;

    const unlisteners: UnlistenFn[] = [];
    void listen<MockInterviewStreamPayload>("mock_interview:delta", (event) => {
      if (event.payload.sessionId !== sessionIdRef.current) return;
      appendStreamDelta(event.payload.content);
    }).then((unlisten) => unlisteners.push(unlisten));

    void listen<MockInterviewStreamPayload>("mock_interview:done", (event) => {
      if (event.payload.sessionId !== sessionIdRef.current) return;
      completeStreamMessage(event.payload.kind, event.payload.content);
    }).then((unlisten) => unlisteners.push(unlisten));

    void listen<MockInterviewStreamPayload>("mock_interview:error", (event) => {
      if (event.payload.sessionId !== sessionIdRef.current) return;
      setStatus("error");
      messageApi.error(event.payload.content || "流式生成失败");
    }).then((unlisten) => unlisteners.push(unlisten));

    return () => {
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [messageApi, open]);

  const summarySection = useMemo(
    () => extractFirstMarkdownSection(summaryMarkdown),
    [summaryMarkdown],
  );

  function appendStreamDelta(delta: string): void {
    const messageId = streamMessageIdRef.current;
    if (!messageId) return;
    const cleanDelta = cleanStreamText(delta);
    if (!cleanDelta) return;

    setMessages((current) =>
      current.map((item) =>
        item.id === messageId
          ? { ...item, content: cleanStreamText(`${item.content}${cleanDelta}`) }
          : item,
      ),
    );
  }

  function completeStreamMessage(
    kind: MockInterviewStreamPayload["kind"],
    content: string,
  ): void {
    const messageId = streamMessageIdRef.current;
    streamMessageIdRef.current = null;
    const cleanContent = cleanStreamText(content);

    setMessages((current) =>
      current.map((item) =>
        item.id === messageId
          ? { ...item, content: cleanContent || cleanStreamText(item.content), streaming: false }
          : item,
      ),
    );

    if (kind === "summary") {
      setSummaryMarkdown(cleanContent);
      setStatus("completed");
      return;
    }

    setStatus("waiting_answer");
  }

  async function startQuestionStream(
    nextRound: number,
    history: UiMessage[],
    activeSessionId = sessionIdRef.current,
  ): Promise<void> {
    const streamingMessage = createMessage("interviewer", "", true);
    streamMessageIdRef.current = streamingMessage.id;
    setMessages((current) => [...current, streamingMessage]);
    setRound(nextRound);
    setStatus("streaming_question");

    try {
      await streamMockInterviewQuestion({
        sessionId: activeSessionId,
        resumeContent: resumeContent.trim(),
        history: toHistory(history),
        round: nextRound,
        maxRounds: MAX_ROUNDS,
      });
    } catch (error: unknown) {
      streamMessageIdRef.current = null;
      setStatus("error");
      const detail = error instanceof Error ? error.message : "生成问题失败";
      messageApi.error(detail);
    }
  }

  async function handleStart(): Promise<void> {
    if (!aiConfigured) { onConfigureAi(); return; }
    if (!hasResume) {
      messageApi.warning("请先输入/导入简历内容");
      return;
    }

    const nextSessionId = createSessionId();
    sessionIdRef.current = nextSessionId;
    setSessionId(nextSessionId);
    setMessages([]);
    setAnswer("");
    setSummaryMarkdown("");
    setRound(0);
    setStatus("idle");

    const intro = createMessage(
      "system",
      "模拟面试开始。AI 面试官会围绕技术深度、个人贡献、量化结果、问题处理、表达可信度各问 1 轮，全部回答后生成总结和可采纳的简历优化章节。",
    );
    setMessages([intro]);

    setTimeout(() => {
      void startQuestionStream(1, [intro], nextSessionId);
    }, 0);
  }

  async function handleSendAnswer(): Promise<void> {
    if (!aiConfigured) { onConfigureAi(); return; }
    const trimmed = answer.trim();
    if (!trimmed) {
      messageApi.warning("请输入您的真实回答");
      return;
    }

    const candidateMessage = createMessage("candidate", trimmed);
    const nextMessages = [...messagesRef.current, candidateMessage];
    setMessages(nextMessages);
    setAnswer("");

    if (round >= MAX_ROUNDS) {
      await handleGenerateSummary(nextMessages);
      return;
    }

    await startQuestionStream(round + 1, nextMessages);
  }

  async function handleGenerateSummary(history = messagesRef.current): Promise<void> {
    if (!aiConfigured) { onConfigureAi(); return; }
    const streamingMessage = createMessage("interviewer", "", true);
    streamMessageIdRef.current = streamingMessage.id;
    setMessages((current) => [...current, streamingMessage]);
    setStatus("streaming_summary");

    try {
      await streamMockInterviewSummary({
        sessionId: sessionIdRef.current,
        resumeContent: resumeContent.trim(),
        history: toHistory(history),
      });
    } catch (error: unknown) {
      streamMessageIdRef.current = null;
      setStatus("error");
      const detail = error instanceof Error ? error.message : "生成总结失败";
      messageApi.error(detail);
    }
  }

  async function handleApply(): Promise<void> {
    if (!summarySection) {
      messageApi.warning("总结中没有可采纳的 Markdown 二级章节");
      return;
    }

    setApplying(true);
    try {
      await onApply(summarySection.title, summarySection.content);
      messageApi.success("已采纳并更新简历");
    } catch (error: unknown) {
      const detail = error instanceof Error ? error.message : "更新简历失败";
      messageApi.error(detail);
    } finally {
      setApplying(false);
    }
  }

  return (
    <Drawer
      title="模拟面试优化"
      placement="right"
      width={640}
      open={open}
      onClose={onClose}
      className="mock-interview-drawer"
    >
      {contextHolder}
      <AiFeatureGate configured={aiConfigured} onConfigure={onConfigureAi}><></></AiFeatureGate>
      <div className="mock-interview-content">
        <Alert
          type="info"
          showIcon
          icon={<RobotOutlined />}
          message="通过 5 个不同方面的问题挖掘简历事实，用户逐轮回答后生成总结和可采纳的优化章节。"
        />

        <div className="mock-chat-toolbar">
          <Space>
            <Tag color={status === "completed" ? "green" : "blue"}>{progressText}</Tag>
            <Tag>{busy ? "流式生成中" : "等待操作"}</Tag>
          </Space>
          <Button
            type="primary"
            icon={<ThunderboltOutlined />}
            loading={busy && messages.length <= 1}
            disabled={!hasResume || busy || !aiConfigured}
            onClick={() => void handleStart()}
          >
            开始模拟面试
          </Button>
        </div>

        {!hasResume && (
          <Alert type="warning" showIcon message="请先输入/导入简历内容" />
        )}

        <div className="mock-chat-list">
          {messages.length === 0 ? (
            <div className="mock-chat-empty">点击开始后，AI 面试官会逐字流式提问。</div>
          ) : (
            messages.map((item) => (
              <div key={item.id} className={`mock-chat-message ${item.role}`}>
                <div className="mock-chat-role">
                  {item.role === "candidate" ? "我" : item.role === "system" ? "系统" : "面试官"}
                  {item.streaming && <span className="mock-streaming-dot">streaming</span>}
                </div>
                <div className="mock-chat-bubble">
                  <Typography.Text>{item.content || "..."}</Typography.Text>
                </div>
              </div>
            ))
          )}
        </div>

        {status !== "completed" && (
          <div className="mock-answer-panel">
            <Input.TextArea
              value={answer}
              onChange={(event) => setAnswer(event.target.value)}
              rows={5}
              disabled={status !== "waiting_answer"}
              placeholder="回答这一轮追问，尽量补充技术细节、个人贡献、量化数据和结果"
              showCount
              maxLength={2000}
            />
            <Button
              type="primary"
              icon={<SendOutlined />}
              disabled={!canAnswer}
              loading={busy}
              onClick={() => void handleSendAnswer()}
            >
              发送回答
            </Button>
          </div>
        )}

        {status === "completed" && (
          <div className="mock-optimized-preview">
            <div className="mock-optimized-header">
              <Typography.Text strong>最终总结与优化章节</Typography.Text>
              <Button
                type="primary"
                icon={<CheckOutlined />}
                loading={applying}
                disabled={!summarySection}
                onClick={() => void handleApply()}
              >
                采纳首个章节
              </Button>
            </div>
            <pre>{summaryMarkdown}</pre>
          </div>
        )}
      </div>
    </Drawer>
  );
}
