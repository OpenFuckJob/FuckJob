import { describe, expect, it } from "vitest";
import {
  MANUAL_REVIEW_REASON_COLORS,
  MANUAL_REVIEW_REASON_LABELS,
  formatRelativeTime,
  type ManualReviewReason,
} from "./manual-review";

const ALL_REASONS: ManualReviewReason[] = [
  "risk_keyword",
  "vet_rejected",
  "missing_job_id",
  "throttle_exhausted",
];

describe("manual review reasons", () => {
  // 后端新增一种原因而前端忘了补映射时，列表里会渲染出空白标签——
  // 用户只看到一个没有文字的色块，完全不知道发生了什么
  it.each(ALL_REASONS)("gives %s both a label and a colour", (reason) => {
    expect(MANUAL_REVIEW_REASON_LABELS[reason]).toBeTruthy();
    expect(MANUAL_REVIEW_REASON_COLORS[reason]).toBeTruthy();
  });
});

describe("formatRelativeTime", () => {
  const now = new Date("2026-08-18T12:00:00").getTime();

  it("reads out how long the item has been waiting", () => {
    expect(formatRelativeTime(now - 30_000, now)).toBe("刚刚");
    expect(formatRelativeTime(now - 5 * 60_000, now)).toBe("5 分钟前");
    expect(formatRelativeTime(now - 3 * 3600_000, now)).toBe("3 小时前");
    expect(formatRelativeTime(now - 2 * 24 * 3600_000, now)).toBe("2 天前");
  });

  // 两端时钟不一致时时间戳可能落在未来。显示「-3 分钟前」比不精确更糟
  it("treats future timestamps as just now instead of negative durations", () => {
    expect(formatRelativeTime(now + 60_000, now)).toBe("刚刚");
  });

  // 超过一个月就不再数天数了，「47 天前」对用户没有意义
  it("switches to an absolute date once the item is over a month old", () => {
    expect(formatRelativeTime(now - 40 * 24 * 3600_000, now)).not.toContain("天前");
  });
});
