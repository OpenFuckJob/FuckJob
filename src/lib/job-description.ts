/**
 * 岗位描述的前端取数出口。
 *
 * 抓下来的 JD 原文混着反爬注入的样式代码、噪声词和页面控件文本，清洗与结构化
 * 统一在 Rust 侧的 `job_description` 模块里完成——前端再写一套必然和喂给模型的
 * 那份漂移。所有要展示或引用岗位描述的地方都从这里取，别直接读 `JobDetail.detail`。
 */

import { invoke } from "@tauri-apps/api/core";
import type { CommandResult } from "../types/command";
import type { ParsedJobDescription } from "../types/job-detail";

const EMPTY: ParsedJobDescription = {
  sections: [],
  highlights: [],
  workplace: null,
  recruiter: null,
  clean_text: "",
  empty: true,
};

/**
 * 取岗位描述的结构化视图。
 *
 * 取不到就返回空结构而不是抛错：调用方展示的是岗位详情，
 * 描述缺失只该让正文区空着，不该把整页拖成错误态。
 */
export async function fetchJobDescription(
  jobId: string,
): Promise<ParsedJobDescription> {
  try {
    const result = await invoke<CommandResult<ParsedJobDescription>>(
      "job_description_view",
      { jobId },
    );
    return result.success && result.data ? result.data : EMPTY;
  } catch {
    return EMPTY;
  }
}

/** 只要清洗后的正文——拼提示词上下文时用 */
export async function fetchJobDescriptionText(jobId: string): Promise<string> {
  return (await fetchJobDescription(jobId)).clean_text;
}

export { EMPTY as EMPTY_JOB_DESCRIPTION };
