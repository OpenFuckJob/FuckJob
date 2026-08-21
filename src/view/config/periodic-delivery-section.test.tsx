import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import PeriodicDeliverySection, {
  canResetPeriodicDelivery,
} from "./PeriodicDeliverySection";
import {
  DEFAULT_PERIODIC_DELIVERY_CONFIG,
  type PeriodicDeliveryConfig,
} from "../../types/app-config";

// vitest 没开 globals，testing-library 的自动清理不会注册，
// 上一个用例的 DOM 会留到下一个用例里造成重复匹配
afterEach(cleanup);

function config(
  overrides: Partial<PeriodicDeliveryConfig> = {},
): PeriodicDeliveryConfig {
  return { ...DEFAULT_PERIODIC_DELIVERY_CONFIG, ...overrides };
}

describe("PeriodicDeliverySection 字段", () => {
  it.each([
    "投递间隔",
    "单轮最多打招呼",
    "单轮最长运行",
    "只在指定时段投递",
    "自动结束",
  ])("渲染出「%s」", (label) => {
    render(<PeriodicDeliverySection config={config()} onChange={vi.fn()} />);

    expect(screen.getByText(label)).toBeTruthy();
  });

  // 时段格子只在开关打开时才有意义，关着还占一片会让人以为它在生效
  it("时段格子跟随开关出现", () => {
    const { rerender } = render(
      <PeriodicDeliverySection config={config()} onChange={vi.fn()} />,
    );
    expect(screen.queryByLabelText(/投递时段 09:00/)).toBeNull();

    rerender(
      <PeriodicDeliverySection
        config={config({ window_enabled: true })}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByLabelText(/投递时段 09:00/)).toBeTruthy();
  });

  // 09-12 与 14-18 之间的午休正是多段存在的理由，摘要必须把缺口说出来
  it("摘要写出全部时段而不是只写首尾", () => {
    render(
      <PeriodicDeliverySection
        config={config({
          window_enabled: true,
          windows: [
            { start_minute: 9 * 60, end_minute: 12 * 60 },
            { start_minute: 14 * 60, end_minute: 18 * 60 },
          ],
        })}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByText("当前：09:00-12:00、14:00-18:00")).toBeTruthy();
  });

  /**
   * 「开了时段限制却一格没选」是个安静的陷阱：任务能入队、能跑，
   * 但永远等不到可投的时刻，日志上只有一句「暂停投递」然后再无下文
   */
  it("一格都没选时给出警告而不是静默通过", () => {
    render(
      <PeriodicDeliverySection
        config={config({ window_enabled: true, windows: [] })}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByText(/一格都没选，任务不会投递/)).toBeTruthy();
  });

  it("点预设一次填好带缺口的两段", () => {
    const onChange = vi.fn();
    render(
      <PeriodicDeliverySection
        config={config({ window_enabled: true })}
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "上下午 9-12 / 14-18" }));

    expect(onChange).toHaveBeenCalledWith({
      windows: [
        { start_minute: 9 * 60, end_minute: 12 * 60 },
        { start_minute: 14 * 60, end_minute: 18 * 60 },
      ],
    });
  });
});

describe("PeriodicDeliverySection 形态", () => {
  /**
   * 抽屉自己的标题栏和 footer 已经承担了标题与「恢复默认」的职责，
   * plain 形态再出一份就是两层壳
   */
  it("plain 形态不带外框标题与恢复按钮", () => {
    render(
      <PeriodicDeliverySection
        variant="plain"
        config={config()}
        resetTo={DEFAULT_PERIODIC_DELIVERY_CONFIG}
        onChange={vi.fn()}
      />,
    );

    expect(screen.queryByText("周期投递")).toBeNull();
    expect(screen.queryByRole("button", { name: /恢复/ })).toBeNull();
    // 字段本身照常渲染
    expect(screen.getByText("投递间隔")).toBeTruthy();
  });

  it("card 形态带标题，没给基准时仍不渲染恢复按钮", () => {
    render(<PeriodicDeliverySection config={config()} onChange={vi.fn()} />);

    expect(screen.getByText("周期投递")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /恢复/ })).toBeNull();
  });
});

describe("PeriodicDeliverySection 恢复默认", () => {
  it("当前值就是基准时按钮禁用", () => {
    render(
      <PeriodicDeliverySection
        config={config()}
        resetTo={DEFAULT_PERIODIC_DELIVERY_CONFIG}
        onChange={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("button", { name: /恢复默认/ }).hasAttribute("disabled"),
    ).toBe(true);
  });

  /**
   * 一次性还原整份配置，而不是逐字段发若干次 onChange：
   * 调用方普遍用 `{...current, ...next}` 合并，分批发会在中间态上重渲染
   */
  it("改动过之后一次性还原整份配置", () => {
    const onChange = vi.fn();
    render(
      <PeriodicDeliverySection
        config={config({ interval_minutes: 90, window_enabled: true })}
        resetTo={DEFAULT_PERIODIC_DELIVERY_CONFIG}
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /恢复默认/ }));

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(DEFAULT_PERIODIC_DELIVERY_CONFIG);
  });

  /**
   * 两处的「默认」不是同一件事：配置页恢复出厂值，启动弹窗的抽屉恢复的是用户
   * 自己在配置页存下的那套。基准由调用方给，组件不能自作主张回落到出厂值
   */
  it("基准由调用方决定，不写死出厂默认", () => {
    const onChange = vi.fn();
    const mine = config({ interval_minutes: 60, max_greets_per_round: 15 });
    render(
      <PeriodicDeliverySection
        config={config({ interval_minutes: 5 })}
        resetTo={mine}
        resetLabel="恢复我的默认"
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /恢复我的默认/ }));

    expect(onChange).toHaveBeenCalledWith(mine);
  });

  // 抽屉的 footer 自己渲染按钮，得靠同一个判定决定置灰，不然两处会给出不同答案
  it("导出的判定与按钮禁用状态是同一套", () => {
    expect(
      canResetPeriodicDelivery(config(), DEFAULT_PERIODIC_DELIVERY_CONFIG),
    ).toBe(false);
    expect(
      canResetPeriodicDelivery(
        config({ interval_minutes: 90 }),
        DEFAULT_PERIODIC_DELIVERY_CONFIG,
      ),
    ).toBe(true);
  });
});
