import type { PlatformKind } from "./rpa";

/**
 * 一个会话被挂起等待人工的原因。
 *
 * 只收「消息已经被读掉、但一个字都没回出去」的情况。模型主动判定无需回复
 * 不在此列——那是正常决策，混进来只会让列表被日常寒暄淹没。
 */
export type ManualReviewReason =
  | "risk_keyword"
  | "vet_rejected"
  | "missing_job_id"
  | "throttle_exhausted";

export interface ManualReviewRecord {
  /** 复合主键：{platform}:{conversation_id} */
  id: string;
  platform: PlatformKind;
  conversation_id: string;
  job_id: string;
  job_name: string;
  company_name: string;
  reason: ManualReviewReason;
  /** 面向用户的一句话说明 */
  detail: string;
  /** 对方最后一条消息的摘要 */
  last_message: string;
  /** 毫秒时间戳 */
  created_at: number;
  updated_at: number;
  /** 累计触发次数 */
  hit_count: number;
}

export const MANUAL_REVIEW_REASON_LABELS: Record<ManualReviewReason, string> = {
  risk_keyword: "涉及敏感话题",
  vet_rejected: "回复未通过体检",
  missing_job_id: "会话标识缺失",
  throttle_exhausted: "自动回复额度用尽",
};

/**
 * 标签配色按「要不要你亲自过目」分层，而不是按原因逐个配色：
 * 敏感话题可能是诈骗，必须人看；额度用尽只是聊得久了该接手；
 * 另外两种是技术原因，回头补一句就行。
 */
export const MANUAL_REVIEW_REASON_COLORS: Record<ManualReviewReason, string> = {
  risk_keyword: "red",
  throttle_exhausted: "orange",
  vet_rejected: "gold",
  missing_job_id: "default",
};

/**
 * 相对时间。待办列表关心的是「多久没管了」，绝对时间戳要用户自己做减法。
 *
 * 未来时刻按「刚刚」处理：两端时钟不一致时显示「-3 分钟前」比不精确更糟。
 */
export function formatRelativeTime(timestampMs: number, now = Date.now()): string {
  const seconds = Math.floor((now - timestampMs) / 1000);
  if (seconds < 60) return "刚刚";

  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} 分钟前`;

  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;

  const days = Math.floor(hours / 24);
  if (days < 30) return `${days} 天前`;

  return new Date(timestampMs).toLocaleDateString("zh-CN");
}
