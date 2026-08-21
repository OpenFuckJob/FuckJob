import { useEffect, useState } from "react";
import { Button, Empty, Segmented, Space, Tabs, Tag, Tooltip } from "antd";
import {
  ClearOutlined,
  CopyOutlined,
  DownloadOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import type { AgentTrace, RoundTrace } from "@/types/playground";
import {
  formatDuration,
  parseRoundOutput,
  splitRework,
  STOP_LABELS,
  totalTokens,
  traceSucceeded,
} from "./trace";

/** jsdom 与非安全上下文都没有 clipboard，复制失败时静默降级，不打断调试 */
async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard?.writeText(text);
    return true;
  } catch {
    return false;
  }
}

function CopyBlock({ label, text }: { label: string; text: string }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="mb-1 flex items-center justify-between">
        <span className="text-[11px] text-slate-400">{label}</span>
        <Button
          type="text"
          size="small"
          icon={<CopyOutlined />}
          aria-label={`复制${label}`}
          onClick={() => void copyText(text)}
        >
          复制
        </Button>
      </div>
      <pre className="m-0 max-h-64 overflow-auto rounded-lg bg-slate-50 p-2 text-[11px] leading-5 whitespace-pre-wrap wrap-break-word text-slate-700">
        {text || "（空）"}
      </pre>
    </div>
  );
}

function PromptTab({ round, previous }: { round: RoundTrace; previous: RoundTrace | null }) {
  const split = previous ? splitRework(previous.prompt, round.prompt) : null;
  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <span className="text-[11px] text-slate-400">
          {split?.appended ? "高亮部分是本轮相对上一轮追加的返工说明" : "完整提示词"}
        </span>
        <Button
          type="text"
          size="small"
          icon={<CopyOutlined />}
          aria-label="复制提示词"
          onClick={() => void copyText(round.prompt)}
        >
          复制
        </Button>
      </div>
      <pre className="m-0 max-h-64 overflow-auto rounded-lg bg-slate-50 p-2 text-[11px] leading-5 whitespace-pre-wrap wrap-break-word text-slate-700">
        {split && split.appended ? (
          <>
            {split.shared}
            <mark data-testid="rework-segment" className="bg-amber-200 text-slate-900">
              {split.appended}
            </mark>
          </>
        ) : (
          round.prompt || "（空）"
        )}
      </pre>
    </div>
  );
}

function VerdictTab({ round }: { round: RoundTrace }) {
  const verdict = round.verdict;
  return (
    <div className="space-y-2 text-xs text-slate-700">
      <div className="flex items-center gap-2">
        <span className="text-[11px] text-slate-400">本轮判定</span>
        {verdict.kind === "passed" ? (
          <Tag color="green">校验通过</Tag>
        ) : verdict.kind === "rejected" ? (
          <Tag color="orange">校验不通过，需返工</Tag>
        ) : (
          <Tag color="red">调用失败</Tag>
        )}
      </div>
      {verdict.kind !== "passed" && (
        <div className="rounded-md bg-amber-50 px-2 py-1.5 leading-5 text-amber-700">
          {verdict.reason}
        </div>
      )}
      {verdict.kind === "passed" && (
        <div className="text-slate-400">这一轮的输出直接被采用，没有触发返工。</div>
      )}
    </div>
  );
}

