export interface InterviewJobAnalysis {
  job_id: string;
  analyzed_at: string;
  fit_summary: string;
  match_score: number;
  strengths: string[];
  risks: string[];
  skill_matrix: SkillEvidence[];
  likely_questions: InterviewQuestion[];
  questions_to_ask_interviewer: string[];
  search_summary: string;
  search_sources: SearchSource[];
  chat_context: string;
  raw_response: string;
  parse_error?: string | null;
}

export interface SearchSource {
  title: string;
  url: string;
  snippet: string;
}

export interface SkillEvidence {
  requirement: string;
  resume_evidence: string;
  gap: string;
  prep_action: string;
}

export interface InterviewQuestion {
  category: string;
  question: string;
  why: string;
  answer_outline: string;
}

export interface PredictedQuestion {
  id: number;
  question: string;
  intent: string;
  target_section: string;
}

export interface OptimizeWithAnswerRequest {
  resume_content: string;
  question: string;
  user_answer: string;
  section_title: string;
}

export interface ResumeLlmResult {
  success: boolean;
  data: string;
}

export interface MockInterviewChatMessage {
  role: "interviewer" | "candidate" | "system";
  content: string;
}

export interface MockInterviewQuestionRequest {
  sessionId: string;
  resumeContent: string;
  history: MockInterviewChatMessage[];
  round: number;
  maxRounds: number;
}

export interface MockInterviewSummaryRequest {
  sessionId: string;
  resumeContent: string;
  history: MockInterviewChatMessage[];
}

export interface MockInterviewStreamPayload {
  sessionId: string;
  kind: "question" | "summary";
  content: string;
}
