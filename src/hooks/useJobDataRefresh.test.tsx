import { act, renderHook, cleanup } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
import { listen } from "@tauri-apps/api/event";
import { useJobDataRefresh } from "./useJobDataRefresh";

let notify: () => void;
const unlisten = vi.fn();
beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  vi.mocked(listen).mockImplementation(async (_name, callback) => { notify = () => callback({} as never); return unlisten; });
});
afterEach(() => { cleanup(); vi.useRealTimers(); });
it("refreshes after a persisted-data event and by timer, then unsubscribes", async () => {
  const refresh = vi.fn().mockResolvedValue(undefined);
  const { unmount } = renderHook(() => useJobDataRefresh(refresh));
  await act(async () => { notify(); });
  expect(refresh).toHaveBeenCalledTimes(1);
  await act(async () => { vi.advanceTimersByTime(3000); });
  expect(refresh).toHaveBeenCalledTimes(2);
  await act(async () => { unmount(); });
  expect(unlisten).toHaveBeenCalledTimes(1);
});
it("serializes overlapping refreshes and reloads again when a second worker changes data", async () => {
  let finish!: () => void;
  const refresh = vi.fn().mockImplementationOnce(() => new Promise<void>(resolve => { finish = resolve; })).mockResolvedValue(undefined);
  renderHook(() => useJobDataRefresh(refresh));
  await act(async () => { notify(); notify(); notify(); });
  expect(refresh).toHaveBeenCalledTimes(1);
  await act(async () => { finish(); });
  expect(refresh).toHaveBeenCalledTimes(2);
});
