import { useEffect, useState, type FocusEventHandler } from "react";
import { InputNumber } from "antd";
import type { InputNumberProps } from "antd";

type NumberBounds = {
  fallback: number;
  min?: number;
  max?: number;
  precision?: number;
};

/** 把编辑中间态（空值 / 越界值）收敛成一个可以写回配置的合法数字。 */
export function settleNumber(
  raw: number | null | undefined,
  { fallback, min, max, precision }: NumberBounds,
): number {
  const base =
    raw === null || raw === undefined || Number.isNaN(raw) ? fallback : raw;
  const rounded =
    precision === undefined ? base : Number(base.toFixed(precision));
  const floored = min === undefined ? rounded : Math.max(rounded, min);
  return max === undefined ? floored : Math.min(floored, max);
}

export function isSettled(
  raw: number | null | undefined,
  { min, max }: Pick<NumberBounds, "min" | "max">,
): raw is number {
  if (raw === null || raw === undefined || Number.isNaN(raw)) return false;
  if (min !== undefined && raw < min) return false;
  if (max !== undefined && raw > max) return false;
  return true;
}

export type NumberFieldProps = Omit<
  InputNumberProps<number>,
  "value" | "onChange" | "min" | "max"
> & {
  value?: number | null;
  onChange?: (value: number) => void;
  /** 输入框被清空或停在越界值时，失焦后回落到的值 */
  fallback: number;
  min?: number;
  max?: number;
};

/**
 * 数字输入框：允许「清空」「暂时低于最小值」这类编辑中间态。
 *
 * 直接用受控的 InputNumber 时，删掉最后一位会触发 onChange(null)，上层若立刻
 * 用默认值回填，输入框会瞬间被写回旧值，表现为「最后一个数字删不掉」。
 * 这里把编辑期的草稿留在组件内部，只有合法值才向上提交，失焦时再收敛。
 */
export function NumberField({
  value,
  onChange,
  fallback,
  min,
  max,
  precision,
  onFocus,
  onBlur,
  ...rest
}: NumberFieldProps) {
  const [draft, setDraft] = useState<number | null>(value ?? fallback);
  const [editing, setEditing] = useState(false);

  useEffect(() => {
    if (!editing) setDraft(value ?? fallback);
  }, [value, fallback, editing]);

  const submit = (next: number) => {
    setDraft(next);
    if (next !== value) onChange?.(next);
  };

  const handleFocus: FocusEventHandler<HTMLInputElement> = (event) => {
    setEditing(true);
    onFocus?.(event);
  };

  const handleChange = (next: number | null) => {
    setDraft(next);
    if (isSettled(next, { min, max })) submit(next);
  };

  const handleBlur: FocusEventHandler<HTMLInputElement> = (event) => {
    setEditing(false);
    submit(settleNumber(draft, { fallback, min, max, precision }));
    onBlur?.(event);
  };

  return (
    <InputNumber
      {...rest}
      min={min}
      max={max}
      precision={precision}
      value={editing ? draft : value ?? fallback}
      onFocus={handleFocus}
      onChange={handleChange}
      onBlur={handleBlur}
    />
  );
}
