import { useEffect, useMemo, useState } from "react";
import { Empty, Modal, Spin, Tag } from "antd";
import { invoke } from "@tauri-apps/api/core";
import type { CommandResult } from "../../types/command";
import type { ChatMessageRecord, JobDetail } from "../../types/job-detail";

/** 平台回执，不是双方说的话，居中弱化显示而不是塞进某一侧的气泡 */
const SYSTEM_MESSAGE_PREFIXES = [
  "简历文件：",
  "简历文件:",
  "对方已查看",
  "对方已同意",
  "您的附件简历",
  "对方拒绝",
  "对方已拒绝",
];

const isSystemMessage = (text: string): boolean => {
  const normalized = text.trim();
  return SYSTEM_MESSAGE_PREFIXES.some((prefix) => normalized.startsWith(prefix));
};

const formatDay = (time: number): string => {
  const date = new Date(time);
  const today = new Date();
  const sameYear = date.getFullYear() === today.getFullYear();
  const label = date.toLocaleDateString("zh-CN", {
    ...(sameYear ? {} : { year: "numeric" }),
    month: "long",
    day: "numeric",
  });
  const dayDiff = Math.round(
    (new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime() -
      new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()) /
      86400000,
  );
  if (dayDiff === 0) return `今天 ${label}`;
  if (dayDiff === 1) return `昨天 ${label}`;
  return label;
};

const formatClock = (time: number): string =>
  new Date(time).toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
  });

type ThreadItem =
  | { kind: "day"; key: string; label: string }
  | { kind: "system"; key: string; text: string }
  | { kind: "message"; key: string; message: ChatMessageRecord };

/// 按时间铺开消息，跨天时插入日期分隔条。日期只在变化时出现，
/// 同一天连着聊几十条不会被分隔条切碎
function buildThread(messages: ChatMessageRecord[]): ThreadItem[] {
  const items: ThreadItem[] = [];
  let lastDay = "";

  for (const message of messages) {
    const day = new Date(message.time).toDateString();
    if (day !== lastDay) {
      lastDay = day;
      items.push({ kind: "day", key: `day-${day}`, label: formatDay(message.time) });
    }
    items.push(
      isSystemMessage(message.text)
        ? { kind: "system", key: message.id, text: message.text.trim() }
        : { kind: "message", key: message.id, message },
    );
  }
  return items;
}

/** 对方头像取姓名首字；姓名缺失时退回一个中性占位 */
const avatarText = (name: string): string => name.trim().charAt(0) || "对";

const ChatBubble = ({ message }: { message: ChatMessageRecord }) => {
  const mine = !message.received;
  return (
    <div className={`chat-row ${mine ? "is-mine" : "is-peer"}`}>
      <div className="chat-avatar" title={mine ? "我" : message.from_name}>
        {mine ? "我" : avatarText(message.from_name)}
      </div>
      <div className="chat-bubble-wrap">
        <div className="chat-sender">
          {mine ? "我" : message.from_name || "对方"}
          <span className="chat-time">{formatClock(message.time)}</span>
        </div>
        <div className="chat-bubble">{message.text}</div>
      </div>
    </div>
  );
};

const ChatThreadModal = ({
  job,
  open,
  onClose,
}: {
  job: JobDetail;
  open: boolean;
  onClose: () => void;
}) => {
  const [messages, setMessages] = useState<ChatMessageRecord[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open) return;
    setLoading(true);
    invoke<CommandResult<ChatMessageRecord[]>>("chat_messages_by_job", {
      jobId: job.id,
    })
      .then((result) => {
        if (result.success && result.data) {
          setMessages([...result.data].sort((a, b) => a.time - b.time));
        } else {
          setMessages([]);
        }
      })
      .catch(() => setMessages([]))
      .finally(() => setLoading(false));
  }, [job.id, open]);

  const thread = useMemo(() => buildThread(messages), [messages]);
  const peerCount = messages.filter((message) => message.received).length;
  const peerName =
    messages.find((message) => message.received)?.from_name.trim() || "对方";

  return (
    <Modal
      title={`${job.title} - 沟通记录`}
      open={open}
      onCancel={onClose}
      footer={null}
      width={680}
      styles={{ body: { padding: "12px 20px 20px" } }}
    >
      {loading ? (
        <div className="chat-placeholder">
          <Spin />
        </div>
      ) : messages.length === 0 ? (
        <div className="chat-placeholder">
          <Empty description="暂无沟通记录" image={Empty.PRESENTED_IMAGE_SIMPLE} />
        </div>
      ) : (
        <>
          {/* 左右分栏靠气泡位置区分，图例把这个约定说明白 */}
          <div className="chat-legend">
            <Tag color="default">左：{peerName}（{peerCount}）</Tag>
            <Tag color="blue">右：我（{messages.length - peerCount}）</Tag>
            <span className="chat-legend-company">{job.company_name}</span>
          </div>
          <div className="chat-thread">
            {thread.map((item) =>
              item.kind === "day" ? (
                <div className="chat-day" key={item.key}>
                  <span>{item.label}</span>
                </div>
              ) : item.kind === "system" ? (
                <div className="chat-system" key={item.key}>
                  {item.text}
                </div>
              ) : (
                <ChatBubble key={item.key} message={item.message} />
              ),
            )}
          </div>
        </>
      )}
    </Modal>
  );
};

export default ChatThreadModal;
export { buildThread, isSystemMessage };
