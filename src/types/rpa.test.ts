import { describe, expect, it } from "vitest";
import type { JobTaskInfo, JobTaskState, PlatformKind } from "./rpa";
import {
  buildPeriodicPlan,
  countJobTasks,
  describePeriodicDelivery,
  describePeriodicPlan,
  formatMinuteOfDay,
  getActiveTaskForPlatform,
  isActiveJobTask,
  isLongRunningMode,
} from "./rpa";
import {
  DEFAULT_PERIODIC_DELIVERY_CONFIG,
  HOURS_PER_DAY,
  hoursToWindows,
  isSamePeriodicDelivery,
  windowsToHours,
  type PeriodicDeliveryConfig,
} from "./app-config";

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

describe("periodic delivery plan", () => {
  it("renders minute-of-day as wall clock time", () => {
    expect(formatMinuteOfDay(0)).toBe("00:00");
    expect(formatMinuteOfDay(9 * 60 + 30)).toBe("09:30");
    expect(formatMinuteOfDay(24 * 60)).toBe("24:00");
  });

  // 关掉的开关必须落成 null / 0，后端才会按「不限制」处理。塞一个装作没启用的
  // 值进去，任务跑起来就会莫名其妙受一个用户没设过的时段约束
  it("omits the window and deadline when neither is enabled", () => {
    const plan = buildPeriodicPlan(DEFAULT_PERIODIC_DELIVERY_CONFIG);

    expect(plan.windows).toEqual([]);
    expect(plan.run_until).toBeNull();
    expect(plan.interval_minutes).toBe(30);
    expect(plan.max_greets_per_round).toBe(30);
  });

  // 午休那两小时正是多段存在的理由，得原样送到后端
  it("carries every window through when the schedule is enabled", () => {
    const plan = buildPeriodicPlan({
      ...DEFAULT_PERIODIC_DELIVERY_CONFIG,
      window_enabled: true,
      windows: [
        { start_minute: 9 * 60, end_minute: 12 * 60 },
        { start_minute: 14 * 60, end_minute: 18 * 60 },
      ],
    });

    expect(plan.windows).toEqual([
      { start_minute: 540, end_minute: 720 },
      { start_minute: 840, end_minute: 1080 },
    ]);
  });

  // 表单存的是「跑 N 小时」，而任务判断该不该收工只能靠绝对时刻。
  // 换算基准是点下确认的那一刻，所以这里把 now 显式传进去钉死
  it("converts the run duration into an absolute deadline", () => {
    const now = new Date("2026-08-19T10:00:00+08:00");

    const plan = buildPeriodicPlan(
      { ...DEFAULT_PERIODIC_DELIVERY_CONFIG, max_run_hours: 8 },
      now,
    );

    expect(plan.run_until).toBe(new Date("2026-08-19T18:00:00+08:00").toISOString());
  });

  // 摘要只说约束，不说默认。把「全天 · 不自动结束 · 不限条数」一并铺出来，
  // 真正被限制住的那几项反而看不见了
  it("summarises only the constraints that are actually set", () => {
    const summary = describePeriodicPlan({
      interval_minutes: 30,
      windows: [],
      run_until: null,
      max_greets_per_round: 0,
      max_round_minutes: 0,
    });

    expect(summary).toBe("每 30 分钟一轮");
  });

  it("summarises the window, deadline and round limits together", () => {
    const summary = describePeriodicPlan({
      interval_minutes: 30,
      windows: [
        { start_minute: 9 * 60, end_minute: 12 * 60 },
        { start_minute: 14 * 60, end_minute: 18 * 60 },
      ],
      run_until: new Date(2026, 7, 21, 2, 0).toISOString(),
      max_greets_per_round: 30,
      max_round_minutes: 60,
    });

    expect(summary).toContain("每 30 分钟一轮");
    expect(summary).toContain("09:00-12:00、14:00-18:00 投递");
    expect(summary).toContain("至 08-21 02:00 结束");
    expect(summary).toContain("单轮至多 30 条");
    expect(summary).toContain("单轮至多 60 分钟");
  });

  // 后端只带间隔的旧计划照样要能显示，不能因为缺字段就渲染出 undefined
  it("summarises a plan that only carries an interval", () => {
    expect(describePeriodicPlan({ interval_minutes: 45 })).toBe("每 45 分钟一轮");
  });
});

describe("periodic delivery form summary", () => {
  // 折叠起来时这行摘要是配置的唯一出口，默认值也必须说清楚单轮护栏还在
  it("summarises the default form without inventing constraints", () => {
    expect(describePeriodicDelivery(DEFAULT_PERIODIC_DELIVERY_CONFIG)).toBe(
      "每 30 分钟一轮 · 单轮至多 30 条 · 单轮至多 60 分钟",
    );
  });

  // 表单形态还没启动，说「8 小时后结束」才对得上刚拖的那个滑块；
  // 换算成绝对时刻是 buildPeriodicPlan 在提交那一刻的事
  it("states the run limit as a duration rather than a wall clock time", () => {
    const summary = describePeriodicDelivery({
      ...DEFAULT_PERIODIC_DELIVERY_CONFIG,
      window_enabled: true,
      max_run_hours: 8,
    });

    expect(summary).toContain("09:00-18:00 投递");
    expect(summary).toContain("8 小时后结束");
  });

  it("drops every constraint that is switched off", () => {
    expect(
      describePeriodicDelivery({
        interval_minutes: 20,
        window_enabled: false,
        windows: [{ start_minute: 9 * 60, end_minute: 18 * 60 }],
        max_run_hours: 0,
        max_greets_per_round: 0,
        max_round_minutes: 0,
      }),
    ).toBe("每 20 分钟一轮");
  });
});

