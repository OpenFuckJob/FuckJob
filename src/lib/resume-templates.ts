import { invoke } from "@tauri-apps/api/core";
import { commandErrorMessage, type CommandResult } from "@/types/command";

export interface ResumeTemplate {
  id: string;
  year: string;
  job: string;
  title: string;
  tag: string[];
  thumbnail: string;
  template: string;
  author: string;
  avatar: string;
  theme: string;
  color: string;
  collect: number;
  updateTime: number;
}

export async function loadResumeTemplates(): Promise<ResumeTemplate[]> {
  const result = await invoke<CommandResult<ResumeTemplate[]>>("get_resume_templates");
  if (!result.success || result.data === null) {
    throw new Error(
      commandErrorMessage(result.error, "Failed to load templates"),
    );
  }
  return result.data;
}
