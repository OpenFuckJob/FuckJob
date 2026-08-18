import { describe, expect, it } from "vitest";
import type { JobTaskInfo, JobTaskState, PlatformKind } from "./rpa";
import {
  countJobTasks,
  getActiveTaskForPlatform,
  isActiveJobTask,
  isLongRunningMode,
} from "./rpa";

function task(
  taskId: string,
  platform: PlatformKind,
  status: JobTaskState,
): JobTaskInfo {
  return {
    task_id: taskId,
    platform,
    mode: "job_hunting",
    status,
    created_at: "2026-08-13T00:00:00Z",
    started_at: null,
    finished_at: null,
    error: null,
  };
}

describe("job task overview helpers", () => {
  it.each(["queued", "starting", "running", "stopping"] as const)(
    "treats %s as active",
    (status) => expect(isActiveJobTask(task("active", "boss", status))).toBe(true),
  );

  it.each(["succeeded", "failed", "cancelled"] as const)(
    "treats %s as terminal",
    (status) => expect(isActiveJobTask(task("finished", "boss", status))).toBe(false),
  );

  it("isolates active tasks by platform", () => {
    const tasks = [
      task("boss-running", "boss", "running"),
      task("liepin-finished", "liepin", "succeeded"),
    ];

    expect(getActiveTaskForPlatform(tasks, "boss")?.task_id).toBe("boss-running");
    expect(getActiveTaskForPlatform(tasks, "liepin")).toBeNull();
  });

  it("counts queued and executing tasks without including terminal history", () => {
    const counts = countJobTasks([
      task("boss-running", "boss", "running"),
      task("liepin-queued", "liepin", "queued"),
      task("old-failure", "boss", "failed"),
    ]);

    expect(counts).toEqual({ active: 2, running: 1, queued: 1 });
  });
});

describe("long running modes", () => {
  // 长驻判定决定「同平台不许开第二个」这条约束覆盖到谁，也决定界面上要不要
  // 提示用户任务不会自己结束。漏掉一个模式，用户会排一个永远跑不起来的队
  it.each(["periodic_job_hunting", "polling_reply"] as const)(
    "treats %s as long running",
    (mode) => expect(isLongRunningMode(mode)).toBe(true),
  );

  it.each(["job_hunting", "reply_unread", "sync_chat_history"] as const)(
    "treats %s as a one-shot run",
    (mode) => expect(isLongRunningMode(mode)).toBe(false),
  );
});
