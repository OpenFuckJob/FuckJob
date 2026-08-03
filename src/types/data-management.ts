export interface BackupStats {
  job_details_count: number;
  chat_messages_count: number;
  interview_analyses_count: number;
  user_resumes_count: number;
}

export interface ExportManifest {
  version: string;
  app_version: string;
  exported_at: string;
  stats: BackupStats;
  includes_config: boolean;
}

export interface ImportResultStats {
  job_details_added: number;
  job_details_updated: number;
  chat_messages_added: number;
  interview_analyses_added: number;
  interview_analyses_updated: number;
  user_resumes_added: number;
  config_imported: boolean;
}

export type ImportStrategy = "merge" | "overwrite";
