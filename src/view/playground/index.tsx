import { useCallback, useEffect, useMemo, useState } from "react";
import { message as antdMessage } from "antd";
import { save } from "@tauri-apps/plugin-dialog";
import { AiFeatureGate } from "@/components/AiFeatureGate";
import { getDefaultJobProfile, getJobProfiles, type AppRuntimeConfig, type JobProfile } from "@/types/app-config";
import type {
  AgentTrace,
  PlaygroundJob,
  PlaygroundMessage,
  PromptOverrides,
  ResumeState,
  StepReport,
} from "@/types/playground";
import { ChatSandbox } from "./ChatSandbox";
import { PipelineView } from "./PipelineView";
import { EMPTY_PROMPT_DRAFT, PromptEditor, type PromptDraft } from "./PromptEditor";
import { ScenarioPanel } from "./ScenarioPanel";
import { TracePanel } from "./TracePanel";
import { extractOutgoingText } from "./detail";
import { clearTraces, exportTraces, fetchTraces, runGreet, runReply, runScreen } from "./api";

const EMPTY_JOB: PlaygroundJob = {
  title: "",
  company_name: "",
  detail: "",
  salary: "",
  location: "",
};

/** 上一次跑的是哪条链路，「重跑」按钮据此重放同一条 */
type LastRun = "screen" | "greet" | "reply" | null;

export interface PlaygroundPromptPatch {
  greet_prompt: string | null;
  reply_prompt: string | null;
  semantic_filter_intent: string | null;
}

export interface PlaygroundPageProps {
  config: AppRuntimeConfig;
  llmConfigured: boolean;
  llmActive: boolean;
  onOpenLlmConfig: () => void;
  /** 按方案 id 写回提示词——测试模式选的方案与配置页的当前方案是两回事 */
  onSavePrompts: (profileId: string, prompts: PlaygroundPromptPatch) => void;
}

function draftOf(profile: JobProfile | undefined): PromptDraft {
  if (!profile) return EMPTY_PROMPT_DRAFT;
  return {
    greet_prompt: profile.greet_config.reply_prompt ?? "",
    reply_prompt: profile.replay_config.reply_prompt ?? "",
    semantic_filter_intent: profile.job_filter_config.semantic_filter_intent ?? "",
  };
}

/** 空串按「不覆盖」处理：后端拿到 null 会回落到方案里存着的提示词 */
function toOverrides(draft: PromptDraft): PromptOverrides {
  const pick = (value: string) => (value.trim() ? value : null);
  return {
    greet_prompt: pick(draft.greet_prompt),
    reply_prompt: pick(draft.reply_prompt),
    semantic_filter_intent: pick(draft.semantic_filter_intent),
  };
}

function errorText(error: unknown, fallback: string): string {
  return error instanceof Error && error.message ? error.message : fallback;
}

/**
 * 测试模式。
 *
 * 手输一个岗位、自己扮演 HR 造对话，把整条自动化链路逐环节跑出来看走向。
 * 与真实任务的关键区别是：这里的每一步都不落库、不发消息，只把决策过程摊开——
 * 提示词是这个页面唯一真正要调的东西，其余控件都是为了把某个分支复现出来。
 */
