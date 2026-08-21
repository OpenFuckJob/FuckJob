import { useEffect, useRef, useState } from "react";
import { Button, Input, Radio } from "antd";
import { SendOutlined } from "@ant-design/icons";
import type { PlaygroundMessage } from "@/types/playground";

type Identity = "hr" | "me";

export interface ChatSandboxProps {
  messages: PlaygroundMessage[];
  onAppend: (message: PlaygroundMessage) => void;
}

/**
 * 聊天沙盘：只负责造对话，「让 AI 回复」在页面底部的行动条上。
 *
 * 身份切换不是为了好看：闸门有一条「对方尚未回复，不重复发送」的分支，
 * 只有当对话的最后一条是我方消息时才会走到。旧调试页固定以 HR 身份造消息，
 * 那条分支在界面上根本构造不出来
 */
export function ChatSandbox({ messages, onAppend }: ChatSandboxProps) {
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

  return (
    <div>
      <div className="h-55 overflow-y-auto rounded-xl border border-slate-200 bg-slate-50/70 p-3">
        {messages.length === 0 ? (
          <div className="flex h-full items-center justify-center text-xs text-slate-400">
            还没有消息。先以 HR 身份说一句，链路才有得判
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
      </div>
    </div>
  );
}
