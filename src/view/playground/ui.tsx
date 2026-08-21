import type { ReactNode } from "react";
import { stageSymbol, type PlaygroundStage, type StageMeta } from "@/types/playground";

/**
 * 测试模式里所有内容块的统一外壳。
 *
 * 每一步只摆两三张这样的卡，卡与卡之间靠标题分段——这是这次重构的前提：
 * 原来一屏塞下全部控件时，块与块只靠一条细分隔线区分，看上去像一张长表单
 */
export function Section({
  title,
  hint,
  extra,
  children,
}: {
  title: string;
  hint?: string;
  extra?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="rounded-2xl border border-slate-200/80 bg-white/85 p-5">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-sm font-semibold text-slate-900">{title}</div>
          {hint && <div className="mt-0.5 text-xs leading-5 text-slate-500">{hint}</div>}
        </div>
        {extra && <div className="shrink-0">{extra}</div>}
      </div>
      {children}
    </section>
  );
}

/** 带标签的表单行，标签右侧可挂一句灰字提示 */
export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <label className="block">
      <span className="mb-1 flex items-baseline gap-2">
        <span className="text-xs font-medium text-slate-600">{label}</span>
        {hint && <span className="text-[11px] text-slate-400">{hint}</span>}
      </span>
      {children}
    </label>
  );
}

export type StageTone = "pass" | "block" | "skip" | "idle";

const TRAIL_TONE: Record<StageTone, string> = {
  pass: "border-emerald-200 bg-emerald-50 text-emerald-700",
  block: "border-rose-300 bg-rose-50 text-rose-700",
  skip: "border-slate-200 bg-slate-100 text-slate-500",
  idle: "border-slate-200 bg-slate-50 text-slate-400",
};

/**
 * 一条链路会经过的环节，横着排成一串。
 *
 * 选链路时它是预告，跑完后同一串染上状态就成了断点示意图——两处用同一种画法，
 * 用户不用在两个界面之间重新认一遍这条链路有几步
 */
export function StageTrail({
  stages,
  tones,
}: {
  stages: StageMeta[];
  tones?: Partial<Record<PlaygroundStage, StageTone>>;
}) {
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {stages.map((meta, index) => (
        <span key={meta.stage} className="flex items-center gap-1.5">
          {index > 0 && <span className="text-[10px] text-slate-300">→</span>}
          <span
            className={`rounded-md border px-2 py-0.5 text-[11px] leading-5 ${
              TRAIL_TONE[tones?.[meta.stage] ?? "idle"]
            }`}
          >
            {stageSymbol(meta.order)} {meta.label}
          </span>
        </span>
      ))}
    </div>
  );
}
