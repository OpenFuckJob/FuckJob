import { useState, type ReactNode } from "react";
import { Slider, Switch } from "antd";
import { NumberField } from "./NumberField";

/**
 * 配置页的滑块 / 开关行。
 *
 * 刻意不给滑块画刻度：轨道下方铺一排数字会把「可拖动」这件事伪装成「只能选这几档」，
 * 而且刻度密了看不清、疏了对不准。当前值统一交给右侧的输入框——它同时还是精确输入口，
 * 拖到大概位置再改两位数字，比对着刻度描准要快。
 */

interface FieldHeaderProps {
  icon?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  /** 描述下方的动态提示，用于越界、冲突这类需要即时反馈的情况 */
  hint?: ReactNode;
}

function FieldHeader({ icon, title, description, hint }: FieldHeaderProps) {
  return (
    <div className="flex min-w-0 items-start gap-3">
      {icon && (
        <span className="mt-px flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-sky-50 text-sky-500 transition-colors group-hover:bg-sky-100">
          {icon}
        </span>
      )}
      <div className="min-w-0">
        <div className="text-sm font-semibold leading-6 text-slate-900">
          {title}
        </div>
        {description && (
          <div className="mt-0.5 text-xs leading-relaxed text-slate-500">
            {description}
          </div>
        )}
        {hint && (
          <div className="mt-1 text-xs leading-relaxed text-amber-600">
            {hint}
          </div>
        )}
      </div>
    </div>
  );
}

/** 若干设置行的容器，行与行之间用细分隔线而不是各自描边，避免嵌套出一堆圆角框 */
export function SettingGroup({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={`divide-y divide-slate-100 overflow-hidden rounded-xl border border-slate-200/80 bg-white ${className}`}
    >
      {children}
    </div>
  );
}

/** 组内小节的标题行，本身不带控件，用来给下面几行滑块一个共同的抬头 */
export function SettingHeading({
  icon,
  title,
  description,
  hint,
}: FieldHeaderProps) {
  return (
    // 抬头没有右侧控件占位，不限宽的话长描述会一路铺到滑块列上方
    <div className="max-w-3xl px-4 py-3.5">
      <FieldHeader
        icon={icon}
        title={title}
        description={description}
        hint={hint}
      />
    </div>
  );
}

interface SettingToggleProps extends FieldHeaderProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  /** 开关下方的展开区，与标题文字左对齐 */
  children?: ReactNode;
}

export function SettingToggle({
  icon,
  title,
  description,
  hint,
  checked,
  onChange,
  disabled,
  children,
}: SettingToggleProps) {
  return (
    <div className="group px-4 py-3.5 transition-colors hover:bg-slate-50/60">
      <div className="flex items-center justify-between gap-6">
        <FieldHeader
          icon={icon}
          title={title}
          description={description}
          hint={hint}
        />
        <Switch
          aria-label={typeof title === "string" ? title : undefined}
          checked={checked}
          disabled={disabled}
          onChange={onChange}
        />
      </div>
      {children && (
        <div className={`mt-4 ${icon ? "sm:pl-11" : ""}`}>{children}</div>
      )}
    </div>
  );
}

interface SettingSliderProps extends FieldHeaderProps {
  value: number | null | undefined;
  onChange: (value: number) => void;
  min: number;
  max: number;
  /**
   * 滑块的拖动上限，默认与 `max` 相同。
   * 上限远大于日常取值时（例如上限 500、常用 20）设成常用区间的边界，
   * 否则整条轨道有效行程都挤在最左边一小截里；更大的值仍可从输入框填。
   */
  sliderMax?: number;
  step?: number;
  /** 输入框被清空或停在越界值时，失焦后回落到的值 */
  fallback: number;
  unit?: string;
  /**
   * 覆盖特定取值的读法，例如 0 表示不限制。
   * 返回 null 时按 `unit` 正常显示。
   */
  valueLabel?: (value: number) => string | null;
  disabled?: boolean;
  /** 归属于上方 SettingHeading 时缩进，让标题与抬头文字对齐 */
  inset?: boolean;
}

export function SettingSlider({
  icon,
  title,
  description,
  hint,
  value,
  onChange,
  min,
  max,
  sliderMax,
  step = 1,
  fallback,
  unit,
  valueLabel,
  disabled,
  inset,
}: SettingSliderProps) {
  // 拖动过程中的值只留在本地，松手才写回配置：整份配置挂在顶层 state 上，
  // 每移动一像素就提交一次会让整个设置页跟着重渲染。
  const [dragging, setDragging] = useState<number | null>(null);
  const settled = value ?? fallback;
  const current = dragging ?? settled;
  const label = valueLabel?.(current) ?? null;
  const ariaTitle = typeof title === "string" ? title : undefined;
  // 已经填到常用区间之外时把轨道让出来，否则滑块会被钉在最右端动不了
  const trackMax = Math.min(Math.max(sliderMax ?? max, current), max);

  return (
    // 按行自身的宽度决定横排还是堆叠：设置面板嵌在侧边栏右侧，视口断点量不准它
    <div className="group @container px-4 py-3.5 transition-colors hover:bg-slate-50/60">
      <div className="flex flex-col gap-3 @2xl:flex-row @2xl:items-center @2xl:gap-6">
        <div
          className={`@2xl:w-[46%] @2xl:max-w-lg @2xl:shrink-0 ${inset ? "sm:pl-11" : ""}`}
        >
          <FieldHeader
            icon={icon}
            title={title}
            description={description}
            hint={hint}
          />
        </div>
        <div
          className={`flex flex-1 items-center gap-4 ${icon || inset ? "pl-11 @2xl:pl-0" : ""}`}
        >
          <Slider
            className="my-0! min-w-0 flex-1"
            min={min}
            max={trackMax}
            step={step}
            value={current}
            disabled={disabled}
            ariaLabelForHandle={ariaTitle}
            tooltip={{
              formatter: (v) =>
                (v !== undefined && valueLabel?.(v)) || `${v}${unit ?? ""}`,
            }}
            onChange={setDragging}
            onChangeComplete={(next) => {
              onChange(next);
              setDragging(null);
            }}
          />
          <div className="flex w-33 shrink-0 items-center gap-2">
            <NumberField
              aria-label={ariaTitle && `${ariaTitle}（输入数值）`}
              size="small"
              min={min}
              max={max}
              step={1}
              precision={0}
              disabled={disabled}
              fallback={fallback}
              value={current}
              style={{ width: 72 }}
              onChange={(next) => {
                setDragging(null);
                onChange(next);
              }}
            />
            <span
              className={`truncate text-xs ${label ? "font-medium text-sky-600" : "text-slate-400"}`}
            >
              {label ?? unit}
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
