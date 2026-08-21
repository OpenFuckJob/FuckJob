import { Button, Collapse, Input, Space, Tag, Tooltip } from "antd";
import { ReloadOutlined, SaveOutlined, UndoOutlined } from "@ant-design/icons";

const { TextArea } = Input;

/** 三段可覆盖的提示词，与后端 PromptOverrides 一一对应 */
export interface PromptDraft {
  greet_prompt: string;
  reply_prompt: string;
  semantic_filter_intent: string;
}

export const EMPTY_PROMPT_DRAFT: PromptDraft = {
  greet_prompt: "",
  reply_prompt: "",
  semantic_filter_intent: "",
};

export interface PromptEditorProps {
  draft: PromptDraft;
  /** 方案里存着的原值，用来判断有没有改过 */
  baseline: PromptDraft;
  onChange: (key: keyof PromptDraft, value: string) => void;
  onRerun: () => void;
  onSave: () => void;
  onReset: () => void;
  running: boolean;
  /** 上一次跑的是哪条链路，决定「重跑」按钮跑什么；没跑过时禁用 */
  canRerun: boolean;
}

const FIELDS: Array<{ key: keyof PromptDraft; label: string; placeholder: string }> = [
  { key: "greet_prompt", label: "打招呼提示词", placeholder: "留空则使用内置的打招呼提示词" },
  { key: "reply_prompt", label: "回复提示词", placeholder: "留空则使用内置的回复提示词" },
  { key: "semantic_filter_intent", label: "语义筛选意图", placeholder: "一句话描述你想要什么样的岗位" },
];

export function isPromptDirty(draft: PromptDraft, baseline: PromptDraft): boolean {
  return FIELDS.some((field) => draft[field.key] !== baseline[field.key]);
}

/**
 * 提示词就地编辑。
 *
 * 编辑的结果默认只作为 overrides 随本次调用带出去，不落盘——调提示词是个
 * 高频试错的过程，每改一个字都写进配置文件，等于把一堆半成品塞进用户的方案里。
 * 真的调顺了再点「保存回方案」。所以「已修改未保存」必须一眼可见，
 * 否则用户会以为改动早就生效了。
 */
export function PromptEditor({
  draft,
  baseline,
  onChange,
  onRerun,
  onSave,
  onReset,
  running,
  canRerun,
}: PromptEditorProps) {
  const dirty = isPromptDirty(draft, baseline);

  return (
    <Collapse
      size="small"
      className="shrink-0"
      // 调提示词才是这个页面存在的理由，默认摊开；嫌占地方再手动收起
      defaultActiveKey={["prompts"]}
      items={[
        {
          key: "prompts",
          label: (
            <span className="flex items-center gap-2 text-sm font-medium text-slate-700">
              提示词编辑
              {dirty && (
                <Tag color="orange" className="m-0!">
                  已修改未保存
                </Tag>
              )}
            </span>
          ),
          children: (
            <div className="space-y-3">
              {dirty && (
                <div className="rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs leading-5 text-amber-700">
                  当前改动只在「重跑」时临时生效，不会写回求职方案。
                </div>
              )}
              {FIELDS.map((field) => (
                <label key={field.key} className="block">
                  <span className="mb-1 flex items-center gap-2 text-xs font-medium text-slate-600">
                    {field.label}
                    {draft[field.key] !== baseline[field.key] && (
                      <span className="text-[11px] font-normal text-amber-600">已改</span>
                    )}
                  </span>
                  <TextArea
                    aria-label={field.label}
                    rows={4}
                    value={draft[field.key]}
                    placeholder={field.placeholder}
                    onChange={(event) => onChange(field.key, event.target.value)}
                  />
                </label>
              ))}
              <Space>
                <Tooltip title={canRerun ? "" : "先跑一次筛选或打招呼"}>
                  <Button
                    type="primary"
                    icon={<ReloadOutlined />}
                    loading={running}
                    disabled={!canRerun}
                    onClick={onRerun}
                  >
                    重跑
                  </Button>
                </Tooltip>
                <Button icon={<SaveOutlined />} disabled={!dirty} onClick={onSave}>
                  保存回方案
                </Button>
                <Button type="text" icon={<UndoOutlined />} disabled={!dirty} onClick={onReset}>
                  还原
                </Button>
              </Space>
            </div>
          ),
        },
      ]}
    />
  );
}
