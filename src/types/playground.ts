/** 链路上的一个环节 */
export type PlaygroundStage =
  | "regex_filter"    // ① 正则与关键词筛选
  | "semantic_match"  // ② 岗位语义复核
  | "greet_decide"    // ③ 打招呼决策
  | "greet_compose"   // ④ 打招呼序列组装
  | "greet_vet"       // ⑤ 打招呼发送前体检
  | "gate"            // ⑥ 回复闸门
  | "route"           // ⑦ 回复路由
  | "reply_decide"    // ⑧ 自动回复决策
  | "reconcile"       // ⑨ 投递意图校正
  | "reply_vet";      // ⑩ 回复发送前体检

export type PlaygroundOutcome =
  | { kind: "pass" }
  | { kind: "block"; reason: string }
  | { kind: "skip"; reason: string };

export interface StepResult {
  stage: PlaygroundStage;
  outcome: PlaygroundOutcome;
  /** 该环节的结构化产物，按 stage 决定怎么渲染 */
  detail: unknown;
}

export interface StepReport {
  steps: StepResult[];
  trace_ids: string[];
}

export type RoundVerdict =
  | { kind: "passed" }
  | { kind: "rejected"; reason: string }
  | { kind: "failed"; reason: string };

export interface LlmUsage {
  prompt_tokens: number | null;
  completion_tokens: number | null;
  total_tokens: number | null;
}

export interface RoundTrace {
  round: number;
  /** 实际送出去的完整提示词，含返工追加段 */
  prompt: string;
  /** 净化后的模型输出，调用失败时为空串 */
  raw: string;
  model: string | null;
  usage: LlmUsage | null;
  duration_ms: number;
  verdict: RoundVerdict;
}

export interface AgentTrace {
  id: string;
  task_name: string;
  /** RFC3339 本地时间 */
  started_at: string;
  duration_ms: number;
  rounds: RoundTrace[];
  /** "first_try" | "recovered"，失败时为 null */
  stop: string | null;
  error: string | null;
}

/** 简历投递入口状态。注意后端是 PascalCase 序列化 */
export type ResumeState = "Sendable" | "RequestedByPeer" | "Unavailable" | "Unknown";

export interface PlaygroundJob {
  title: string;
  company_name: string;
  detail: string;
  salary: string;
  location: string;
}

/** 只在本次调用内生效的提示词覆盖，不落盘 */
export interface PromptOverrides {
  greet_prompt?: string | null;
  reply_prompt?: string | null;
  semantic_filter_intent?: string | null;
}

export interface PlaygroundMessage {
  text: string;
  /** true = HR 发来的，false = 我发出去的 */
  received: boolean;
}

/* -------------------------------------------------------------------------- */
/* 展示元数据                                                                  */
/* -------------------------------------------------------------------------- */

/**
 * 三条链路各对应一条后端命令。环节归属固定不变，所以直接写死在元数据里：
 * 跑「筛选」只会点亮 screen 链上的环节，其余的保持未执行的灰态，
 * 用户一眼能看出「没跑到」和「跑了但被拦下」的区别。
 */
export type PlaygroundChain = "screen" | "greet" | "reply";

export interface StageMeta {
  stage: PlaygroundStage;
  /** 序号从 1 开始，与产品口径的 ①②③ 对齐 */
  order: number;
  label: string;
  chain: PlaygroundChain;
  /** 一句话说明这个环节在判什么，展开前就能看懂 */
  hint: string;
}

export const PLAYGROUND_STAGES: readonly StageMeta[] = [
  { stage: "regex_filter", order: 1, label: "正则与关键词筛选", chain: "screen", hint: "按方案里的关键词与正则规则先过一遍，纯本地判断" },
  { stage: "semantic_match", order: 2, label: "岗位语义复核", chain: "screen", hint: "让模型读 JD，判断岗位方向是否真的对得上" },
  { stage: "greet_decide", order: 3, label: "打招呼决策", chain: "greet", hint: "决定这个岗位要不要打招呼、用什么口径开场" },
  { stage: "greet_compose", order: 4, label: "打招呼序列组装", chain: "greet", hint: "把模型话术与固定模板拼成实际要发的几条消息" },
  { stage: "greet_vet", order: 5, label: "打招呼发送前体检", chain: "greet", hint: "发送前的最后一道闸：占位符、超长、明显机器腔都会被拦下" },
  { stage: "gate", order: 6, label: "回复闸门", chain: "reply", hint: "判断此刻该不该回：对方是否在等回复、额度是否用完" },
  { stage: "route", order: 7, label: "回复路由", chain: "reply", hint: "决定走正则模板还是交给模型生成" },
  { stage: "reply_decide", order: 8, label: "自动回复决策", chain: "reply", hint: "模型给出回复正文与是否该投简历的判断" },
  { stage: "reconcile", order: 9, label: "投递意图校正", chain: "reply", hint: "把模型的投递意图与真实的简历入口状态对齐" },
  { stage: "reply_vet", order: 10, label: "回复发送前体检", chain: "reply", hint: "发送前的最后一道闸，与打招呼体检同一套规则" },
] as const;

const STAGE_META_BY_KEY = new Map<PlaygroundStage, StageMeta>(
  PLAYGROUND_STAGES.map((meta) => [meta.stage, meta]),
);

export function stageMeta(stage: PlaygroundStage): StageMeta | undefined {
  return STAGE_META_BY_KEY.get(stage);
}

const ORDER_SYMBOLS = ["①", "②", "③", "④", "⑤", "⑥", "⑦", "⑧", "⑨", "⑩"];

/** 圈号只到 ⑩，超出后退回普通数字而不是渲染出空白 */
export function stageSymbol(order: number): string {
  return ORDER_SYMBOLS[order - 1] ?? String(order);
}

export const CHAIN_LABELS: Record<PlaygroundChain, string> = {
  screen: "筛选",
  greet: "打招呼",
  reply: "回复",
};

export const RESUME_STATE_OPTIONS: ReadonlyArray<{
  value: ResumeState;
  label: string;
  hint: string;
}> = [
  { value: "Sendable", label: "可主动投递", hint: "简历入口开着，模型说要投就能投" },
  { value: "RequestedByPeer", label: "对方正在索要", hint: "HR 点了要简历，等我方确认" },
  { value: "Unavailable", label: "已投递或不可用", hint: "投过了或平台不给投，投递意图会被校正掉" },
  { value: "Unknown", label: "无法确认", hint: "页面没读到入口状态，按保守口径处理" },
];
