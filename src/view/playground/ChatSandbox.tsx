import { useEffect, useRef, useState } from "react";
import { Button, Input, Radio, Tooltip } from "antd";
import { ClearOutlined, RobotOutlined, SendOutlined } from "@ant-design/icons";
import type { PlaygroundMessage } from "@/types/playground";

type Identity = "hr" | "me";

export interface ChatSandboxProps {
  messages: PlaygroundMessage[];
  onAppend: (message: PlaygroundMessage) => void;
  onClear: () => void;
  onAskAi: () => void;
  running: boolean;
}

/**
 * 聊天沙盘。
 *
 * 身份切换不是为了好看：闸门有一条「对方尚未回复，不重复发送」的分支，
 * 只有当对话的最后一条是我方消息时才会走到。旧调试页固定以 HR 身份造消息，
 * 那条分支在界面上根本构造不出来。
 */
export function ChatSandbox({ messages, onAppend, onClear, onAskAi, running }: ChatSandboxProps) {
  const [identity, setIdentity] = useState<Identity>("hr");
  const [draft, setDraft] = useState("");
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" });
  }, [messages.length]);

  const send = () => {
    const text = draft.trim();
    if (!text) return;
    onAppend({ text, received: identity === "hr" });
    setDraft("");
  };

  const lastIsMine = messages.length > 0 && !messages[messages.length - 1].received;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-xs font-semibold text-slate-500">聊天沙盘</span>
        <Button
          type="text"
          size="small"
          icon={<ClearOutlined />}
          disabled={messages.length === 0}
          onClick={onClear}
        >
          清空对话
        </Button>
      </div>

      <div className="min-h-[140px] flex-1 overflow-y-auto rounded-xl border border-slate-200 bg-slate-50/70 p-3">
        {messages.length === 0 ? (
          <div className="flex h-full items-center justify-center text-xs text-slate-400">
            以 HR 身份造几条消息，再点「让 AI 回复」
          </div>
        ) : (
          messages.map((message, index) => (
            <div
              key={index}
              className={`mb-2 flex ${message.received ? "justify-start" : "justify-end"}`}
            >
              <div
                data-testid={message.received ? "bubble-hr" : "bubble-me"}
                className={`max-w-[78%] rounded-xl border px-3 py-1.5 text-xs leading-6 whitespace-pre-wrap wrap-break-word ${
                  message.received
                    ? "border-slate-200 bg-white text-slate-800"
                    : "border-sky-200 bg-sky-50 text-slate-800"
                }`}
              >
                <div className="text-[10px] text-slate-400">{message.received ? "HR" : "我"}</div>
                {message.text}
              </div>
            </div>
          ))
        )}
        <div ref={bottomRef} />
      </div>

      <div className="mt-2 flex items-center gap-2">
        <Radio.Group
          aria-label="发送身份"
          size="small"
          optionType="button"
          buttonStyle="solid"
          value={identity}
          onChange={(event) => setIdentity(event.target.value as Identity)}
          options={[
            { value: "hr", label: "HR" },
            { value: "me", label: "我" },
          ]}
        />
        <Input
          aria-label="消息内容"
          placeholder={identity === "hr" ? "HR 会说什么…" : "我方回过去的话…"}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onPressEnter={send}
        />
        <Button icon={<SendOutlined />} disabled={!draft.trim()} onClick={send}>
          发送
        </Button>
        <Tooltip title={lastIsMine ? "最后一条是我方消息，闸门大概率会拦下——这正是要测的分支" : ""}>
          <Button
            type="primary"
            icon={<RobotOutlined />}
            loading={running}
            disabled={messages.length === 0}
            onClick={onAskAi}
          >
            让 AI 回复
          </Button>
        </Tooltip>
      </div>
    </div>
  );
}