describe("小时格与时段列表互转", () => {
  const hoursOf = (...ranges: Array<[number, number]>) =>
    Array.from({ length: HOURS_PER_DAY }, (_, hour) =>
      ranges.some(([from, to]) => hour >= from && hour < to),
    );

  // 这是这次需求的核心：09-12 与 14-18 之间的午休必须真的留空
  it("把带缺口的格子合并成两段", () => {
    expect(hoursToWindows(hoursOf([9, 12], [14, 18]))).toEqual([
      { start_minute: 9 * 60, end_minute: 12 * 60 },
      { start_minute: 14 * 60, end_minute: 18 * 60 },
    ]);
  });

  it("连续的格子只合并成一段", () => {
    expect(hoursToWindows(hoursOf([9, 18]))).toEqual([
      { start_minute: 9 * 60, end_minute: 18 * 60 },
    ]);
  });

  // 全选等于不限制。塞一个 00:00-24:00 给后端只会让每一步判断都多绕一次
  it("全选折叠成空数组，全不选也是空数组", () => {
    expect(hoursToWindows(hoursOf([0, 24]))).toEqual([]);
    expect(hoursToWindows(hoursOf())).toEqual([]);
  });

  it("末尾那格收在 24:00 而不是溢出到次日", () => {
    expect(hoursToWindows(hoursOf([22, 24]))).toEqual([
      { start_minute: 22 * 60, end_minute: 24 * 60 },
    ]);
  });

  it("时段列表摊回格子", () => {
    expect(
      windowsToHours([
        { start_minute: 9 * 60, end_minute: 12 * 60 },
        { start_minute: 14 * 60, end_minute: 18 * 60 },
      ]),
    ).toEqual(hoursOf([9, 12], [14, 18]));
  });

  // 跨零点的段在格子上就是两头都亮，不需要单独的表示法
  it("跨零点的段摊成首尾两截", () => {
    expect(windowsToHours([{ start_minute: 22 * 60, end_minute: 6 * 60 }])).toEqual(
      hoursOf([0, 6], [22, 24]),
    );
  });

  // 半点边界只能向外取整——这是格子粒度的取舍，得钉住免得日后被当成 bug 改坏
  it("半点边界向外取整到整点格", () => {
    expect(windowsToHours([{ start_minute: 9 * 60 + 30, end_minute: 17 * 60 + 30 }])).toEqual(
      hoursOf([9, 18]),
    );
  });

  it("空列表摊成一格都不亮", () => {
    expect(windowsToHours([])).toEqual(hoursOf());
  });

  it("多段经一轮往返后保持不变", () => {
    const windows = [
      { start_minute: 8 * 60, end_minute: 11 * 60 },
      { start_minute: 13 * 60, end_minute: 17 * 60 },
      { start_minute: 20 * 60, end_minute: 22 * 60 },
    ];

    expect(hoursToWindows(windowsToHours(windows))).toEqual(windows);
  });
});

describe("periodic delivery equality", () => {
  // 「恢复默认」按钮靠它置灰。用 JSON.stringify 比会依赖键顺序，
  // 两份内容相同、来源不同的对象被判成不等，按钮就永远亮着
  it("ignores key order when comparing two equivalent configs", () => {
    const reordered = {
      max_round_minutes: DEFAULT_PERIODIC_DELIVERY_CONFIG.max_round_minutes,
      interval_minutes: DEFAULT_PERIODIC_DELIVERY_CONFIG.interval_minutes,
      max_greets_per_round: DEFAULT_PERIODIC_DELIVERY_CONFIG.max_greets_per_round,
      max_run_hours: DEFAULT_PERIODIC_DELIVERY_CONFIG.max_run_hours,
      window_enabled: DEFAULT_PERIODIC_DELIVERY_CONFIG.window_enabled,
      // 内容相同但引用不同的数组，必须按内容判等
      windows: DEFAULT_PERIODIC_DELIVERY_CONFIG.windows.map((window) => ({ ...window })),
    };

    expect(isSamePeriodicDelivery(DEFAULT_PERIODIC_DELIVERY_CONFIG, reordered)).toBe(true);
  });

  const changes: Array<[string, Partial<PeriodicDeliveryConfig>]> = [
    ["interval_minutes", { interval_minutes: 45 }],
    ["window_enabled", { window_enabled: true }],
    ["windows", { windows: [{ start_minute: 10 * 60, end_minute: 12 * 60 }] }],
    ["windows 段数", {
      windows: [
        { start_minute: 9 * 60, end_minute: 12 * 60 },
        { start_minute: 14 * 60, end_minute: 18 * 60 },
      ],
    }],
    ["max_run_hours", { max_run_hours: 8 }],
    ["max_greets_per_round", { max_greets_per_round: 10 }],
    ["max_round_minutes", { max_round_minutes: 90 }],
  ];

  it.each(changes)("detects a change to %s", (_field, patch) => {
    expect(
      isSamePeriodicDelivery(DEFAULT_PERIODIC_DELIVERY_CONFIG, {
        ...DEFAULT_PERIODIC_DELIVERY_CONFIG,
        ...patch,
      }),
    ).toBe(false);
  });
});