function TraceDetail({ trace }: { trace: AgentTrace }) {
  const [roundIndex, setRoundIndex] = useState(0);
  // 切换轨迹后轮次要回到第一轮，否则上一条的第 3 轮会落在只有 1 轮的轨迹上
  useEffect(() => setRoundIndex(0), [trace.id]);

  const round = trace.rounds[Math.min(roundIndex, trace.rounds.length - 1)];
  if (!round) {
    return <div className="px-1 py-2 text-xs text-slate-400">这条轨迹没有记录到任何一轮调用</div>;
  }
  const previous = roundIndex > 0 ? trace.rounds[roundIndex - 1] : null;
  const output = parseRoundOutput(round.raw);
  const tokens = totalTokens(round);

  return (
    <div className="space-y-2 border-t border-slate-200 pt-2">
      {trace.rounds.length > 1 && (
        <Segmented
          size="small"
          aria-label="轮次"
          value={roundIndex}
          onChange={(value) => setRoundIndex(Number(value))}
          options={trace.rounds.map((item, index) => ({
            value: index,
            label: `第 ${item.round} 轮`,
          }))}
        />
      )}

      <div className="flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-slate-500">
        <span>模型：{round.model ?? "未记录"}</span>
        <span>耗时：{formatDuration(round.duration_ms)}</span>
        <span>
          token：
          {tokens === null
            ? "未记录"
            : `${tokens}（入 ${round.usage?.prompt_tokens ?? "-"} / 出 ${round.usage?.completion_tokens ?? "-"}）`}
        </span>
      </div>

      <Tabs
        size="small"
        items={[
          { key: "prompt", label: "完整提示词", children: <PromptTab round={round} previous={previous} /> },
          { key: "raw", label: "原始输出", children: <CopyBlock label="原始输出" text={round.raw} /> },
          {
            key: "parsed",
            label: "解析结果",
            children: output.parsed ? (
              <CopyBlock label="解析结果" text={output.pretty} />
            ) : (
              <div className="text-xs text-slate-400">
                这轮输出不是 JSON，无法结构化展示，请看「原始输出」。
              </div>
            ),
          },
          { key: "verdict", label: "校验与返工", children: <VerdictTab round={round} /> },
        ]}
      />
    </div>
  );
}

export interface TracePanelProps {
  traces: AgentTrace[];
  loading: boolean;
  onRefresh: () => void;
  onClear: () => void;
  onExport: () => void;
}

/**
 * 这一趟链路里模型到底被调了几次、每次说了什么。
 *
 * 轨迹按 StepReport 带回的 trace_ids 拉取，所以列表顺序就是调用顺序，
 * 不再按时间倒序——调试时要顺着链路读下来，倒序反而得从底下往上看。
 * 提示词原文动辄上千字，所以它占满结果区的整幅宽度，而不是挤在一条窄侧栏里
 */
export function TracePanel({ traces, loading, onRefresh, onClear, onExport }: TracePanelProps) {
  const [selected, setSelected] = useState<string | null>(null);
  const active = traces.find((trace) => trace.id === selected) ?? null;

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-xs text-slate-500">
          {traces.length > 0 ? "点开任意一次调用，能看到完整提示词、原始输出与返工过程" : ""}
        </span>
        <Space>
          <Button type="text" size="small" icon={<ReloadOutlined />} loading={loading} onClick={onRefresh}>
            刷新
          </Button>
          <Tooltip title="清掉后端缓存的全部轨迹">
            <Button type="text" size="small" icon={<ClearOutlined />} onClick={onClear}>
              清空
            </Button>
          </Tooltip>
          <Button type="text" size="small" icon={<DownloadOutlined />} onClick={onExport}>
            导出
          </Button>
        </Space>
      </div>

      <div>
        {traces.length === 0 ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={<span className="text-xs text-slate-400">这一趟没有调用大模型</span>}
          />
        ) : (
          <div className="space-y-1.5">
            {traces.map((trace) => {
              const ok = traceSucceeded(trace);
              const open = trace.id === selected;
              return (
                <div
                  key={trace.id}
                  className={`rounded-lg border px-2.5 py-2 ${
                    open ? "border-sky-300 bg-sky-50/50" : "border-slate-200 bg-white"
                  }`}
                >
                  <button
                    type="button"
                    aria-expanded={open}
                    aria-label={`轨迹 ${trace.task_name}`}
                    className="w-full cursor-pointer text-left"
                    onClick={() => setSelected(open ? null : trace.id)}
                  >
                    <div className="flex items-baseline justify-between gap-2">
                      <span className="truncate text-sm font-medium text-slate-800">
                        {trace.task_name}
                      </span>
                      <span className={`shrink-0 text-[11px] ${ok ? "text-emerald-600" : "text-rose-600"}`}>
                        {ok ? "成功" : "失败"}
                      </span>
                    </div>
                    <div className="mt-0.5 flex flex-wrap gap-x-2 text-[11px] text-slate-500">
                      <span>{trace.rounds.length} 轮</span>
                      <span>{formatDuration(trace.duration_ms)}</span>
                      {trace.stop && <span>{STOP_LABELS[trace.stop] ?? trace.stop}</span>}
                    </div>
                    {trace.error && (
                      <div className="mt-1 rounded bg-rose-50 px-1.5 py-1 text-[11px] leading-4 text-rose-600">
                        {trace.error}
                      </div>
                    )}
                  </button>
                  {open && active && <TraceDetail trace={active} />}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