export default function PlaygroundPage({
  config,
  llmConfigured,
  llmActive,
  onOpenLlmConfig,
  onSavePrompts,
}: PlaygroundPageProps) {
  const [messageApi, contextHolder] = antdMessage.useMessage();
  const profiles = useMemo(
    () => getJobProfiles(config).filter((profile) => !profile.archived),
    [config],
  );
  const [profileId, setProfileId] = useState(() => getDefaultJobProfile(config).id);
  const profile = profiles.find((item) => item.id === profileId) ?? profiles[0];

  const [job, setJob] = useState<PlaygroundJob>(EMPTY_JOB);
  const [resumeState, setResumeState] = useState<ResumeState>("Sendable");
  const [repliesInWindow, setRepliesInWindow] = useState(0);
  const [messages, setMessages] = useState<PlaygroundMessage[]>([]);

  const baseline = useMemo(() => draftOf(profile), [profile]);
  const [draft, setDraft] = useState<PromptDraft>(baseline);
  // 换方案等于换一套提示词，草稿必须跟着走，否则会把 A 方案的话术存进 B 方案
  useEffect(() => setDraft(baseline), [baseline]);

  const [report, setReport] = useState<StepReport | null>(null);
  const [traces, setTraces] = useState<AgentTrace[]>([]);
  const [running, setRunning] = useState(false);
  const [tracesLoading, setTracesLoading] = useState(false);
  const [lastRun, setLastRun] = useState<LastRun>(null);

  const loadTraces = useCallback(
    async (ids: string[]) => {
      if (ids.length === 0) {
        setTraces([]);
        return;
      }
      setTracesLoading(true);
      try {
        setTraces(await fetchTraces(ids));
      } catch (error: unknown) {
        messageApi.error(errorText(error, "读取调用轨迹失败"));
      } finally {
        setTracesLoading(false);
      }
    },
    [messageApi],
  );

  const applyReport = useCallback(
    async (next: StepReport) => {
      setReport(next);
      await loadTraces(next.trace_ids ?? []);
      return next;
    },
    [loadTraces],
  );

  const runChain = useCallback(
    async (kind: Exclude<LastRun, null>) => {
      if (!profile) return;
      if (kind !== "reply" && !job.title.trim()) {
        messageApi.warning("先填岗位名称，链路的第一步就要用它");
        return;
      }
      setRunning(true);
      setLastRun(kind);
      const overrides = toOverrides(draft);
      try {
        if (kind === "screen") {
          await applyReport(await runScreen(profile.id, job, overrides));
          return;
        }
        if (kind === "greet") {
          await applyReport(await runGreet(profile.id, job, overrides));
          return;
        }
        const next = await applyReport(
          await runReply({
            profileId: profile.id,
            job,
            messages,
            resumeState,
            repliesInWindow,
            overrides,
          }),
        );
        // 体检通过的回复才追加进对话——被拦下的文本本来就不会发出去，
        // 塞进气泡会让下一轮的上下文和真实运行时对不上
        const vet = next.steps.find((step) => step.stage === "reply_vet");
        const text = vet?.outcome.kind === "pass" ? extractOutgoingText(vet.detail) : null;
        if (text) setMessages((prev) => [...prev, { text, received: false }]);
      } catch (error: unknown) {
        messageApi.error(errorText(error, "执行失败"));
      } finally {
        setRunning(false);
      }
    },
    [applyReport, draft, job, messageApi, messages, profile, repliesInWindow, resumeState],
  );

  const handleSavePrompts = useCallback(() => {
    if (!profile) return;
    const patch = toOverrides(draft);
    onSavePrompts(profile.id, {
      greet_prompt: patch.greet_prompt ?? null,
      reply_prompt: patch.reply_prompt ?? null,
      semantic_filter_intent: patch.semantic_filter_intent ?? null,
    });
    messageApi.success(`已写回「${profile.name}」`);
  }, [draft, messageApi, onSavePrompts, profile]);

  const handleExport = useCallback(async () => {
    try {
      const path = await save({
        defaultPath: "playground-traces.json",
        filters: [{ name: "调用轨迹 (*.json)", extensions: ["json"] }],
      });
      if (!path) return;
      const count = await exportTraces(path);
      messageApi.success(`已导出 ${count} 条轨迹`);
    } catch (error: unknown) {
      messageApi.error(errorText(error, "导出轨迹失败"));
    }
  }, [messageApi]);

  const handleClearTraces = useCallback(async () => {
    try {
      await clearTraces();
      setTraces([]);
      messageApi.success("已清空轨迹");
    } catch (error: unknown) {
      messageApi.error(errorText(error, "清空轨迹失败"));
    }
  }, [messageApi]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      {contextHolder}
      <AiFeatureGate active={llmActive} configured={llmConfigured} onConfigure={onOpenLlmConfig}>
        <div className="flex h-full min-h-0 gap-3">
          <ScenarioPanel
            profiles={profiles}
            profileId={profile?.id ?? ""}
            onSelectProfile={setProfileId}
            job={job}
            onJobChange={(next) => setJob((prev) => ({ ...prev, ...next }))}
            resumeState={resumeState}
            onResumeStateChange={setResumeState}
            repliesInWindow={repliesInWindow}
            onRepliesInWindowChange={setRepliesInWindow}
            running={running}
            onRunScreen={() => void runChain("screen")}
            onRunGreet={() => void runChain("greet")}
          />

          <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-3 border-l border-slate-100 pl-3">
            <PromptEditor
              draft={draft}
              baseline={baseline}
              onChange={(key, value) => setDraft((prev) => ({ ...prev, [key]: value }))}
              onRerun={() => void runChain(lastRun ?? "screen")}
              onSave={handleSavePrompts}
              onReset={() => setDraft(baseline)}
              running={running}
              canRerun={lastRun !== null}
            />

            <div className="min-h-0 flex-1 overflow-y-auto pr-1">
              <PipelineView steps={report?.steps ?? []} hasRun={report !== null} />
            </div>

            <ChatSandbox
              messages={messages}
              onAppend={(next) => setMessages((prev) => [...prev, next])}
              onClear={() => setMessages([])}
              onAskAi={() => void runChain("reply")}
              running={running}
            />
          </div>

          <TracePanel
            traces={traces}
            loading={tracesLoading}
            onRefresh={() => void loadTraces(report?.trace_ids ?? [])}
            onClear={() => void handleClearTraces()}
            onExport={() => void handleExport()}
          />
        </div>
      </AiFeatureGate>
    </div>
  );
}
