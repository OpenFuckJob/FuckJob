import { useEffect, useMemo, useRef, useState } from "react";
import { Alert, Button, Input, Space, Tag, Typography, message } from "antd";
import { ArrowLeftOutlined, CheckOutlined, RobotOutlined, SendOutlined, ThunderboltOutlined } from "@ant-design/icons";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  streamMockInterviewQuestion,
  streamMockInterviewSummary,
} from "@/lib/mock-interview";
import type {
  MockInterviewChatMessage,
  MockInterviewStreamPayload,
} from "@/types/analysis";

export interface MockInterviewPanelProps {
  resumeContent: string;
  onApply: (sectionTitle: string, optimizedMarkdown: string) => Promise<void>;
  onBack: () => void;
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
const SSE_DATA_ARTIFACT_RE = /(?:^|\r?\n)\s*data:\s*|data:(?=[㐀-鿿A-Za-z0-9{[""])/g;
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
    .filter((m) => m.content.trim())
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

export function MockInterviewPanel({
  resumeContent,
  onApply,
  onBack,
}: MockInterviewPanelProps) {
  const [sessionId, setSessionId] = useState(createSessionId);
  const [messages, setMessages] = useState<UiMessage[]>([]);
  const [answer, setAnswer] = useState("");
  const [round, setRound] = useState(0);
  const [status, setStatus] = useState<InterviewStatus>("idle");
  const [summaryMarkdown, setSummaryMarkdown] = useState("");
  const [applying, setApplying] = useState(false);
  const [started, setStarted] = useState(false);
  const [messageApi, contextHolder] = message.useMessage();

  const messagesRef = useRef<UiMessage[]>([]);
  const sessionIdRef = useRef(sessionId);
  const streamMessageIdRef = useRef<string | null>(null);
  const chatListRef = useRef<HTMLDivElement>(null);

  const busy = status === "streaming_question" || status === "streaming_summary";
  const canAnswer = status === "waiting_answer" && answer.trim().length > 0;
  const currentFocus = round > 0 ? QUESTION_FOCUS_LABELS[Math.min(round, MAX_ROUNDS) - 1] : "";
  const progressText = round > 0
    ? `第 ${Math.min(round, MAX_ROUNDS)}/${MAX_ROUNDS} 轮：${currentFocus}`
    : "面试进行中";

  useEffect(() => {
    messagesRef.current = messages;
    // Auto-scroll to bottom
    if (chatListRef.current) {
      chatListRef.current.scrollTop = chatListRef.current.scrollHeight;
    }
  }, [messages]);

  useEffect(() => {
    sessionIdRef.current = sessionId;
  }, [sessionId]);

  useEffect(() => {
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
  }, [messageApi]);

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
    const nextSessionId = createSessionId();
    sessionIdRef.current = nextSessionId;
    setSessionId(nextSessionId);
    setMessages([]);
    setAnswer("");
    setSummaryMarkdown("");
    setRound(0);
    setStatus("idle");
    setStarted(true);

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

  function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && canAnswer && !busy) {
      e.preventDefault();
      void handleSendAnswer();
    }
  }

  // Entry screen — not yet started
  if (!started) {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 16, alignItems: "center", padding: "24px 0" }}>
        {contextHolder}
        <Alert
          type="info"
          showIcon
          icon={<RobotOutlined />}
          message="通过 5 个不同方面的问题挖掘简历事实，逐轮回答后生成总结和可采纳的优化章节。"
          style={{ width: "100%", maxWidth: 560 }}
        />
        <Button
          type="primary"
          size="large"
          icon={<ThunderboltOutlined />}
          onClick={() => void handleStart()}
          style={{ minWidth: 160 }}
        >
          开始模拟面试
        </Button>
      </div>
    );
  }

  // Chat view
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        minHeight: 0,
        background: "transparent",
      }}
    >
      {contextHolder}

      {/* Header bar */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "10px 16px",
          borderBottom: "1px solid rgba(0,0,0,0.06)",
          background: "rgba(255,255,255,0.8)",
          backdropFilter: "blur(8px)",
          flexShrink: 0,
          borderRadius: "10px 10px 0 0",
        }}
      >
        <Button
          type="text"
          size="small"
          icon={<ArrowLeftOutlined />}
          onClick={onBack}
          style={{ color: "#64748b" }}
        >
          返回
        </Button>
        <div style={{ flex: 1, display: "flex", alignItems: "center", gap: 8 }}>
          <RobotOutlined style={{ color: "#1677ff" }} />
          <Typography.Text strong style={{ fontSize: 14 }}>AI 模拟面试</Typography.Text>
        </div>
        <Space size={6}>
          <Tag color={status === "completed" ? "green" : "blue"}>{progressText}</Tag>
          {busy && <Tag>流式生成中</Tag>}
        </Space>
        {status === "completed" || status === "error" ? (
          <Button
            size="small"
            icon={<ThunderboltOutlined />}
            onClick={() => void handleStart()}
          >
            重新开始
          </Button>
        ) : null}
      </div>

      {/* Message list */}
      <div
        ref={chatListRef}
        style={{
          flex: 1,
          overflowY: "auto",
          padding: "16px 16px 8px",
          display: "flex",
          flexDirection: "column",
          gap: 12,
          minHeight: 0,
        }}
      >
        {messages.length === 0 ? (
          <div style={{ textAlign: "center", color: "#94a3b8", paddingTop: 40, fontSize: 14 }}>
            AI 面试官会逐字流式提问，请耐心等待…
          </div>
        ) : (
          messages.map((item) => {
            const isCandidate = item.role === "candidate";
            const isSystem = item.role === "system";
            return (
              <div
                key={item.id}
                style={{
                  display: "flex",
                  flexDirection: "column",
                  alignItems: isCandidate ? "flex-end" : "flex-start",
                  gap: 4,
                }}
              >
                {!isCandidate && (
                  <Typography.Text type="secondary" style={{ fontSize: 11, paddingLeft: 4 }}>
                    {isSystem ? "系统提示" : "面试官"}
                    {item.streaming && (
                      <span style={{ marginLeft: 6, color: "#1677ff" }}>●</span>
                    )}
                  </Typography.Text>
                )}
                <div
                  style={{
                    maxWidth: "82%",
                    padding: isSystem ? "8px 14px" : "10px 16px",
                    borderRadius: isCandidate ? "16px 4px 16px 16px" : "4px 16px 16px 16px",
                    background: isCandidate
                      ? "linear-gradient(135deg, #1677ff, #0958d9)"
                      : isSystem
                        ? "rgba(100,116,139,0.08)"
                        : "rgba(255,255,255,0.95)",
                    border: isCandidate
                      ? "none"
                      : isSystem
                        ? "1px solid rgba(100,116,139,0.15)"
                        : "1px solid rgba(22,119,255,0.15)",
                    boxShadow: isCandidate
                      ? "0 2px 8px rgba(22,119,255,0.25)"
                      : isSystem
                        ? "none"
                        : "0 1px 4px rgba(0,0,0,0.06)",
                    wordBreak: "break-word",
                    whiteSpace: "pre-wrap",
                  }}
                >
                  <Typography.Text
                    style={{
                      color: isCandidate ? "#fff" : isSystem ? "#64748b" : "#0f172a",
                      fontSize: isSystem ? 12 : 14,
                      lineHeight: 1.65,
                    }}
                  >
                    {item.content || (item.streaming ? "" : "...")}
                    {item.streaming && (
                      <span
                        style={{
                          display: "inline-block",
                          width: 2,
                          height: "1em",
                          background: "#1677ff",
                          marginLeft: 2,
                          verticalAlign: "text-bottom",
                          animation: "cursor-blink 1s step-end infinite",
                        }}
                      />
                    )}
                  </Typography.Text>
                </div>
                {isCandidate && (
                  <Typography.Text type="secondary" style={{ fontSize: 11, paddingRight: 4 }}>
                    我
                  </Typography.Text>
                )}
              </div>
            );
          })
        )}

        {/* Summary section shown inline at bottom of chat */}
        {status === "completed" && summaryMarkdown && (
          <div
            style={{
              marginTop: 8,
              padding: "16px",
              background: "linear-gradient(135deg, rgba(22,119,255,0.04), rgba(22,119,255,0.08))",
              borderRadius: 12,
              border: "1px solid rgba(22,119,255,0.2)",
            }}
          >
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 12 }}>
              <Typography.Text strong style={{ color: "#1677ff", fontSize: 13 }}>
                ✦ 面试总结 & 简历优化建议
              </Typography.Text>
              <Button
                type="primary"
                size="small"
                icon={<CheckOutlined />}
                loading={applying}
                disabled={!summarySection}
                onClick={() => void handleApply()}
              >
                采纳首个章节到简历
              </Button>
            </div>
            <pre
              style={{
                fontFamily: "'SF Mono', 'Menlo', monospace",
                fontSize: 12,
                lineHeight: 1.7,
                color: "#334155",
                background: "transparent",
                border: "none",
                padding: 0,
                margin: 0,
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
              }}
            >
              {summaryMarkdown}
            </pre>
          </div>
        )}
      </div>

      {/* Input area — fixed at bottom, only shown while not completed */}
      {status !== "completed" && (
        <div
          style={{
            padding: "12px 16px",
            borderTop: "1px solid rgba(0,0,0,0.06)",
            background: "rgba(255,255,255,0.9)",
            backdropFilter: "blur(8px)",
            flexShrink: 0,
            borderRadius: "0 0 10px 10px",
          }}
        >
          <div style={{ display: "flex", gap: 10, alignItems: "flex-end" }}>
            <Input.TextArea
              value={answer}
              onChange={(e) => setAnswer(e.target.value)}
              onKeyDown={handleKeyDown}
              rows={3}
              disabled={status !== "waiting_answer"}
              placeholder={
                status === "idle"
                  ? "等待 AI 面试官提问…"
                  : status === "streaming_question" || status === "streaming_summary"
                    ? "AI 正在生成，请稍候…"
                    : status === "error"
                      ? "发生错误，请重新开始"
                      : "回答这一轮追问，尽量补充技术细节、个人贡献、量化数据（⌘/Ctrl+Enter 发送）"
              }
              maxLength={2000}
              showCount
              style={{
                flex: 1,
                resize: "none",
                borderRadius: 10,
                fontSize: 14,
              }}
            />
            <Button
              type="primary"
              icon={<SendOutlined />}
              disabled={!canAnswer}
              loading={busy}
              onClick={() => void handleSendAnswer()}
              style={{
                height: 76,
                width: 48,
                borderRadius: 10,
                flexShrink: 0,
              }}
            />
          </div>
        </div>
      )}

      {/* Blinking cursor CSS */}
      <style>{`
        @keyframes cursor-blink {
          0%, 100% { opacity: 1; }
          50% { opacity: 0; }
        }
      `}</style>
    </div>
  );
}
