import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import {
  describePendingReview,
  describeReplyActiveHours,
  filterLogLines,
  filterTaskLogContent,
  filterTasksByPlatform,
  getTaskProfileLabel,
  isGreetConfigReady,
  parseLogLineLevel,
  requestStopJobTask,
  resolveStepStatus,
  resolveWorkspaceEffectiveStage,
  sortTasksForQueue,
  sortTasksNewestFirst,
} from ".";
import type { JobTaskInfo, JobTaskState, PlatformKind } from "../../types/rpa";

function task(
  taskId: string,
  platform: PlatformKind,
  status: JobTaskState,
  createdAt: string,
): JobTaskInfo {
  return {
    task_id: taskId,
    platform,
    mode: "job_hunting",
    status,
    created_at: createdAt,
    started_at: null,
    finished_at: null,
    error: null,
  };
}

describe("workspace task API", () => {
  beforeEach(() => vi.clearAllMocks());

  it("passes the selected task id when stopping a task", async () => {
    vi.mocked(invoke).mockResolvedValue({ success: true, data: null, error: null });

    await requestStopJobTask("liepin-task-42");

    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("stop_job_task", {
      taskId: "liepin-task-42",
    });
  });

  it("filters logs by the exact selected task id", () => {
    const content = [
      "[INFO] [task=boss-1] [BOSS] first",
      "[INFO] [task=liepin-2] [猎聘] second",
      "[INFO] legacy line",
    ].join("\n");

    expect(filterTaskLogContent(content, "liepin-2")).toBe(
      "[INFO] [task=liepin-2] [猎聘] second",
    );
    expect(filterTaskLogContent(content, null)).toBe(content);
  });

  it("orders running tasks first, queued tasks FIFO, then recent history", () => {
    const tasks = [
      task("old-finished", "boss", "succeeded", "2026-08-13T08:00:00Z"),
      task("queue-two", "boss", "queued", "2026-08-13T10:00:00Z"),
      task("running", "liepin", "running", "2026-08-13T11:00:00Z"),
      task("queue-one", "boss", "queued", "2026-08-13T09:00:00Z"),
      task("new-failed", "liepin", "failed", "2026-08-13T12:00:00Z"),
    ];

    expect(sortTasksForQueue(tasks).map((item) => item.task_id)).toEqual([
      "running",
      "queue-one",
      "queue-two",
      "new-failed",
      "old-finished",
    ]);
  });

  it("orders the visible queue newest first", () => {
    const tasks = [
      task("old", "boss", "queued", "2026-08-13T08:00:00Z"),
      task("new", "liepin", "running", "2026-08-13T12:00:00Z"),
      task("middle", "boss", "failed", "2026-08-13T10:00:00Z"),
    ];

    expect(sortTasksNewestFirst(tasks).map((item) => item.task_id)).toEqual([
      "new",
      "middle",
      "old",
    ]);
  });

  it("filters the queue by platform without changing task order", () => {
    const tasks = [
      task("liepin-new", "liepin", "running", "2026-08-13T12:00:00Z"),
      task("boss-middle", "boss", "queued", "2026-08-13T10:00:00Z"),
      task("liepin-old", "liepin", "failed", "2026-08-13T08:00:00Z"),
    ];

    expect(filterTasksByPlatform(tasks, "liepin").map((item) => item.task_id)).toEqual([
      "liepin-new",
      "liepin-old",
    ]);
    expect(filterTasksByPlatform(tasks, "all")).toBe(tasks);
  });

  it("shows conversation routing for reply tasks without a fixed profile", () => {
    const replyTask = { ...task("reply", "boss", "queued", "2026-08-13T12:00:00Z"), mode: "reply_unread" as const };
    expect(getTaskProfileLabel(replyTask)).toBe("按会话方案自动路由");
    expect(getTaskProfileLabel({ ...replyTask, profile_name: "按会话自动选择" })).toBe("按会话方案自动路由");
    expect(getTaskProfileLabel({ ...replyTask, profile_id: "ai", profile_name: "AI 工程师" })).toBe("AI 工程师");
  });
});

describe("pending review tile", () => {
  // 磁贴能待在统计区的前提就是这条：没事时安静，有事时抢眼。
  // 两种状态用同一套配色的话，用户根本不会注意到有会话在等他
  it("stays muted at zero and turns alarming once anything is waiting", () => {
    const idle = describePendingReview(0);
    const waiting = describePendingReview(3);

    expect(idle.value).toBe("0");
    expect(waiting.value).toBe("3");
    expect(idle.color).not.toBe(waiting.color);
    expect(idle.bg).not.toBe(waiting.bg);
  });

  // 副标题要告诉用户接下来干什么，不能两种状态说同一句话
  it("tells the user what to do next in each state", () => {
    expect(describePendingReview(0).subtitle).not.toBe(describePendingReview(1).subtitle);
    expect(describePendingReview(1).subtitle).toContain("点击");
  });
});

