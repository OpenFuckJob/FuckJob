export type JobPlatform = "boss" | "liepin" | "51job";

export type JobPipelineStage =
  | "pending"
  | "applied"
  | "replied"
  | "interview_scheduled"
  | "interviewing"
  | "offer"
  | "rejected"
  | "archived";

export interface JobDetail {
  source_url?: string | null;
  resume_delivery_pending?: boolean;
  id: string;
  platform?: JobPlatform | "";
  profile_id?: string | null;
  profile_name?: string | null;
  title: string;
  company_name: string;
  detail: string;
  salary: string;
  location: string | null;
  is_reply: boolean;
  is_send_resume: boolean;
  created_at: string;
  resume_sent_at: string | null;
  updated_at: string;
  pipeline_stage?: JobPipelineStage | null;
  notes?: string | null;
  interview_at?: string | null;
}

export type CommunicationStatus =
  | "rejected"
  | "interview_invited"
  | "replied"
  | "no_reply";

export interface JobListItem extends JobDetail {
  communication_status: CommunicationStatus;
  /** 后端双轨状态机计算的有效阶段（平台事实 + 手工覆盖正向合并） */
  effective_stage?: JobPipelineStage;
  /** 零点击列表同步抓到的未读条数（不消红点） */
  unread_count?: number;
  /** 平台侧原始状态提示（例如：收到约面 / 不合适 / 2 条未读） */
  platform_status_tag?: string | null;
  latest_message?: string | null;
  latest_message_at?: number | null;
  latest_message_received?: boolean | null;
}

export interface PlatformSyncMeta {
  platform: string;
  last_synced_at?: string | null;
  watermark_ms: number;
  last_scanned: number;
  last_updated: number;
  last_unread_total: number;
  last_error?: string | null;
}

export interface SyncStatusSummary {
  platforms: string[];
  scanned: number;
  updated: number;
  unread_total: number;
  interview_detected: number;
  warnings: string[];
  synced_at: string;
}

/* ── 岗位描述的结构化视图 ──
 * 与 Rust 侧 `job_description::ParsedJobDescription` 对应。清洗规则跟着平台
 * 页面结构走，只在后端维护一份：前端另写一套必然和喂给模型的那份漂移。
 * 取数走 `job_description_view` 命令。
 */

export interface JobSection {
  title: string;
  items: string[];
}

export interface Recruiter {
  name: string;
  /** 「在线」「2周内活跃」这类活跃度描述 */
  status: string;
  company: string;
  role: string;
}

export interface ParsedJobDescription {
  /** 结构化后的正文小节；没识别出标题时会有一个标题为空的兜底小节 */
  sections: JobSection[];
  /** 学历、经验、公司规模这类短标签，主要来自猎聘卡片 */
  highlights: string[];
  workplace: string | null;
  recruiter: Recruiter | null;
  /** 清洗后的正文全文，供「查看原文」兜底 */
  clean_text: string;
  /** 洗完没剩下有效内容——页面结构变了或本就没抓到 JD */
  empty: boolean;
}

export interface ChatMessageRecord {
  id: string;
  job_id: string;
  mid: number;
  received: boolean;
  text: string;
  time: number;
  from_name: string;
}
