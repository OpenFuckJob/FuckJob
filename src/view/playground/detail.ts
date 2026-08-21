import type { PlaygroundStage } from "@/types/playground";

/**
 * 各环节的 detail 是 `unknown`：后端还在往里加字段，前端要是照着某一版的结构
 * 写死解析，B 线一改就白屏。这里只做两件事——
 * 1. 从一小撮「大概率存在」的键里捞一句摘要，捞不到就不显示摘要；
 * 2. 剩下的一律按 JSON 结构铺开成键值行。
 * 所有读取都过类型守卫，任何形状的 detail 都不会抛异常。
 */

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function readString(source: unknown, key: string): string | null {
  if (!isRecord(source)) return null;
  const value = source[key];
  return typeof value === "string" && value.trim() ? value : null;
}

export function readNumber(source: unknown, key: string): number | null {
  if (!isRecord(source)) return null;
  const value = source[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function readBoolean(source: unknown, key: string): boolean | null {
  if (!isRecord(source)) return null;
  const value = source[key];
  return typeof value === "boolean" ? value : null;
}

export function readArray(source: unknown, key: string): unknown[] | null {
  if (!isRecord(source)) return null;
  const value = source[key];
  return Array.isArray(value) ? value : null;
}

/** 把任意条目压成一行可读文本：字符串原样，对象优先取 text / content / name */
export function toLine(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (value === null || value === undefined) return "";
  const named = readString(value, "text") ?? readString(value, "content") ?? readString(value, "name")
    ?? readString(value, "message") ?? readString(value, "reason");
  return named ?? JSON.stringify(value);
}

/** 环节 detail 里可能承载「实际要发出去的文本」的键，按可信度排序 */
const TEXT_KEYS = ["text", "reply", "content", "message", "greeting"];

/** 从 detail 里取出可以直接当成聊天气泡发出去的正文 */
export function extractOutgoingText(detail: unknown): string | null {
  if (typeof detail === "string" && detail.trim()) return detail;
  for (const key of TEXT_KEYS) {
    const value = readString(detail, key);
    if (value) return value;
  }
  // 组装环节给的是一串消息，取第一条纯文本兜底
  const items = readArray(detail, "messages") ?? readArray(detail, "items");
  const first = items?.map(toLine).find((line) => line.trim());
  return first ?? null;
}

/** 环节行右侧那句摘要。摘不出来返回 null，宁可留白也不编 */
export function summarizeDetail(stage: PlaygroundStage, detail: unknown): string | null {
  if (detail === null || detail === undefined) return null;
  if (typeof detail === "string") return detail.trim() || null;

  switch (stage) {
    case "regex_filter": {
      const rule = readString(detail, "matched_rule") ?? readString(detail, "rule");
      return rule ? `命中规则 ${rule}` : readString(detail, "reason");
    }
    case "semantic_match": {
      const score = readNumber(detail, "score");
      const verdict = readString(detail, "verdict") ?? readString(detail, "reason");
      if (score !== null) return verdict ? `${score} 分 · ${verdict}` : `${score} 分`;
      return verdict;
    }
    case "greet_decide":
    case "reply_decide": {
      const action = readString(detail, "action") ?? readString(detail, "decision");
      const text = extractOutgoingText(detail);
      if (action && text) return `${action} · ${text}`;
      return action ?? text;
    }
    case "greet_compose": {
      const items = readArray(detail, "messages") ?? readArray(detail, "items");
      return items ? `${items.length} 条消息` : null;
    }
    case "greet_vet":
    case "reply_vet": {
      const issues = readArray(detail, "issues");
      if (issues?.length) return `${issues.length} 处问题：${issues.map(toLine).join("；")}`;
      return extractOutgoingText(detail);
    }
    case "gate":
      return readString(detail, "state") ?? readString(detail, "reason");
    case "route":
      return readString(detail, "route") ?? readString(detail, "kind") ?? readString(detail, "template");
    case "reconcile": {
      const send = readBoolean(detail, "send_resume");
      const reason = readString(detail, "reason") ?? readString(detail, "intent");
      const label = send === null ? null : send ? "投简历" : "不投简历";
      if (label && reason) return `${label} · ${reason}`;
      return label ?? reason;
    }
    default:
      return null;
  }
}

export interface DetailField {
  key: string;
  /** 标量直接展示；数组和对象各自换一种排版 */
  value: string | string[];
  multiline: boolean;
}

const MULTILINE_KEYS = new Set(["prompt", "text", "reply", "content", "detail", "jd", "raw"]);

/** 把 detail 摊成可渲染的字段列表，未知形状退化成一条 raw */
export function toDetailFields(detail: unknown): DetailField[] {
  if (detail === null || detail === undefined) return [];
  if (typeof detail === "string") {
    return detail.trim() ? [{ key: "内容", value: detail, multiline: true }] : [];
  }
  if (!isRecord(detail)) {
    return [{ key: "内容", value: toLine(detail), multiline: false }];
  }
  return Object.entries(detail).flatMap(([key, value]): DetailField[] => {
    if (value === null || value === undefined) return [];
    if (Array.isArray(value)) {
      if (value.length === 0) return [];
      return [{ key, value: value.map(toLine), multiline: false }];
    }
    if (isRecord(value)) {
      return [{ key, value: JSON.stringify(value, null, 2), multiline: true }];
    }
    const text = String(value);
    return [{ key, value: text, multiline: MULTILINE_KEYS.has(key) || text.length > 60 }];
  });
}
