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
