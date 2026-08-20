import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import ReplyPollingSection, {
  describeReplyActiveHours,
  replyActiveHoursToGrid,
  replyGridToActiveHours,
} from "./ReplyPollingSection";
import {
  DEFAULT_REPLY_POLLING_CONFIG,
  HOURS_PER_DAY,
  type ReplyPollingConfig,
} from "@/types/app-config";

afterEach(cleanup);

function config(overrides: Partial<ReplyPollingConfig> = {}): ReplyPollingConfig {
  return { ...DEFAULT_REPLY_POLLING_CONFIG, ...overrides };
}

const hoursOf = (...ranges: Array<[number, number]>) =>
  Array.from({ length: HOURS_PER_DAY }, (_, hour) =>
    ranges.some(([from, to]) => hour >= from && hour < to),
  );

describe("ReplyPollingSection 回复时段转换", () => {
  it("把常规起止小时摊成小时格", () => {
    const hours = replyActiveHoursToGrid(config({ active_start_hour: 9, active_end_hour: 22 }));

    expect(hours[9]).toBe(true);
    expect(hours[21]).toBe(true);
    expect(hours[22]).toBe(false);
    expect(hours[3]).toBe(false);
  });

  it("起止相同表示全天回复", () => {
    const hours = replyActiveHoursToGrid(config({ active_start_hour: 9, active_end_hour: 9 }));

    expect(hours.every(Boolean)).toBe(true);
    expect(describeReplyActiveHours(config({ active_start_hour: 9, active_end_hour: 9 })))
      .toBe("全天回复");
    expect(describeReplyActiveHours(config({ active_start_hour: 0, active_end_hour: 24 })))
      .toBe("全天回复");
  });

  it("跨午夜时段在两端都点亮", () => {
    const hours = replyActiveHoursToGrid(config({ active_start_hour: 22, active_end_hour: 8 }));

    expect(hours[23]).toBe(true);
    expect(hours[2]).toBe(true);
    expect(hours[12]).toBe(false);
    expect(describeReplyActiveHours(config({ active_start_hour: 22, active_end_hour: 8 })))
      .toBe("22:00-08:00 回复");
  });

  it("小时格写回现有单段起止字段", () => {
    expect(replyGridToActiveHours(hoursOf([9, 12]))).toEqual({
      active_start_hour: 9,
      active_end_hour: 12,
    });
    expect(replyGridToActiveHours(hoursOf([0, 8], [22, 24]))).toEqual({
      active_start_hour: 22,
      active_end_hour: 8,
    });
    expect(replyGridToActiveHours(Array.from({ length: HOURS_PER_DAY }, () => true))).toEqual({
      active_start_hour: 0,
      active_end_hour: 0,
    });
  });

  it("多段选择会收敛成覆盖这些小时的最短连续范围", () => {
    expect(replyGridToActiveHours(hoursOf([9, 12], [14, 18]))).toEqual({
      active_start_hour: 9,
      active_end_hour: 18,
    });
  });
});

describe("ReplyPollingSection 回复时段界面", () => {
  it("打开开关后显示小时格和当前摘要", () => {
    render(<ReplyPollingSection config={config()} onChange={vi.fn()} />);

    expect(screen.getByLabelText(/回复时段 09:00/)).toBeTruthy();
    expect(screen.getByText("当前：09:00-22:00 回复")).toBeTruthy();
  });

  it("快捷预设一次性写回起止小时", () => {
    const onChange = vi.fn();
    render(<ReplyPollingSection config={config()} onChange={onChange} />);

    fireEvent.click(screen.getByRole("button", { name: "夜间 22-08" }));

    expect(onChange).toHaveBeenCalledWith({
      active_start_hour: 22,
      active_end_hour: 8,
    });
  });
});
