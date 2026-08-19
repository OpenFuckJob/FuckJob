export type JobPlatform = "boss" | "liepin";

export interface JobDetail {
  id: string;
  platform?: JobPlatform | "";
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
}

export type CommunicationStatus = "rejected" | "replied" | "no_reply";

export interface JobListItem extends JobDetail {
  communication_status: CommunicationStatus;
  latest_message?: string | null;
  latest_message_at?: number | null;
  latest_message_received?: boolean | null;
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
