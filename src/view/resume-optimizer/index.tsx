import { useEffect, useState } from "react";
import { message } from "antd";
import type { AppRuntimeConfig } from "@/types/app-config";
import type { MockInterviewQuestionReview } from "@/types/analysis";
import type { JobDetail } from "@/types/job-detail";
import { MockInterviewHome } from "./MockInterviewHome";
import { buildInterviewJobContext } from "./MockInterviewSetup";
import { fetchJobDescriptionText } from "@/lib/job-description";
import { MockInterviewPanel } from "./MockInterviewPanel";
import { MockInterviewReportPage } from "./MockInterviewReportPage";
import { MockInterviewSetupPage } from "./MockInterviewSetupPage";
import {
  deleteInterviewSession,
  generateInterviewReport,
  getInterviewSession,
  listInterviewSessions,
  saveInterviewSession,
  subscribeInterviewSessions,
} from "./interview-store";
import {
  DEFAULT_INTERVIEW_SETTINGS,
  createInterviewSession,
  type InterviewSession,
  type MockInterviewSettings,
} from "./interview-types";
import "./style.css";

export interface ResumeMarkdownSection {
  title: string;
  start: number;
  end: number;
}

export function extractSections(content: string): ResumeMarkdownSection[] {
  const lines = content.split("\n");
  const sections: ResumeMarkdownSection[] = [];
  const firstHeaderIdx = lines.findIndex((line) => line.match(/^##\s+/));
  if (firstHeaderIdx > 0) sections.push({ title: "个人信息", start: 0, end: firstHeaderIdx });
  else if (firstHeaderIdx === -1 && lines.some((line) => line.trim())) sections.push({ title: "个人信息", start: 0, end: lines.length });
  let currentSection: { title: string; start: number } | null = null;
  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(/^##\s+(.+)/);
    if (!match) continue;
    if (currentSection) sections.push({ ...currentSection, end: index });
    currentSection = { title: match[1].trim(), start: index };
  }
  if (currentSection) sections.push({ ...currentSection, end: lines.length });
  return sections;
}

export function replaceSectionContent(content: string, section: ResumeMarkdownSection, nextSectionContent: string): string {
  const lines = content.split("\n");
  return [lines.slice(0, section.start).join("\n"), nextSectionContent, lines.slice(section.end).join("\n")].filter(Boolean).join("\n");
}

export function findSectionIndexByRenderedTitle(sections: ResumeMarkdownSection[], renderedTitle: string): number {
  const normalizedTitle = renderedTitle.replace(/^#+\s*/, "").trim();
  return sections.findIndex((section) => section.title.trim() === normalizedTitle);
}

export interface ResumeOptimizerPageProps {
  config: AppRuntimeConfig;
  onOpenLlmConfig: () => void;
  onUpdateResume: (content: string) => void;
  /** 从岗位管理跳过来时要直接预填的岗位 */
  pendingInterviewJob?: JobDetail;
  onPendingInterviewHandled?: () => void;
}

type PageState =
  | { name: "home" }
  | { name: "setup" }
  | { name: "session"; sessionId: string }
  | { name: "report"; sessionId: string; initialTab?: "summary" | "abilities" | "questions" | "transcript" };

function ResumeOptimizerPage({ config, onOpenLlmConfig, pendingInterviewJob, onPendingInterviewHandled }: ResumeOptimizerPageProps) {
  const [page, setPage] = useState<PageState>({ name: "home" });
  const [sessions, setSessions] = useState<InterviewSession[]>(listInterviewSessions);
  const [settings, setSettings] = useState<MockInterviewSettings>({ ...DEFAULT_INTERVIEW_SETTINGS });
  const [setupFromJob, setSetupFromJob] = useState(false);
  const [messageApi, contextHolder] = message.useMessage();
  const resumeContent = (config.resume_config.resume_content ?? "").trim();
  const canStart = !!config.llm_config && !!resumeContent;

  useEffect(() => subscribeInterviewSessions(() => setSessions(listInterviewSessions())), []);
  // 从岗位管理带岗位过来时直接进配置页，岗位信息已经填好，用户只需要挑面试参数
  useEffect(() => {
    if (!pendingInterviewJob) return;
    // JD 要先经后端清洗，取数是异步的；页面切换不等它，
    // 岗位标题公司这些本地就有，用户可以立刻开始挑面试参数
    const job = pendingInterviewJob;
    void fetchJobDescriptionText(job.id).then((detail) =>
      setSettings({
        ...DEFAULT_INTERVIEW_SETTINGS,
        selectedJobId: job.id,
        jobTitle: job.title.trim(),
        companyName: job.company_name.trim(),
        jobContext: buildInterviewJobContext(job, detail),
      }),
    );
    setSetupFromJob(true);
    setPage({ name: "setup" });
    onPendingInterviewHandled?.();
  }, [onPendingInterviewHandled, pendingInterviewJob]);
  useEffect(() => {
    sessions
      .filter((session) => session.status === "report_queued" || session.status === "report_generating")
      .forEach((session) => void generateInterviewReport(session.id));
  }, [sessions]);

  const startSession = (nextSettings: MockInterviewSettings, resumeSnapshot = resumeContent) => {
    const session = saveInterviewSession(createInterviewSession(nextSettings, resumeSnapshot));
    setPage({ name: "session", sessionId: session.id });
  };

  const restartSession = (sessionId: string) => {
    const source = getInterviewSession(sessionId);
    if (!source) return;
    startSession({ ...source.settings, focusAreas: [...source.settings.focusAreas] }, resumeContent || source.resumeSnapshot);
  };

  const practiceQuestion = (sessionId: string, review: MockInterviewQuestionReview) => {
    const source = getInterviewSession(sessionId);
    if (!source) return;
    startSession({
      ...source.settings,
      duration: "quick",
      focusAreas: [review.module].filter(Boolean),
      customFocus: `专项练习：${review.question}`,
    }, resumeContent || source.resumeSnapshot);
  };

  return (
    <div className="mi-root">
      {contextHolder}
      {page.name === "home" && (
        <MockInterviewHome
          sessions={sessions}
          canStart={canStart}
          onCreate={() => {
            if (!canStart) {
              messageApi.warning("请先完成AI模型配置和简历配置");
              return;
            }
            setSettings({ ...DEFAULT_INTERVIEW_SETTINGS });
            setSetupFromJob(false);
            setPage({ name: "setup" });
          }}
          onContinue={(sessionId) => setPage({ name: "session", sessionId })}
          onOpenReport={(sessionId) => setPage({ name: "report", sessionId })}
          onOpenTranscript={(sessionId) => setPage({ name: "report", sessionId, initialTab: "transcript" })}
          onRestart={restartSession}
          onDelete={(sessionId) => deleteInterviewSession(sessionId)}
          onRetryReport={(sessionId) => {
            setPage({ name: "report", sessionId });
            void generateInterviewReport(sessionId);
          }}
        />
      )}

      {page.name === "setup" && (
        <MockInterviewSetupPage
          value={settings}
          resumeReady={!!resumeContent}
          aiReady={!!config.llm_config}
          fromJob={setupFromJob}
          onChange={setSettings}
          onBack={() => setPage({ name: "home" })}
          onConfigureAi={onOpenLlmConfig}
          onStart={() => startSession(settings)}
        />
      )}

      {page.name === "session" && (
        <MockInterviewPanel
          sessionId={page.sessionId}
          onBack={() => setPage({ name: "home" })}
          onReport={() => setPage({ name: "report", sessionId: page.sessionId })}
        />
      )}

      {page.name === "report" && (
        <MockInterviewReportPage
          sessionId={page.sessionId}
          sessions={sessions}
          initialTab={page.initialTab}
          onBack={() => setPage({ name: "home" })}
          onRestart={() => restartSession(page.sessionId)}
          onPracticeQuestion={(review) => practiceQuestion(page.sessionId, review)}
        />
      )}
    </div>
  );
}

export default ResumeOptimizerPage;
