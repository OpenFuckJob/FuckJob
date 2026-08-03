import { parseMockInterviewReport, streamMockInterviewSummary } from "@/lib/mock-interview";
import type { MockInterviewChatMessage } from "@/types/analysis";
import type { InterviewSession } from "./interview-types";

const STORAGE_KEY = "offerflow.mock-interviews.v1";
const CHANGE_EVENT = "offerflow:mock-interviews-changed";
const runningReports = new Set<string>();

function safeRead(): InterviewSession[] {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const value = JSON.parse(raw) as InterviewSession[];
    return Array.isArray(value) ? value : [];
  } catch {
    return [];
  }
}
function write(sessions: InterviewSession[]): void {
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(sessions));
  window.dispatchEvent(new CustomEvent(CHANGE_EVENT));
}

export function listInterviewSessions(): InterviewSession[] {
  return safeRead().sort((left, right) => right.updatedAt.localeCompare(left.updatedAt));
}

export function getInterviewSession(id: string): InterviewSession | undefined {
  return safeRead().find((item) => item.id === id);
}

export function saveInterviewSession(session: InterviewSession): InterviewSession {
  const next = { ...session, updatedAt: new Date().toISOString() };
  const sessions = safeRead();
  const index = sessions.findIndex((item) => item.id === session.id);
  if (index >= 0) sessions[index] = next;
  else sessions.unshift(next);
  write(sessions);
  return next;
}

export function updateInterviewSession(
  id: string,
  updater: (session: InterviewSession) => InterviewSession,
): InterviewSession | undefined {
  const sessions = safeRead();
  const index = sessions.findIndex((item) => item.id === id);
  if (index < 0) return undefined;
  sessions[index] = { ...updater(sessions[index]), updatedAt: new Date().toISOString() };
  write(sessions);
  return sessions[index];
}

export function deleteInterviewSession(id: string): void {
  write(safeRead().filter((item) => item.id !== id));
}

export function subscribeInterviewSessions(listener: () => void): () => void {
  window.addEventListener(CHANGE_EVENT, listener);
  window.addEventListener("storage", listener);
  return () => {
    window.removeEventListener(CHANGE_EVENT, listener);
    window.removeEventListener("storage", listener);
  };
}

function toHistory(session: InterviewSession): MockInterviewChatMessage[] {
  return session.messages
    .filter((item) => item.content.trim())
    .map(({ role, content }) => ({ role, content }));
}

export async function generateInterviewReport(sessionId: string): Promise<void> {
  if (runningReports.has(sessionId)) return;
  const session = getInterviewSession(sessionId);
  if (!session) return;
  runningReports.add(sessionId);
  updateInterviewSession(sessionId, (current) => ({
    ...current,
    status: "report_generating",
    reportError: undefined,
  }));

  try {
    const content = await streamMockInterviewSummary({
      sessionId,
      resumeContent: session.resumeSnapshot,
      history: toHistory(session),
      jobContext: session.settings.jobContext,
      interviewType: session.settings.interviewType,
      difficulty: session.settings.difficulty,
    });
    const report = parseMockInterviewReport(content);
    updateInterviewSession(sessionId, (current) => ({
      ...current,
      status: "report_completed",
      report,
      reportError: undefined,
    }));
  } catch (error) {
    updateInterviewSession(sessionId, (current) => ({
      ...current,
      status: "report_failed",
      reportError: error instanceof Error ? error.message : "报告生成失败",
    }));
  } finally {
    runningReports.delete(sessionId);
  }
}
