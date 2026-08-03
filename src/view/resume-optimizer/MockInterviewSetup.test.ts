import { describe, expect, it } from "vitest";
import type { JobDetail } from "@/types/job-detail";
import {
  buildInterviewJobContext,
  isSelectableInterviewJob,
} from "./MockInterviewSetup";

function job(overrides: Partial<JobDetail> = {}): JobDetail {
  return {
    id: "job-1",
    platform: "boss",
    title: "Agent 开发工程师",
    company_name: "示例科技",
    detail: "负责 Agent 平台研发",
    salary: "20-30K",
    location: "南京",
    is_reply: false,
    is_send_resume: false,
    created_at: "2026-08-03 10:00:00",
    resume_sent_at: null,
    updated_at: "2026-08-03 10:00:00",
    ...overrides,
  };
}

describe("mock interview job selection", () => {
  it("hides chat sync placeholder records", () => {
    expect(isSelectableInterviewJob(job({ title: "聊天同步岗位" }))).toBe(false);
    expect(isSelectableInterviewJob(job({ company_name: "BOSS 会话" }))).toBe(false);
  });

  it("keeps real captured jobs selectable", () => {
    expect(isSelectableInterviewJob(job())).toBe(true);
  });

  it("builds interview context from the selected job", () => {
    expect(buildInterviewJobContext(job())).toContain(
      "岗位：Agent 开发工程师\n公司：示例科技\n薪资：20-30K\n地点：南京\nJD：\n负责 Agent 平台研发",
    );
  });
});
