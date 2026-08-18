import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { NumberField, settleNumber } from "./NumberField";

function Harness({
  onChange,
  initial = 30,
  min = 1,
  max = 1440,
  fallback = 30,
}: {
  onChange: (value: number) => void;
  initial?: number;
  min?: number;
  max?: number;
  fallback?: number;
}) {
  const [value, setValue] = useState(initial);
  return (
    <NumberField
      aria-label="间隔"
      min={min}
      max={max}
      precision={0}
      fallback={fallback}
      value={value}
      onChange={(next) => {
        setValue(next);
        onChange(next);
      }}
    />
  );
}

const intervalInput = () => screen.getByLabelText("间隔") as HTMLInputElement;

describe("NumberField", () => {
  afterEach(cleanup);

  it("允许清空输入框重新输入，不会被默认值抢先回填", () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    const input = intervalInput();

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "3" } });
    expect(onChange).toHaveBeenLastCalledWith(3);

    // 删掉最后一位：输入框保持为空，上层拿到的仍是 3，而不是被 30 覆盖
    fireEvent.change(input, { target: { value: "" } });
    expect(input.value).toBe("");
    expect(onChange).toHaveBeenLastCalledWith(3);

    fireEvent.change(input, { target: { value: "2" } });
    expect(onChange).toHaveBeenLastCalledWith(2);
  });

  it("失焦时空值回落到 fallback", () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    const input = intervalInput();

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "2" } });
    expect(onChange).toHaveBeenLastCalledWith(2);

    fireEvent.change(input, { target: { value: "" } });
    fireEvent.blur(input);

    expect(onChange).toHaveBeenLastCalledWith(30);
    expect(input.value).toBe("30");
  });

  it("低于最小值的中间态不上报，失焦时夹紧", () => {
    const onChange = vi.fn();
    render(
      <Harness onChange={onChange} initial={1000} min={100} max={10000} fallback={100} />,
    );
    const input = intervalInput();

    fireEvent.focus(input);
    // 想输入 200，先打出的 "2" 低于 min，不应写回配置
    fireEvent.change(input, { target: { value: "2" } });
    expect(onChange).not.toHaveBeenCalled();

    fireEvent.change(input, { target: { value: "200" } });
    expect(onChange).toHaveBeenLastCalledWith(200);

    fireEvent.change(input, { target: { value: "5" } });
    fireEvent.blur(input);
    expect(onChange).toHaveBeenLastCalledWith(100);
  });

  it("外部值变化在非编辑态下同步到输入框", () => {
    const { rerender } = render(
      <NumberField aria-label="间隔" min={1} max={1440} fallback={30} value={30} />,
    );
    expect(intervalInput().value).toBe("30");

    rerender(
      <NumberField aria-label="间隔" min={1} max={1440} fallback={30} value={45} />,
    );
    expect(intervalInput().value).toBe("45");
  });
});

describe("settleNumber", () => {
  it("空值回落到 fallback", () => {
    expect(settleNumber(null, { fallback: 30 })).toBe(30);
    expect(settleNumber(undefined, { fallback: 30 })).toBe(30);
    expect(settleNumber(Number.NaN, { fallback: 30 })).toBe(30);
  });

  it("越界值夹紧到区间内", () => {
    expect(settleNumber(0, { fallback: 30, min: 1, max: 1440 })).toBe(1);
    expect(settleNumber(9999, { fallback: 30, min: 1, max: 1440 })).toBe(1440);
    expect(settleNumber(2, { fallback: 30, min: 1, max: 1440 })).toBe(2);
  });

  it("按精度取整", () => {
    expect(settleNumber(2.6, { fallback: 30, precision: 0 })).toBe(3);
    expect(settleNumber(2.64, { fallback: 30, precision: 1 })).toBe(2.6);
  });
});