describe("reply active hours copy", () => {
  it("renders 全天回复 when disabled or when start equals end (0:00 - 0:00)", () => {
    expect(
      describeReplyActiveHours({ active_hours_enabled: false, active_start_hour: 9, active_end_hour: 22 }),
    ).toBe("全天回复");
    expect(
      describeReplyActiveHours({ active_hours_enabled: true, active_start_hour: 0, active_end_hour: 0 }),
    ).toBe("全天回复");
    expect(
      describeReplyActiveHours({ active_hours_enabled: true, active_start_hour: 9, active_end_hour: 22 }),
    ).toBe("只在 9:00 - 22:00 之间回复");
  });
});

describe("log line classification and filtering", () => {
  it("classifies error, warning, success, and info lines", () => {
    expect(parseLogLineLevel("[ERROR] 检测到 BOSS 风控熔断")).toBe("error");
    expect(parseLogLineLevel("[WARN] 跳过不匹配岗位")).toBe("warn");
    expect(parseLogLineLevel("[INFO] 后端工程师 初次沟通成功")).toBe("success");
    expect(parseLogLineLevel("[INFO] 正在加载页面")).toBe("info");
  });

  it("filters log lines by task id, severity, and keyword", () => {
    const sample = [
      "[INFO] [task=t1] 页面加载到 30 条",
      "[WARN] [task=t1] 列表前置过滤跳过：外包岗位",
      "[ERROR] [task=t1] 检测到 BOSS 风控熔断",
      "[ERROR] [task=t2] 其他任务错误",
    ].join("\n");

    expect(filterLogLines(sample, "t1", "warn_error", "")).toHaveLength(2);
    expect(filterLogLines(sample, "t1", "warn_error", "风控")).toEqual([
      "[ERROR] [task=t1] 检测到 BOSS 风控熔断",
    ]);
  });
});

describe("workspace readiness and env step status", () => {
  const activeLlm = {
    llm_config: {
      provider: "openai" as const,
      base_url: "https://api.openai.com/v1",
      model: "gpt-4o",
      insecure: false,
    },
    llm_enabled: true,
  };
  const noLlm = { llm_config: null, llm_enabled: true };

  it("mirrors Rust GreetConfig::has_sendable_resource for LLM and static greetings", () => {
    expect(
      isGreetConfigReady(activeLlm, {
        enable_llm: true,
        reply_prompt: "写一句打招呼",
        default_template: [{ resource_type: "LLM", content: "", enabled: true }],
      }),
    ).toBe(true);

    expect(
      isGreetConfigReady(activeLlm, {
        enable_llm: true,
        reply_prompt: "   ",
        default_template: [{ resource_type: "LLM", content: "", enabled: true }],
      }),
    ).toBe(false);

    expect(
      isGreetConfigReady(activeLlm, {
        enable_llm: true,
        reply_prompt: "写一句打招呼",
        default_template: [],
      }),
    ).toBe(false);

    expect(
      isGreetConfigReady(noLlm, {
        enable_llm: false,
        reply_prompt: null,
        default_template: [{ resource_type: "LLM", content: "", enabled: true }],
      }),
    ).toBe(false);

    expect(
      isGreetConfigReady(noLlm, {
        enable_llm: false,
        reply_prompt: null,
        default_template: [{ resource_type: "Text", content: "您好，期待沟通", enabled: true }],
      }),
    ).toBe(true);
  });

  it("marks step 0 as error when check_env fails without result", () => {
    expect(resolveStepStatus("done", null, 0, 0, true)).toBe("error");
    expect(resolveStepStatus("done", "completed", 2, 2, false)).toBe("finish");
    expect(resolveStepStatus("done", "login_required", 1, 1, false)).toBe("error");
  });

  it("derives effective stage from synced communication signals", () => {
    expect(
      resolveWorkspaceEffectiveStage({
        id: "boss:1",
        title: "Frontend",
        company_name: "Acme",
        detail: "",
        salary: "30K",
        location: null,
        is_reply: false,
        is_send_resume: false,
        created_at: "2026-11-28T00:00:00Z",
        resume_sent_at: null,
        updated_at: "2026-11-28T00:00:00Z",
        communication_status: "interview_invited",
      }),
    ).toBe("interview_scheduled");
  });
});
