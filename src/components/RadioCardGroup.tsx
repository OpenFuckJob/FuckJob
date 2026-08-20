import { Radio } from "antd";

/**
 * 单选卡片组：一组互斥选项，每项一张可点的卡片。
 *
 * 标签和描述排在同一行，描述长了自动折行——不必为长短文案各拆一种布局。
 * 配置页里「选一个策略」的地方都用它，避免同一套卡片样式在各页面各写一遍，
 * 改动时漏掉其中一处就会出现两种深浅不一的选中态。
 */
export interface RadioCardOption<T extends string> {
  value: T;
  label: string;
  description: string;
  /** 标签后的推荐标记，一组里至多标一个 */
  recommended?: boolean;
  disabled?: boolean;
}

interface RadioCardGroupProps<T extends string> {
  /** 整组的无障碍名称，例如「回复策略」 */
  ariaLabel: string;
  value: T;
  options: RadioCardOption<T>[];
  onChange: (next: T) => void;
}

export function RadioCardGroup<T extends string>({
  ariaLabel,
  value,
  options,
  onChange,
}: RadioCardGroupProps<T>) {
  return (
    <Radio.Group
      aria-label={ariaLabel}
      value={value}
      onChange={(event) => onChange(event.target.value as T)}
      className="!block w-full"
    >
      <div className="space-y-2">
        {options.map((option) => (
          <Radio
            key={option.value}
            value={option.value}
            aria-label={option.label}
            disabled={option.disabled}
            className={`!m-0 !flex !w-full !items-center !gap-2.5 !rounded-lg !border !px-3 !py-2.5 transition-colors ${
              value === option.value
                ? "!border-sky-400 !bg-sky-50/60"
                : "!border-slate-200 !bg-white hover:!border-sky-200 hover:!bg-slate-50/60"
            } ${option.disabled ? "!cursor-not-allowed !bg-slate-50/70" : ""}`}
          >
            <span className="flex min-w-0 flex-wrap items-baseline gap-x-2">
              <span className="text-sm font-semibold leading-5 text-slate-900">
                {option.label}
              </span>
              {option.recommended && (
                <span className="rounded bg-sky-100 px-1 text-[10px] font-medium leading-4 text-sky-700">
                  推荐
                </span>
              )}
              <span className="text-xs leading-5 text-slate-500">
                {option.description}
              </span>
            </span>
          </Radio>
        ))}
      </div>
    </Radio.Group>
  );
}
