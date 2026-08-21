import { Button, Input, InputNumber, Select } from "antd";
import { ClearOutlined } from "@ant-design/icons";
import {
  RESUME_STATE_OPTIONS,
  chainMeta,
  type PlaygroundChain,
  type PlaygroundJob,
  type PlaygroundMessage,
  type PromptKey,
  type ResumeState,
} from "@/types/playground";
import { ChatSandbox } from "./ChatSandbox";
import { PromptEditor, type PromptDraft } from "./PromptEditor";
import { Field, Section } from "./ui";

const { TextArea } = Input;

export interface ScenarioStepProps {
  chain: PlaygroundChain;
  job: PlaygroundJob;
  onJobChange: (next: Partial<PlaygroundJob>) => void;
  resumeState: ResumeState;
  onResumeStateChange: (next: ResumeState) => void;
  repliesInWindow: number;
  onRepliesInWindowChange: (next: number) => void;
  messages: PlaygroundMessage[];
  onAppendMessage: (message: PlaygroundMessage) => void;
  onClearMessages: () => void;
  draft: PromptDraft;
  baseline: PromptDraft;
  onDraftChange: (key: PromptKey, value: string) => void;
  onSavePrompt: () => void;
  onResetPrompt: (key: PromptKey) => void;
}

/**
 * 第二步：把要复现的那个场景摆出来。
 *
 * 只渲染当前链路真正用得上的输入。简历状态和「窗口内已回复数」仍然是可见的表单项、
 * 而不是藏进高级选项——投递校正和回复闸门的大半分支只有靠它俩才构造得出来，
 * 但它们只在回复链路下出现，筛选链路不该被这两个不相干的字段占住视线
 */
export function ScenarioStep({
  chain,
  job,
  onJobChange,
  resumeState,
  onResumeStateChange,
  repliesInWindow,
  onRepliesInWindowChange,
  messages,
  onAppendMessage,
  onClearMessages,
  draft,
  baseline,
  onDraftChange,
  onSavePrompt,
  onResetPrompt,
}: ScenarioStepProps) {
  const meta = chainMeta(chain);

  return (
    <div className="space-y-4">
      <Section
        title="岗位信息"
        hint={
          meta.needsChat
            ? "这条会话挂在哪个岗位上，模型读得到"
            : "手输一个岗位，链路的第一步就从这里开始"
        }
      >
        <div className="grid gap-3 md:grid-cols-2">
          <Field label="岗位名称" hint="必填">
            <Input
              aria-label="岗位名称"
              placeholder="如：高级前端工程师"
              value={job.title}
              onChange={(event) => onJobChange({ title: event.target.value })}
            />
          </Field>
          <Field label="公司名称">
            <Input
              aria-label="公司名称"
              placeholder="如：示例科技"
              value={job.company_name}
              onChange={(event) => onJobChange({ company_name: event.target.value })}
            />
          </Field>
          <Field label="薪资">
            <Input
              aria-label="薪资"
              placeholder="25-50K"
              value={job.salary}
              onChange={(event) => onJobChange({ salary: event.target.value })}
            />
          </Field>
          <Field label="工作地点">
            <Input
              aria-label="工作地点"
              placeholder="深圳·南山"
              value={job.location}
              onChange={(event) => onJobChange({ location: event.target.value })}
            />
          </Field>
        </div>
        <div className="mt-3">
          <Field label="岗位描述" hint="JD 原文，语义复核与话术都读它">
            <TextArea
              aria-label="岗位描述"
              placeholder="粘贴完整的岗位 JD…"
              rows={6}
              value={job.detail}
              onChange={(event) => onJobChange({ detail: event.target.value })}
            />
          </Field>
        </div>
      </Section>

      {meta.needsChat && (
        <Section
          title="聊天沙盘"
          hint="以 HR 身份造几条消息；最后一条是谁发的，会直接决定闸门的判断"
          extra={
            <Button
              size="small"
              icon={<ClearOutlined />}
              disabled={messages.length === 0}
              onClick={onClearMessages}
            >
              清空对话
            </Button>
          }
        >
          <ChatSandbox messages={messages} onAppend={onAppendMessage} />
        </Section>
      )}

      {meta.needsReplyContext && (
        <Section title="会话现场" hint="这两项决定投递校正与回复闸门走哪条分支">
          <div className="grid gap-3 md:grid-cols-2">
            <Field label="简历投递入口" hint="影响投递意图校正">
              <Select
                aria-label="简历投递入口"
                className="w-full"
                value={resumeState}
                onChange={onResumeStateChange}
                options={RESUME_STATE_OPTIONS.map((option) => ({
                  value: option.value,
                  label: option.label,
                  title: option.hint,
                }))}
                optionRender={(option) => (
                  <div>
                    <div className="text-sm text-slate-800">{String(option.data.label)}</div>
                    <div className="text-[11px] leading-4 text-slate-400">
                      {String(option.data.title)}
                    </div>
                  </div>
                )}
              />
            </Field>
            <Field label="窗口内已回复条数" hint="用满就会被闸门挂起">
              <InputNumber
                aria-label="窗口内已回复条数"
                className="w-full"
                min={0}
                max={99}
                value={repliesInWindow}
                onChange={(value) => onRepliesInWindowChange(typeof value === "number" ? value : 0)}
              />
            </Field>
          </div>
        </Section>
      )}

      <PromptEditor
        chain={chain}
        draft={draft}
        baseline={baseline}
        onChange={onDraftChange}
        onSave={onSavePrompt}
        onReset={onResetPrompt}
      />
    </div>
  );
}
