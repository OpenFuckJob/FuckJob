import { Alert, Badge, Tabs } from "antd";
import {
  chainMeta,
  reportVerdict,
  stagesOfChain,
  stageSymbol,
  type AgentTrace,
  type PlaygroundChain,
  type PlaygroundStage,
  type StepReport,
} from "@/types/playground";
import { PipelineView } from "./PipelineView";
import { TracePanel } from "./TracePanel";
import { Section, StageTrail, type StageTone } from "./ui";

export interface ResultStepProps {
  chain: PlaygroundChain;
  report: StepReport;
  traces: AgentTrace[];
  tracesLoading: boolean;
  onRefreshTraces: () => void;
  onClearTraces: () => void;
  onExportTraces: () => void;
  /** 回复链路里，通过体检的那句话已经作为我方消息进了沙盘 */
  appendedToChat: boolean;
}

/**
 * 第三步：这一趟走到哪、为什么停。
 *
 * 结论单独占一条横幅，而不是让用户自己在环节列表里找那一行红的——「断在第几步」
 * 才是跑这一趟要问的问题，环节明细和模型轨迹都是拿到答案之后才需要往下追的东西，
 * 所以它俩分在两个页签里，默认停在环节明细
 */
export function ResultStep({
  chain,
  report,
  traces,
  tracesLoading,
  onRefreshTraces,
  onClearTraces,
  onExportTraces,
  appendedToChat,
}: ResultStepProps) {
  const meta = chainMeta(chain);
  const verdict = reportVerdict(report.steps);
  const skipped = report.steps.filter((step) => step.outcome.kind === "skip").length;
  const tones: Partial<Record<PlaygroundStage, StageTone>> = Object.fromEntries(
    report.steps.map((step) => [step.stage, step.outcome.kind]),
  );

  return (
    <div className="space-y-4">
      {verdict.kind === "block" ? (
        <Alert
          type="error"
          showIcon
          message={`在 ${stageSymbol(verdict.stage.order)} ${verdict.stage.label} 被拦下`}
          description={verdict.reason || "后端没有给出具体原因"}
        />
      ) : verdict.kind === "pass" ? (
        <Alert
          type="success"
          showIcon
          message={`${meta.label}链路走完，没有被拦下`}
          description={
            <span className="text-xs">
              最后停在 {stageSymbol(verdict.stage.order)} {verdict.stage.label}
              {skipped > 0 ? ` · ${skipped} 个环节按配置跳过` : ""}
              {appendedToChat ? " · 生成的回复已追加进沙盘对话" : ""}
              。真实运行到这里才会发出消息，测试模式不发。
            </span>
          }
        />
      ) : (
        <Alert type="info" showIcon message="这一趟没有产生任何环节结果" />
      )}

      <Section title="断点示意" hint="绿色通过、红色拦下、灰色没跑到">
        <StageTrail stages={stagesOfChain(chain)} tones={tones} />
      </Section>

      <Tabs
        items={[
          {
            key: "pipeline",
            label: `环节明细 · ${report.steps.length}`,
            children: <PipelineView steps={report.steps} chain={chain} />,
          },
          {
            key: "traces",
            label: (
              <span className="flex items-center gap-2">
                模型调用
                <Badge
                  count={traces.length}
                  showZero
                  color={traces.length > 0 ? "#1677ff" : "#cbd5e1"}
                  size="small"
                />
              </span>
            ),
            children: (
              <TracePanel
                traces={traces}
                loading={tracesLoading}
                onRefresh={onRefreshTraces}
                onClear={onClearTraces}
                onExport={onExportTraces}
              />
            ),
          },
        ]}
      />
    </div>
  );
}
