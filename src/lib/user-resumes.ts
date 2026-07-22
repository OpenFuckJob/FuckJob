import { invoke } from "@tauri-apps/api/core";
import { commandErrorMessage, type CommandResult } from "@/types/command";

export interface ResumeEntry {
  content: string;
  thumbnail?: string | null;
}

export type UserResumes = Record<string, ResumeEntry>;

export async function loadUserResumes(): Promise<UserResumes> {
  const result = await invoke<CommandResult<UserResumes>>("load_user_resumes");
  if (!result.success) return {};
  return result.data ?? {};
}

export async function saveUserResumes(resumes: UserResumes): Promise<void> {
  const result = await invoke<CommandResult<null>>("save_user_resumes", { resumes });
  if (!result.success) {
    throw new Error(commandErrorMessage(result.error, "保存失败"));
  }
}
