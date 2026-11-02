import { act, render, screen, cleanup, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../hooks/useJobDataRefresh", () => ({ useJobDataRefresh: vi.fn() }));
import { invoke } from "@tauri-apps/api/core";
import { useJobDataRefresh } from "../../hooks/useJobDataRefresh";
import JobOverviewPage from "./index";

let refresh: () => Promise<unknown>;
const overview = (count: number) => ({
  success: true, data: {
    days: 30, high_match_score: 80,
    metrics: { total_jobs: count, communicated_jobs: 0, replied_jobs: 0, reply_rate: 0, resume_sent_jobs: count, high_match_jobs: 0, analyzed_jobs: 0 },
    previous_metrics: { total_jobs: 0, communicated_jobs: 0, replied_jobs: 0, reply_rate: 0, resume_sent_jobs: 0, high_match_jobs: 0, analyzed_jobs: 0 },
    daily_activity: [], source_distribution: [], active_conversations: [],
  },
});
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(useJobDataRefresh).mockImplementation(callback => { refresh = callback; });
});
afterEach(cleanup);

it("recovers the rendered overview after a temporary refresh failure", async () => {
  vi.mocked(invoke).mockResolvedValueOnce(overview(3)).mockRejectedValueOnce(new Error("temporary refresh failure")).mockResolvedValueOnce(overview(4));
  render(<JobOverviewPage />);
  await waitFor(() => expect(screen.queryByText("数据统计基于所选时间范围内的活动记录，趋势图固定展示最近 0 天走势。")).toBeTruthy());
  await act(async () => { await refresh(); });
  expect(screen.getByText("temporary refresh failure")).toBeTruthy();
  await act(async () => { await refresh(); });
  expect(screen.queryByText("temporary refresh failure")).toBeNull();
  expect(screen.getByText("数据统计基于所选时间范围内的活动记录，趋势图固定展示最近 0 天走势。")).toBeTruthy();
});

it("ignores a late initial response after a newer successful refresh", async () => {
  let finishInitial!: (value: unknown) => void;
  vi.mocked(invoke).mockImplementationOnce(() => new Promise(resolve => { finishInitial = resolve; })).mockResolvedValueOnce(overview(4));
  render(<JobOverviewPage />);
  await act(async () => { await refresh(); });
  await act(async () => { finishInitial({ success: false, error: { message: "stale initial error" } }); });
  expect(screen.queryByText("stale initial error")).toBeNull();
  expect(screen.getByText("数据统计基于所选时间范围内的活动记录，趋势图固定展示最近 0 天走势。")).toBeTruthy();
});
