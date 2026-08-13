import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import {
  filterTaskLogContent,
  filterTasksByPlatform,
  requestStopJobTask,
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
});
