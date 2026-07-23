import { invoke } from "@tauri-apps/api/core";
import type {
  MockInterviewQuestionRequest,
  MockInterviewReport,
  MockInterviewSummaryRequest,
  OptimizeWithAnswerRequest,
  PredictedQuestion,
  ResumeLlmResult,
} from "@/types/analysis";
import type { CommandResult } from "@/types/command";
import { unwrap } from "@/types/command";

export function predictResumeQuestions(
  resumeContent: string,
): Promise<PredictedQuestion[]> {
  return invoke<CommandResult<PredictedQuestion[]>>(
    "predict_resume_questions",
    { resumeContent },
  ).then(unwrap);
}

export function optimizeResumeWithAnswer(
  request: OptimizeWithAnswerRequest,
): Promise<ResumeLlmResult> {
  return invoke<CommandResult<ResumeLlmResult>>(
    "optimize_resume_with_answer",
    { request },
  ).then(unwrap);
}

export function streamMockInterviewQuestion(
  request: MockInterviewQuestionRequest,
): Promise<string> {
  return invoke<CommandResult<string>>(
    "stream_mock_interview_question",
    { request },
  ).then(unwrap);
}

export function streamMockInterviewSummary(
  request: MockInterviewSummaryRequest,
): Promise<string> {
  return invoke<CommandResult<string>>(
    "stream_mock_interview_summary",
    { request },
  ).then(unwrap);
}

export function parseMockInterviewReport(content: string): MockInterviewReport {
  const report = JSON.parse(content) as MockInterviewReport;
  if (!Array.isArray(report.dimensions) || !Array.isArray(report.optimizations)) {
    throw new Error("模拟面试报告格式无效");
  }
  return report;
}
