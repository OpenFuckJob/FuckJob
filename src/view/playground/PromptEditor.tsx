import { Button, Input, Space, Tag } from "antd";
import { SaveOutlined, UndoOutlined } from "@ant-design/icons";
import { chainMeta, type PlaygroundChain, type PromptKey } from "@/types/playground";
import { Section } from "./ui";

const { TextArea } = Input;

/** 三段可覆盖的提示词，与后端 PromptOverrides 一一对应 */
export type PromptDraft = Record<PromptKey, string>;

export const EMPTY_PROMPT_DRAFT: PromptDraft = {
  greet_prompt: "",
  reply_prompt: "",
  semantic_filter_intent: "",
};

const FIELDS: Record<PromptKey, { label: string; hint: string; placeholder: string; rows: number }> = {
  greet_prompt: {
    label: "打招呼提示词",
    hint: "模型照它来写开场白",
    placeholder: "留空则使用内置的打招呼提示词",
    rows: 8,
  },
  reply_prompt: {
    label: "回复提示词",
    hint: "模型照它来接 HR 的话",
    placeholder: "留空则使用内置的回复提示词",
    rows: 8,
  },
  semantic_filter_intent: {
    label: "语义筛选意图",
    hint: "一句话说清你想要什么样的岗位",
    placeholder: "如：只要后端方向，不接受外包与驻场",
    rows: 4,
  },
};

const PROMPT_KEYS = Object.keys(FIELDS) as PromptKey[];

export function isPromptDirty(draft: PromptDraft, baseline: PromptDraft): boolean {
  return PROMPT_KEYS.some((key) => draft[key] !== baseline[key]);
}

export interface PromptEditorProps {
  chain: PlaygroundChain;
  draft: PromptDraft;
  /** 方案里存着的原值，用来判断有没有改过 */
  baseline: PromptDraft;
  onChange: (key: PromptKey, value: string) => void;
  /** 三条一起写回方案：另外两条没改，写回去就是原值 */
  onSave: () => void;
  /** 只还原当前这条，别的链路改到一半的草稿不该被连坐 */
  onReset: (key: PromptKey) => void;
}

/**
 * 提示词就地编辑，一次只露出当前链路吃的那一条。
 *
 * 三条一起摆出来时，改错框是常事——跑的是筛选，改的却是打招呼提示词，
 * 然后对着没有变化的结果反复琢磨。另外编辑默认只作为 overrides 随本次调用带出去、
 * 不落盘：调提示词是高频试错，每改一个字就写进配置文件等于把半成品塞进用户的方案里。
 * 所以「已修改未保存」必须一眼可见，否则用户会以为改动早就生效了
 */
export function PromptEditor({
  chain,
  draft,
  baseline,
  onChange,
  onSave,
  onReset,
}: PromptEditorProps) {
  const key = chainMeta(chain).promptKey;
  if (!key) return null;

  const field = FIELDS[key];
  const dirty = draft[key] !== baseline[key];
  // 在别的链路下改的那两条也要能存回去，否则切一次链路就把改动锁死了
  const anyDirty = isPromptDirty(draft, baseline);

  return (
    <Section
      title={field.label}
      hint={`${field.hint}。改动只在本次运行临时生效，不写回求职方案`}
      extra={dirty ? <Tag color="orange">已修改未保存</Tag> : undefined}
    >
      <TextArea
        aria-label={field.label}
        rows={field.rows}
        value={draft[key]}
        placeholder={field.placeholder}
        onChange={(event) => onChange(key, event.target.value)}
      />
      <Space className="mt-3">
        <Button icon={<SaveOutlined />} disabled={!anyDirty} onClick={onSave}>
          保存回方案
        </Button>
        <Button type="text" icon={<UndoOutlined />} disabled={!dirty} onClick={() => onReset(key)}>
          还原
        </Button>
      </Space>
    </Section>
  );
}
