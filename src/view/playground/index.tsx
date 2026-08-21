import { useCallback, useEffect, useMemo, useState } from "react";
import { Button, Segmented, Steps, Tooltip, Typography, message as antdMessage } from "antd";
import {
  ArrowLeftOutlined,
  ArrowRightOutlined,
  ExperimentOutlined,
  PlayCircleOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { save } from "@tauri-apps/plugin-dialog";
import { AiFeatureGate } from "@/components/AiFeatureGate";
import { getDefaultJobProfile, getJobProfiles, type AppRuntimeConfig, type JobProfile } from "@/types/app-config";
import {
  PLAYGROUND_CHAINS,
  chainMeta,
  type AgentTrace,
  type PlaygroundChain,
  type PlaygroundJob,
  type PlaygroundMessage,
  type PromptKey,
  type PromptOverrides,
  type ResumeState,
  type StepReport,
} from "@/types/playground";
import { ChainPicker } from "./ChainPicker";
import { EMPTY_PROMPT_DRAFT, isPromptDirty, type PromptDraft } from "./PromptEditor";
import { ResultStep } from "./ResultStep";
import { ScenarioStep } from "./ScenarioStep";
import { extractOutgoingText } from "./detail";
import { clearTraces, exportTraces, fetchTraces, runGreet, runReply, runScreen } from "./api";

const EMPTY_JOB: PlaygroundJob = {
  title: "",
  company_name: "",
  detail: "",
  salary: "",
  location: "",
};

const STEPS = [
  { title: "选链路"},
  { title: "备场景"},
  { title: "看结果"},
];

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
 * 手输一个岗位、自己扮演 HR 造对话，把某一条自动化链路逐环节跑出来看走向。
 * 与真实任务的关键区别是：这里的每一步都不落库、不发消息，只把决策过程摊开。
 *
 * 页面按「选链路 → 备场景 → 看结果」分三步走。旧版把三条链路的输入、三段提示词、
 * 十个环节和模型轨迹全平铺在一屏三栏里，看着什么都能点，实际上每一趟只用得到其中
 * 三分之一，剩下三分之二既占地方又让人分不清自己在测什么。分了步之后，每一步只问
 * 一件事，要填什么由上一步的选择推出来
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

  const [step, setStep] = useState(0);
  const [chain, setChain] = useState<PlaygroundChain>("screen");
  const [job, setJob] = useState<PlaygroundJob>(EMPTY_JOB);
  const [resumeState, setResumeState] = useState<ResumeState>("Sendable");
  const [repliesInWindow, setRepliesInWindow] = useState(0);
  const [messages, setMessages] = useState<PlaygroundMessage[]>([]);

  const baseline = useMemo(() => draftOf(profile), [profile]);
  const [draft, setDraft] = useState<PromptDraft>(baseline);
  // 换方案等于换一套提示词，草稿必须跟着走，否则会把 A 方案的话术存进 B 方案
  useEffect(() => setDraft(baseline), [baseline]);

  const [report, setReport] = useState<StepReport | null>(null);
  // 结果属于跑它出来的那条链路。切链路时不清结果，只记住它是谁的——
  // 否则想对比「筛选过了，打招呼却被拦」时，前一趟的结论刚看完就没了
  const [resultChain, setResultChain] = useState<PlaygroundChain>("screen");
  const [traces, setTraces] = useState<AgentTrace[]>([]);
  const [running, setRunning] = useState(false);
  const [tracesLoading, setTracesLoading] = useState(false);
  const [appendedToChat, setAppendedToChat] = useState(false);

  const meta = chainMeta(chain);

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

  const run = useCallback(async () => {
    if (!profile) return;
    if (!job.title.trim()) {
      messageApi.warning("先填岗位名称，链路的第一步就要用它");
      return;
    }
    if (meta.needsChat && messages.length === 0) {
      messageApi.warning("先在沙盘里造一条消息，闸门才有得判");
      return;
    }
    setRunning(true);
    setAppendedToChat(false);
    setResultChain(chain);
    const overrides = toOverrides(draft);
    try {
      if (chain === "screen") {
        await applyReport(await runScreen(profile.id, job, overrides));
      } else if (chain === "greet") {
        await applyReport(await runGreet(profile.id, job, overrides));
      } else {
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
        const vet = next.steps.find((item) => item.stage === "reply_vet");
        const text = vet?.outcome.kind === "pass" ? extractOutgoingText(vet.detail) : null;
        if (text) {
          setMessages((prev) => [...prev, { text, received: false }]);
          setAppendedToChat(true);
        }
      }
      setStep(2);
    } catch (error: unknown) {
      messageApi.error(errorText(error, "执行失败"));
    } finally {
      setRunning(false);
    }
  }, [applyReport, chain, draft, job, messageApi, messages, meta.needsChat, profile, repliesInWindow, resumeState]);

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

  /**
   * 换链路只换「测什么」，岗位、对话、会话现场原样留着。
   *
   * 这三条链路在真实运行里本来就是顺着走的：筛选过了才打招呼，打完招呼才有回复。
   * 挨个试的时候要是每换一条就得把 JD 重贴一遍，测试模式立刻变成体力活
   */
  const selectChain = (next: PlaygroundChain) => {
    if (next === chain) return;
    setChain(next);
    // 结果留着但会标明是上一条链路跑的，第三步据此渲染
    setStep((current) => (current === 2 ? 1 : current));
  };

  const nextChain: PlaygroundChain | null =
    resultChain === "screen" ? "greet" : resultChain === "greet" ? "reply" : null;

  const promptDirty = isPromptDirty(draft, baseline);

  const footer = (
    // 负边距把行动条铺到配置页内容区的两侧留白上，否则滚动的内容会从边缘穿出来
    <div className="sticky bottom-0 -mx-6 mt-4 flex flex-wrap items-center justify-between gap-3 border-t border-slate-200 bg-white/95 px-6 py-3 backdrop-blur md:-mx-10 md:px-10">
      <div className="flex items-center gap-2 text-xs text-slate-500">
        {promptDirty ? (
          <span className="text-amber-600">提示词有未保存改动，本次运行按改动后的跑</span>
        ) : (
          <span>岗位与对话在三条链路之间共用，换链路不必重填</span>
        )}
      </div>
      <div className="flex items-center gap-2">
        {step > 0 && (
          <Button icon={<ArrowLeftOutlined />} onClick={() => setStep(step - 1)}>
            上一步
          </Button>
        )}
        {step === 0 && (
          <Button type="primary" onClick={() => setStep(1)}>
            下一步：备场景 <ArrowRightOutlined />
          </Button>
        )}
        {step === 1 && (
          <Tooltip title="只在本地预演，不会产生任何发送动作">
            <Button type="primary" icon={<PlayCircleOutlined />} loading={running} onClick={() => void run()}>
              {meta.action}
            </Button>
          </Tooltip>
        )}
        {step === 2 && (
          <>
            {nextChain && (
              // 顺着链路往下测是最常见的下一步，别让用户自己退回去换链路
              <Button
                icon={<ArrowRightOutlined />}
                onClick={() => {
                  selectChain(nextChain);
                  setStep(1);
                }}
              >
                接着测{chainMeta(nextChain).label}
              </Button>
            )}
            <Button type="primary" icon={<ReloadOutlined />} loading={running} onClick={() => void run()}>
              用当前场景重跑
            </Button>
          </>
        )}
      </div>
    </div>
  );

  /**
   * 第二、三步都挂着的一条现场提要：这一趟用的什么方案、什么岗位、测哪条链路。
   *
   * 链路切换做在这里而不是只留在第一步，是因为「同一个岗位挨条链路试过去」才是
   * 常态；顺带把方案名和岗位名摆出来，用户一眼能确认这些资料是共用的、不用重填
   */
  const contextBar = (
    <div className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-slate-200/80 bg-slate-50/70 px-4 py-3">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-[11px] font-bold uppercase tracking-widest text-slate-500">当前测试链路</span>
        <span className="truncate text-xs text-slate-500">
          方案 {profile?.name ?? "—"} · 岗位 {job.title.trim() || "未填写"}
          {messages.length > 0 ? ` · 对话 ${messages.length} 条` : ""}
        </span>
      </div>
      <Segmented<PlaygroundChain>
        aria-label="切换测试链路"
        value={chain}
        onChange={selectChain}
        options={PLAYGROUND_CHAINS.map((item) => ({ value: item.chain, label: item.label }))}
      />
    </div>
  );

  return (
    <div className="flex flex-col">
      {contextHolder}
      <div className="mb-5">
        <Typography.Title level={4} className="text-slate-900! m-0! flex items-center gap-2">
          <ExperimentOutlined className="text-sky-500" />
          测试模式
        </Typography.Title>
        <Typography.Text className="text-slate-500 text-xs uppercase font-bold tracking-widest">
          不落库 · 不发消息 · 逐环节看清链路怎么判
        </Typography.Text>
      </div>

      <AiFeatureGate active={llmActive} configured={llmConfigured} onConfigure={onOpenLlmConfig}>
        <Steps
          size="small"
          className="mb-5"
          current={step}
          // 前两步随时可回去改，结果页只在真跑过之后才点得动——
          // 没跑过就跳过去，看到的是一片空白，比点不动更让人困惑
          onChange={(next) => {
            if (next === 2 && !report) return;
            setStep(next);
          }}
          items={STEPS.map((item, index) => ({
            title: item.title,
            disabled: index === 2 && !report,
          }))}
        />

        {step > 0 && contextBar}

        {step === 0 && (
          <ChainPicker
            profiles={profiles}
            profileId={profile?.id ?? ""}
            onSelectProfile={setProfileId}
            defaultProfileId={config.default_job_profile_id || getDefaultJobProfile(config).id}
            chain={chain}
            onSelectChain={selectChain}
          />
        )}

        {step === 1 && (
          <ScenarioStep
            chain={chain}
            job={job}
            onJobChange={(next) => setJob((prev) => ({ ...prev, ...next }))}
            resumeState={resumeState}
            onResumeStateChange={setResumeState}
            repliesInWindow={repliesInWindow}
            onRepliesInWindowChange={setRepliesInWindow}
            messages={messages}
            onAppendMessage={(next) => setMessages((prev) => [...prev, next])}
            onClearMessages={() => setMessages([])}
            draft={draft}
            baseline={baseline}
            onDraftChange={(key: PromptKey, value: string) =>
              setDraft((prev) => ({ ...prev, [key]: value }))
            }
            onSavePrompt={handleSavePrompts}
            onResetPrompt={(key: PromptKey) =>
              setDraft((prev) => ({ ...prev, [key]: baseline[key] }))
            }
          />
        )}

        {step === 2 && report && (
          <ResultStep
            chain={resultChain}
            report={report}
            traces={traces}
            tracesLoading={tracesLoading}
            onRefreshTraces={() => void loadTraces(report.trace_ids ?? [])}
            onClearTraces={() => void handleClearTraces()}
            onExportTraces={() => void handleExport()}
            appendedToChat={appendedToChat}
          />
        )}

        {footer}
      </AiFeatureGate>
    </div>
  );
}
