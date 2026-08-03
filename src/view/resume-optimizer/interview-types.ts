import type { MockInterviewReport } from "@/types/analysis";

export type InterviewDuration = "quick" | "standard" | "deep";
export type InterviewSessionStatus =
  | "paused"
  | "in_progress"
  | "report_queued"
  | "report_generating"
  | "report_completed"
  | "report_failed";

export interface InterviewModule {
  id: string;
  name: string;
  description: string;
  weight: number;
  targetQuestions: number;
  completedQuestions: number;
  followUpQuestions: number;
}
export interface MockInterviewSettings {
  selectedJobId?: string;
  jobTitle: string;
  companyName: string;
  jobContext: string;
  interviewType: string;
  difficulty: string;
  duration: InterviewDuration;
  focusAreas: string[];
  customFocus: string;
}

export interface InterviewMessage {
  id: string;
  role: "interviewer" | "candidate" | "system";
  content: string;
  createdAt: string;
  moduleId?: string;
  questionKind?: "core" | "followup";
  skipped?: boolean;
}

export interface InterviewSession {
  id: string;
  createdAt: string;
  updatedAt: string;
  completedAt?: string;
  status: InterviewSessionStatus;
  settings: MockInterviewSettings;
  resumeSnapshot: string;
  messages: InterviewMessage[];
  modules: InterviewModule[];
  currentModuleIndex: number;
  mainQuestionCount: number;
  followUpCount: number;
  draft: string;
  report?: MockInterviewReport;
  reportError?: string;
}

export const DURATION_META: Record<InterviewDuration, { label: string; minutes: string; targetQuestions: number }> = {
  quick: { label: "快速面试", minutes: "10～15分钟", targetQuestions: 7 },
  standard: { label: "标准面试", minutes: "25～35分钟", targetQuestions: 12 },
  deep: { label: "深度面试", minutes: "40～60分钟", targetQuestions: 18 },
};

export const DEFAULT_INTERVIEW_SETTINGS: MockInterviewSettings = {
  jobTitle: "",
  companyName: "",
  jobContext: "",
  interviewType: "技术面",
  difficulty: "中级",
  duration: "standard",
  focusAreas: [],
  customFocus: "",
};

const BASE_MODULES = [
  ["motivation", "开场与动机", "自我介绍、求职动机和岗位理解", 10],
  ["project", "项目深挖", "项目背景、个人贡献、技术取舍和结果", 30],
  ["professional", "专业能力", "岗位核心知识、技术深度和工程实践", 25],
  ["scenario", "场景问题", "问题分析、方案设计、异常处理和复盘", 20],
  ["collaboration", "协作行为", "沟通协作、推动落地和冲突处理", 10],
  ["career", "职业规划", "发展方向、岗位匹配和候选人反问", 5],
] as const;

export function createInterviewModules(settings: MockInterviewSettings): InterviewModule[] {
  const target = DURATION_META[settings.duration].targetQuestions;
  const focusText = `${settings.focusAreas.join(" ")} ${settings.customFocus}`;
  const modules = BASE_MODULES.map(([id, name, description, baseWeight]) => ({
    id,
    name,
    description,
    weight: baseWeight,
    targetQuestions: Math.max(1, Math.round((target * baseWeight) / 100)),
    completedQuestions: 0,
    followUpQuestions: 0,
  }));

  if (focusText.trim()) {
    for (const module of modules) {
      if (
        (module.id === "project" && /项目|贡献|经历/.test(focusText)) ||
        (module.id === "professional" && /技术|Agent|RAG|模型|架构|工程|Python|Docker/.test(focusText)) ||
        (module.id === "scenario" && /场景|排查|系统设计|稳定|性能|成本/.test(focusText)) ||
        (module.id === "collaboration" && /协作|沟通|管理|推动/.test(focusText))
      ) {
        module.targetQuestions += 1;
      }
    }
  }
  return modules;
}

export function createInterviewSession(settings: MockInterviewSettings, resumeSnapshot: string): InterviewSession {
  const now = new Date().toISOString();
  return {
    id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    createdAt: now,
    updatedAt: now,
    status: "in_progress",
    settings,
    resumeSnapshot,
    messages: [],
    modules: createInterviewModules(settings),
    currentModuleIndex: 0,
    mainQuestionCount: 0,
    followUpCount: 0,
    draft: "",
  };
}
