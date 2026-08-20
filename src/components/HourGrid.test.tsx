import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import HourGrid, { applyDrag } from "./HourGrid";
import { HOURS_PER_DAY } from "@/types/app-config";

afterEach(cleanup);

const hoursOf = (...ranges: Array<[number, number]>) =>
  Array.from({ length: HOURS_PER_DAY }, (_, hour) =>
    ranges.some(([from, to]) => hour >= from && hour < to),
  );

const cells = () => screen.getAllByRole("button");

/** 模拟一次真实的按下—横扫—松开，包括浏览器在 mouseup 之后补发的 click */
function dragAcross(from: number, to: number) {
  const grid = cells();
  fireEvent.mouseDown(grid[from]);
  for (let hour = from; hour !== to + Math.sign(to - from); hour += Math.sign(to - from) || 1) {
    fireEvent.mouseEnter(grid[hour]);
    if (hour === to) break;
  }
  fireEvent.mouseUp(window);
  fireEvent.click(grid[to]);
}

describe("applyDrag", () => {
  it("把一整段涂成同一个状态，与拖动方向无关", () => {
    const blank = hoursOf();

    expect(applyDrag(blank, { from: 9, to: 11, paint: true })).toEqual(hoursOf([9, 12]));
    expect(applyDrag(blank, { from: 11, to: 9, paint: true })).toEqual(hoursOf([9, 12]));
  });

  it("从已选格起笔时是擦除", () => {
    expect(applyDrag(hoursOf([9, 18]), { from: 12, to: 13, paint: false })).toEqual(
      hoursOf([9, 12], [14, 18]),
    );
  });
});

describe("HourGrid", () => {
  it("渲染 24 个格子并反映选中状态", () => {
    render(<HourGrid value={hoursOf([9, 12])} onChange={vi.fn()} />);

    const grid = cells();
    expect(grid).toHaveLength(HOURS_PER_DAY);
    expect(grid[9].getAttribute("aria-pressed")).toBe("true");
    expect(grid[11].getAttribute("aria-pressed")).toBe("true");
    expect(grid[12].getAttribute("aria-pressed")).toBe("false");
  });

  /**
   * 浏览器在 mouseup 之后还会补发一次 click。少了这道判断，单击一格会被
   * mouseup 和 click 各切一次，最终回到原样——用户看到的是「点不动」
   */
  it("单击一格只切换一次", () => {
    const onChange = vi.fn();
    render(<HourGrid value={hoursOf()} onChange={onChange} />);

    dragAcross(9, 9);

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(hoursOf([9, 10]));
  });

  it("横扫连选一整段", () => {
    const onChange = vi.fn();
    render(<HourGrid value={hoursOf()} onChange={onChange} />);

    dragAcross(9, 11);

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(hoursOf([9, 12]));
  });

  // 午休那两小时正是多段存在的理由：在已选区间中间擦掉一截就得到两段
  it("在已选区间中间擦出缺口", () => {
    const onChange = vi.fn();
    render(<HourGrid value={hoursOf([9, 18])} onChange={onChange} />);

    dragAcross(12, 13);

    expect(onChange).toHaveBeenCalledWith(hoursOf([9, 12], [14, 18]));
  });

  // 键盘走的是 click 而没有 mousedown，那条「跳过补发 click」的判断不能误伤它
  it("键盘触发的 click 照常切换", () => {
    const onChange = vi.fn();
    render(<HourGrid value={hoursOf()} onChange={onChange} />);

    fireEvent.click(cells()[3]);

    expect(onChange).toHaveBeenCalledWith(hoursOf([3, 4]));
  });

  it("禁用时不响应", () => {
    const onChange = vi.fn();
    render(<HourGrid value={hoursOf()} onChange={onChange} disabled />);

    fireEvent.click(cells()[3]);

    expect(onChange).not.toHaveBeenCalled();
  });
});
