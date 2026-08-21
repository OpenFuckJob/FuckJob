import { invoke } from "@tauri-apps/api/core";
import { commandErrorMessage, type CommandResult } from "@/types/command";
import type {
  AgentTrace,
  PlaygroundJob,
  PlaygroundMessage,
  PromptOverrides,
  ResumeState,
  StepReport,
} from "@/types/playground";

/**
 * Tauri 会把 JS 侧的 camelCase 参数名映射成 Rust 的 snake_case，所以这里
 * 传的是 profileId / resumeState 而不是 profile_id。命令返回体本身是后端
 * 直接序列化的结构，仍然保持 snake_case。
 */

async function call<T>(command: string, args: Record<string, unknown>, fallback: string): Promise<T> {
  const result = await invoke<CommandResult<T>>(command, args);
  if (!result?.success || result.data === null || result.data === undefined) {
    throw new Error(commandErrorMessage(result?.error, fallback));
  }
  return result.data;
}

export function runScreen(
  profileId: string,
  job: PlaygroundJob,
  overrides: PromptOverrides,
): Promise<StepReport> {
  return call<StepReport>("playground_screen", { profileId, job, overrides }, "筛选链路执行失败");
}

export function runGreet(
  profileId: string,
  job: PlaygroundJob,
  overrides: PromptOverrides,
): Promise<StepReport> {
  return call<StepReport>("playground_greet", { profileId, job, overrides }, "打招呼链路执行失败");
}

export function runReply(params: {
  profileId: string;
  job: PlaygroundJob;
  messages: PlaygroundMessage[];
  resumeState: ResumeState;
  repliesInWindow: number;
  overrides: PromptOverrides;
}): Promise<StepReport> {
  return call<StepReport>("playground_reply", { ...params }, "回复链路执行失败");
}

export function fetchTraces(ids: string[]): Promise<AgentTrace[]> {
  return call<AgentTrace[]>("playground_traces", { ids }, "读取调用轨迹失败");
}

export async function clearTraces(): Promise<void> {
  // 清空成功时 data 是 null，不能走 call 的非空校验
  const result = await invoke<CommandResult<null>>("playground_clear_traces", {});
  if (!result?.success) throw new Error(commandErrorMessage(result?.error, "清空轨迹失败"));
}

export function exportTraces(path: string): Promise<number> {
  return call<number>("playground_export_traces", { path }, "导出轨迹失败");
}
