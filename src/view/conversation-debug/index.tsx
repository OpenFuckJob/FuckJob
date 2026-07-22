import { useCallback, useRef, useState } from "react";
import {
  Button,
  Divider,
  Input,
  message,
  Space,
  Spin,
  Typography,
} from "antd";
import { SendOutlined, UserOutlined } from "@ant-design/icons";
import { invoke } from "@tauri-apps/api/core";
import type { CommandResult } from "../../types/command";
import { commandErrorMessage } from "../../types/command";
import { AiFeatureGate } from "@/components/AiFeatureGate";

const { TextArea } = Input;

interface JobInput {
  job_title: string;
  company_name: string;
  job_detail: string;
  salary: string;
  location: string;
}

interface DebugChatMessage {
  text: string;
  from_name: string;
  received: boolean;
}

interface ChatBubble {
  role: "user" | "hr" | "assistant";
  content: string;
}

const emptyJob: JobInput = {
  job_title: "",
  company_name: "",
  job_detail: "",
  salary: "",
  location: "",
};

const ConversationDebugPage = ({ aiConfigured, onConfigureAi }: { aiConfigured: boolean; onConfigureAi: () => void }) => {
  const [job, setJob] = useState<JobInput>(emptyJob);
  const [bubbles, setBubbles] = useState<ChatBubble[]>([]);
  const [inputText, setInputText] = useState("");
  const [generating, setGenerating] = useState(false);
  const [messageApi, contextHolder] = message.useMessage();
  const chatEndRef = useRef<HTMLDivElement>(null);

  const scrollToBottom = useCallback(() => {
    setTimeout(() => {
      chatEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }, 100);
  }, []);

  const updateJob = useCallback(
    (field: keyof JobInput, value: string) => {
      setJob((prev) => ({ ...prev, [field]: value }));
    },
    [],
  );

  const sendUserMessage = useCallback(async () => {
    if (!aiConfigured) return;
    const text = inputText.trim();
    if (!text || generating) return;
    const newBubbles: ChatBubble[] = [
      ...bubbles,
      { role: "hr", content: text },
    ];
    setBubbles(newBubbles);
    setInputText("");
    scrollToBottom();

    setGenerating(true);
    try {
      const messages: DebugChatMessage[] = newBubbles
        .filter((b) => b.role === "hr" || b.role === "assistant")
        .map((b) => ({
          text: b.content,
          from_name: b.role === "hr" ? "HR" : "我",
          received: b.role === "hr",
        }));

      const result = await invoke<CommandResult<string>>(
        "debug_generate_replay",
        { req: { ...job, messages } },
      );

      if (!result.success || result.data === null) {
        messageApi.error(commandErrorMessage(result.error, "生成失败"));
        return;
      }

      setBubbles((prev) => [
        ...prev,
        { role: "assistant", content: result.data! },
      ]);
      scrollToBottom();
    } catch (error: unknown) {
      messageApi.error(
        error instanceof Error ? error.message : "生成回复失败",
      );
    } finally {
      setGenerating(false);
    }
  }, [aiConfigured, inputText, bubbles, job, generating, messageApi, scrollToBottom]);

  const generateGreet = useCallback(async () => {
    if (!aiConfigured) return;
    if (!job.job_title.trim()) {
      messageApi.warning("请填写岗位名称");
      return;
    }
    setGenerating(true);
    try {
      const result = await invoke<CommandResult<string>>(
        "debug_generate_greet",
        { req: job },
      );
      if (!result.success || result.data === null) {
        messageApi.error(commandErrorMessage(result.error, "生成失败"));
        return;
      }
      setBubbles((prev) => [
        ...prev,
        { role: "assistant", content: result.data! },
      ]);
      scrollToBottom();
    } catch (error: unknown) {
      messageApi.error(
        error instanceof Error ? error.message : "生成打招呼内容失败",
      );
    } finally {
      setGenerating(false);
    }
  }, [aiConfigured, job, messageApi, scrollToBottom]);

  const clearChat = useCallback(() => {
    setBubbles([]);
  }, []);

  return (
    <div style={{ display: "flex", height: "100%", gap: 0 }}>
      {contextHolder}
      <AiFeatureGate configured={aiConfigured} onConfigure={onConfigureAi}><></></AiFeatureGate>

      {/* 左侧：岗位信息输入 */}
      <div
        style={{
          width: "33.3%",
          minWidth: 280,
          display: "flex",
          flexDirection: "column",
          gap: 12,
          padding: "0 16px 0 0",
          overflowY: "auto",
        }}
      >
        <Typography.Title level={5} style={{ margin: 0 }}>
          岗位信息
        </Typography.Title>

        <div>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            岗位名称
          </Typography.Text>
          <Input
            placeholder="如：高级前端工程师"
            value={job.job_title}
            onChange={(e) => updateJob("job_title", e.target.value)}
          />
        </div>

        <div>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            公司名称
          </Typography.Text>
          <Input
            placeholder="如：字节跳动"
            value={job.company_name}
            onChange={(e) => updateJob("company_name", e.target.value)}
          />
        </div>

        <div>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            薪资范围
          </Typography.Text>
          <Input
            placeholder="如：25-50K"
            value={job.salary}
            onChange={(e) => updateJob("salary", e.target.value)}
          />
        </div>

        <div>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            工作地点
          </Typography.Text>
          <Input
            placeholder="如：北京·朝阳区"
            value={job.location}
            onChange={(e) => updateJob("location", e.target.value)}
          />
        </div>

        <div style={{ flex: 1, display: "flex", flexDirection: "column" }}>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            岗位描述（JD）
          </Typography.Text>
          <TextArea
            placeholder="粘贴完整的岗位 JD..."
            value={job.job_detail}
            onChange={(e) => updateJob("job_detail", e.target.value)}
            style={{ flex: 1, minHeight: 180, resize: "none" }}
          />
        </div>

        <Button
          type="primary"
          onClick={generateGreet}
          loading={generating}
          block
        >
          生成打招呼内容
        </Button>
      </div>

      <Divider type="vertical" style={{ height: "auto", margin: "0 4px" }} />

      {/* 右侧：对话窗口 */}
      <div
        style={{
          flex: 1,
          display: "flex",
          flexDirection: "column",
          minWidth: 0,
        }}
      >
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            marginBottom: 12,
          }}
        >
          <Typography.Title level={5} style={{ margin: 0 }}>
            对话调试
          </Typography.Title>
          <Space>
            <Button onClick={clearChat} disabled={bubbles.length === 0}>
              清空对话
            </Button>
          </Space>
        </div>

        {/* 消息列表 */}
        <div
          style={{
            flex: 1,
            overflowY: "auto",
            padding: 12,
            background: "#fafafa",
            borderRadius: 12,
            border: "1px solid #f0f0f0",
          }}
        >
          {bubbles.length === 0 && (
            <div
              style={{
                height: "100%",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                color: "#bfbfbf",
              }}
            >
              在下方输入 HR 发来的消息，开始调试
            </div>
          )}

          {bubbles.map((bubble, index) => (
            <div
              key={index}
              style={{
                display: "flex",
                justifyContent:
                  bubble.role === "hr" ? "flex-start" : "flex-end",
                marginBottom: 10,
              }}
            >
              <div
                style={{
                  maxWidth: "75%",
                  padding: "8px 14px",
                  borderRadius: 12,
                  fontSize: 13,
                  lineHeight: 1.7,
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                  background:
                    bubble.role === "hr"
                      ? "#ffffff"
                      : bubble.role === "assistant"
                        ? "#e6f4ff"
                        : "#f0f0f0",
                  border:
                    bubble.role === "hr"
                      ? "1px solid #f0f0f0"
                      : bubble.role === "assistant"
                        ? "1px solid #91caff"
                        : "1px solid #d9d9d9",
                }}
              >
                <div
                  style={{
                    fontSize: 11,
                    color: "#999",
                    marginBottom: 2,
                  }}
                >
                  {bubble.role === "hr"
                    ? "HR"
                    : bubble.role === "assistant"
                      ? "AI 回复"
                      : "我"}
                </div>
                {bubble.content}
              </div>
            </div>
          ))}

          {generating && (
            <div
              style={{
                display: "flex",
                justifyContent: "flex-end",
                marginBottom: 10,
              }}
            >
              <div
                style={{
                  padding: "8px 14px",
                  borderRadius: 12,
                  background: "#e6f4ff",
                  border: "1px solid #91caff",
                }}
              >
                <Spin size="small" />
                <span style={{ marginLeft: 8, fontSize: 13, color: "#666" }}>
                  正在生成...
                </span>
              </div>
            </div>
          )}

          <div ref={chatEndRef} />
        </div>

        {/* 输入区 */}
        <div
          style={{
            display: "flex",
            gap: 8,
            marginTop: 12,
          }}
        >
          <Input
            placeholder="输入 HR 发来的消息..."
            value={inputText}
            onChange={(e) => setInputText(e.target.value)}
            onPressEnter={sendUserMessage}
            prefix={<UserOutlined style={{ color: "#bfbfbf" }} />}
            size="large"
          />
          <Button
            type="primary"
            icon={<SendOutlined />}
            onClick={sendUserMessage}
            disabled={!inputText.trim()}
            size="large"
          />
        </div>
      </div>
    </div>
  );
};

export default ConversationDebugPage;
