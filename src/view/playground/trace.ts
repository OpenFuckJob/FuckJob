import type { AgentTrace, RoundTrace } from "@/types/playground";

export interface ReworkSplit {
  /** 与上一轮完全相同的前缀 */
  shared: string;
  /** 本轮新追加的返工说明 */
  appended: string;
}

/**
 * 返工是往上一轮提示词的尾部追加一段说明，前面的内容原样保留。
 * 所以做前缀比对就够了，不必上真正的 diff 算法——真上了反而会把
 * 「同一段话在两处出现」这种巧合标成改动，把人往错的方向带。
 *
 * 前缀对不上（后端换了拼装方式）时整段算新内容，宁可多标也别漏标。
 */
export function splitRework(previous: string, current: string): ReworkSplit {
  if (!previous) return { shared: "", appended: current };
  let index = 0;
  const max = Math.min(previous.length, current.length);
  while (index < max && previous[index] === current[index]) index += 1;
  if (index < previous.length) return { shared: "", appended: current };
  return { shared: current.slice(0, index), appended: current.slice(index) };
}

/** 模型输出多数是 JSON，能解析就格式化后展示，解析不动就原样交还 */
export function parseRoundOutput(raw: string): { pretty: string; parsed: boolean } {
  const trimmed = raw.trim();
  if (!trimmed) return { pretty: "", parsed: false };
  try {
    return { pretty: JSON.stringify(JSON.parse(trimmed) as unknown, null, 2), parsed: true };
  } catch {
    return { pretty: raw, parsed: false };
  }
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

export function traceSucceeded(trace: AgentTrace): boolean {
  return trace.error === null && trace.stop !== null;
}

export function totalTokens(round: RoundTrace): number | null {
  const usage = round.usage;
  if (!usage) return null;
  if (usage.total_tokens !== null) return usage.total_tokens;
  const prompt = usage.prompt_tokens ?? 0;
  const completion = usage.completion_tokens ?? 0;
  return prompt + completion || null;
}

export const STOP_LABELS: Record<string, string> = {
  first_try: "一次过",
  recovered: "返工后通过",
};
