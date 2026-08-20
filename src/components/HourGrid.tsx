import { useEffect, useRef, useState } from "react";
import { HOURS_PER_DAY } from "@/types/app-config";

interface DragState {
  from: number;
  to: number;
  /** 这一笔是在涂上还是在擦掉，由按下那一格的当前状态决定 */
  paint: boolean;
}

interface Props {
  /** 24 个小时格的选中状态，第 h 项代表 `[h:00, h+1:00)` */
  value: boolean[];
  onChange: (next: boolean[]) => void;
  disabled?: boolean;
  /** 无障碍标签前缀，用于拼每一格的 aria-label */
  label?: string;
}

/** 把一笔涂抹应用到格子上 */
export function applyDrag(hours: boolean[], drag: DragState): boolean[] {
  const from = Math.min(drag.from, drag.to);
  const to = Math.max(drag.from, drag.to);
  return hours.map((on, hour) => (hour >= from && hour <= to ? drag.paint : on));
}

/**
 * 24 小时选择格。
 *
 * 一天摊成 24 格，点一格切换、按住横扫连选。相比两个时间下拉，它的价值在于
 * 一眼看得出全天的分布——尤其是「上午投、午休停、下午再投」这种带缺口的安排，
 * 用起止时间描述要读两遍才明白，摊成格子是一目了然的。
 *
 * 代价是粒度只到整点，09:30 这种半点边界表达不了。
 */
export default function HourGrid({ value, onChange, disabled, label = "投递时段" }: Props) {
  const [drag, setDrag] = useState<DragState | null>(null);
  // 鼠标松开之后浏览器还会补发一次 click。不记下这一笔的来路，
  // 单击一格就会被 mouseup 和 click 各切一次，最终回到原样、看着像点不动
  const mouseDriven = useRef(false);
  const preview = drag ? applyDrag(value, drag) : value;

  // 拖到格子外面松手同样要结算，否则那一笔会一直挂着，
  // 鼠标再移回来会接着涂——看起来就像控件自己在动
  useEffect(() => {
    if (!drag) return;
    const commit = () => {
      onChange(applyDrag(value, drag));
      setDrag(null);
    };
    window.addEventListener("mouseup", commit);
    return () => window.removeEventListener("mouseup", commit);
  }, [drag, onChange, value]);

  return (
    <div className="select-none">
      <div className="grid grid-cols-24 gap-px">
        {Array.from({ length: HOURS_PER_DAY }, (_, hour) => {
          const on = preview[hour] === true;
          return (
            <button
              key={hour}
              type="button"
              disabled={disabled}
              aria-pressed={on}
              aria-label={`${label} ${String(hour).padStart(2, "0")}:00 至 ${String(hour + 1).padStart(2, "0")}:00`}
              className={`h-7 rounded-[3px] transition-colors ${
                on ? "bg-sky-500 hover:bg-sky-600" : "bg-slate-200 hover:bg-slate-300"
              } ${disabled ? "cursor-not-allowed opacity-50" : "cursor-pointer"}`}
              onMouseDown={() => {
                if (disabled) return;
                mouseDriven.current = true;
                setDrag({ from: hour, to: hour, paint: !value[hour] });
              }}
              onMouseEnter={() =>
                setDrag((current) => (current ? { ...current, to: hour } : null))
              }
              // 键盘的 Enter/Space 也走 click，这条路径才是它唯一的入口
              onClick={() => {
                if (disabled) return;
                if (mouseDriven.current) {
                  mouseDriven.current = false;
                  return;
                }
                onChange(value.map((state, index) => (index === hour ? !state : state)));
              }}
            />
          );
        })}
      </div>
      <div className="mt-1 flex justify-between text-[10px] text-slate-400 tabular-nums">
        <span>00</span>
        <span>06</span>
        <span>12</span>
        <span>18</span>
        <span>24</span>
      </div>
    </div>
  );
}
