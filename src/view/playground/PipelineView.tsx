import { useState, type ReactNode } from "react";
import {
  CheckCircleFilled,
  CloseCircleFilled,
  MinusCircleOutlined,
  RightOutlined,
} from "@ant-design/icons";
import {
  stagesOfChain,
  stageSymbol,
  type PlaygroundChain,
  type PlaygroundStage,
  type StepResult,
} from "@/types/playground";
import { summarizeDetail, toDetailFields } from "./detail";

type RowState = "pass" | "block" | "skip" | "idle";

const ROW_STYLE: Record<RowState, { icon: ReactNode; label: string; className: string }> = {
  pass: {
    icon: <CheckCircleFilled className="text-emerald-500" />,
    label: "通过",
    className: "border-emerald-200 bg-emerald-50/50",
  },
  block: {
    icon: <CloseCircleFilled className="text-rose-500" />,
    label: "拦下",
    className: "border-rose-300 bg-rose-50",
  },
  skip: {
    icon: <MinusCircleOutlined className="text-slate-400" />,
    label: "跳过",
    className: "border-slate-200 bg-white",
  },
  idle: {
    icon: <MinusCircleOutlined className="text-slate-200" />,
    label: "未执行",
    className: "border-slate-100 bg-slate-50/40",
  },
};

function DetailBody({ stage, detail }: { stage: PlaygroundStage; detail: unknown }) {
  const fields = toDetailFields(detail);
  if (fields.length === 0) {
    return <div className="px-3 py-2 text-xs text-slate-400">该环节没有返回结构化产物</div>;
  }
  return (
    <dl className="m-0 space-y-1.5 px-3 py-2" data-testid={`step-detail-${stage}`}>
      {fields.map((field) => (
        <div key={field.key} className={field.multiline ? "" : "flex gap-2"}>
          <dt className="shrink-0 text-xs font-medium text-slate-500">{field.key}</dt>
          <dd className="m-0 min-w-0 text-xs text-slate-700">
            {Array.isArray(field.value) ? (
              <ul className="m-0 list-disc pl-4">
                {field.value.map((item, index) => (
                  <li key={index} className="wrap-break-word">{item}</li>
                ))}
              </ul>
            ) : (
              <span className={field.multiline ? "block whitespace-pre-wrap wrap-break-word" : "wrap-break-word"}>
                {field.value}
              </span>
            )}
          </dd>
        </div>
      ))}
    </dl>
  );
}

export interface PipelineViewProps {
  steps: StepResult[];
  /** 只列这条链路的环节，别的链路没跑本来就不该出现在结果里 */
  chain: PlaygroundChain;
}

/**
 * 本条链路的环节固定全列出来。
 *
 * 只渲染「跑到的那几个」会让人误以为链路就这么长，看不出请求是在第几步断的；
 * 没跑到的保持灰态，配上被拦下环节的红底，用户扫一眼就知道断点在哪。
 * 但另外两条链路的环节不在这里凑数——它们压根没被触发，摆十行灰条只是噪音
 */
export function PipelineView({ steps, chain }: PipelineViewProps) {
  const [expanded, setExpanded] = useState<PlaygroundStage | null>(null);
  const executed = new Map(steps.map((step) => [step.stage, step]));

  return (
    <div className="space-y-1" data-testid="pipeline">
      {stagesOfChain(chain).map((meta) => {
        const step = executed.get(meta.stage);
        const state: RowState = step ? step.outcome.kind : "idle";
        const style = ROW_STYLE[state];
        const reason = step && step.outcome.kind !== "pass" ? step.outcome.reason : null;
        const summary = step ? summarizeDetail(meta.stage, step.detail) : null;
        const open = expanded === meta.stage;

        return (
          <div key={meta.stage} className={`rounded-lg border ${style.className}`}>
            <button
              type="button"
              aria-label={`${meta.order} ${meta.label}`}
              aria-expanded={open}
              disabled={!step}
              onClick={() => setExpanded(open ? null : meta.stage)}
              className={`flex w-full items-start gap-2 px-3 py-2 text-left ${step ? "cursor-pointer" : "cursor-default"}`}
            >
              <span className="w-4 shrink-0 text-sm leading-5 text-slate-400">
                {stageSymbol(meta.order)}
              </span>
              <span className="shrink-0 leading-5">{style.icon}</span>
              <span className="min-w-0 flex-1">
                <span className="flex flex-wrap items-baseline gap-x-2">
                  <span
                    className={`text-sm font-medium ${state === "idle" ? "text-slate-400" : "text-slate-800"}`}
                  >
                    {meta.label}
                  </span>
                  <span className="text-[11px] text-slate-400">{style.label}</span>
                </span>
                {summary && (
                  <span className="mt-0.5 block truncate text-xs text-slate-600">{summary}</span>
                )}
                {!step && (
                  <span className="mt-0.5 block truncate text-xs text-slate-400">{meta.hint}</span>
                )}
              </span>
              {step && (
                <RightOutlined
                  className={`mt-1 shrink-0 text-[10px] text-slate-300 transition-transform ${open ? "rotate-90" : ""}`}
                />
              )}
            </button>

            {/* 被拦下的原因不藏进折叠区：那是用户跑这一趟最想看的一句话 */}
            {reason && (
              <div
                className={`mx-3 mb-2 rounded-md px-2 py-1.5 text-xs leading-5 ${
                  step?.outcome.kind === "block"
                    ? "bg-rose-100 font-medium text-rose-700"
                    : "bg-slate-100 text-slate-600"
                }`}
              >
                {`${step?.outcome.kind === "block" ? "拦下原因" : "跳过原因"}：${reason}`}
              </div>
            )}

            {open && step && (
              <div className="border-t border-slate-200/70 bg-white/70">
                <DetailBody stage={meta.stage} detail={step.detail} />
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
