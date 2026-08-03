import { describe, expect, it } from "vitest";
import {
  DEFAULT_INTERVIEW_SETTINGS,
  createInterviewModules,
  createInterviewSession,
} from "./interview-types";

describe("mock interview plan", () => {
  it("creates a comprehensive standard interview plan", () => {
    const modules = createInterviewModules(DEFAULT_INTERVIEW_SETTINGS);
    expect(modules.map((item) => item.name)).toEqual([
      "开场与动机",
      "项目深挖",
      "专业能力",
      "场景问题",
      "协作行为",
      "职业规划",
    ]);
    expect(modules.reduce((sum, item) => sum + item.targetQuestions, 0)).toBe(12);
    expect(modules.reduce((sum, item) => sum + item.weight, 0)).toBe(100);
  });

  it("raises relevant module coverage for selected focus areas", () => {
    const base = createInterviewModules(DEFAULT_INTERVIEW_SETTINGS);
    const focused = createInterviewModules({
      ...DEFAULT_INTERVIEW_SETTINGS,
      focusAreas: ["Agent工作流", "系统设计"],
    });
    expect(focused.find((item) => item.id === "professional")!.targetQuestions)
      .toBeGreaterThan(base.find((item) => item.id === "professional")!.targetQuestions);
    expect(focused.find((item) => item.id === "scenario")!.targetQuestions)
      .toBeGreaterThan(base.find((item) => item.id === "scenario")!.targetQuestions);
  });

  it("creates a resumable session with frozen settings", () => {
    const settings = { ...DEFAULT_INTERVIEW_SETTINGS, jobTitle: "AI应用开发工程师" };
    const session = createInterviewSession(settings, "## 项目经历");
    expect(session.status).toBe("in_progress");
    expect(session.resumeSnapshot).toBe("## 项目经历");
    expect(session.modules).toHaveLength(6);
    expect(session.mainQuestionCount).toBe(0);
  });
});
