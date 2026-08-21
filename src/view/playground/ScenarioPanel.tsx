import type { ReactNode } from "react";
import { Button, Input, InputNumber, Select, Tooltip, Typography } from "antd";
import { FilterOutlined, ThunderboltOutlined } from "@ant-design/icons";
import type { JobProfile } from "@/types/app-config";
import {
  RESUME_STATE_OPTIONS,
  type PlaygroundJob,
  type ResumeState,
} from "@/types/playground";

const { TextArea } = Input;

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
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

export interface ScenarioPanelProps {
  profiles: JobProfile[];
  profileId: string;
  onSelectProfile: (id: string) => void;
  job: PlaygroundJob;
  onJobChange: (next: Partial<PlaygroundJob>) => void;
  resumeState: ResumeState;
  onResumeStateChange: (next: ResumeState) => void;
  repliesInWindow: number;
  onRepliesInWindowChange: (next: number) => void;
  running: boolean;
  onRunScreen: () => void;
  onRunGreet: () => void;
}

/**
 * 左栏：这次要拿什么场景去跑。
 *
 * 简历状态和「窗口内已回复数」是刻意放在这里、而不是藏进高级选项的——
 * 旧调试页把它俩写死成「可投递 / 0」，于是投递校正和回复闸门的大半分支
 * 根本没法在界面上复现。要调那些分支的提示词，就得先能把它们跑出来。
 */
export function ScenarioPanel({
  profiles,
  profileId,
  onSelectProfile,
  job,
  onJobChange,
  resumeState,
  onResumeStateChange,
  repliesInWindow,
  onRepliesInWindowChange,
  running,
  onRunScreen,
  onRunGreet,
}: ScenarioPanelProps) {
  return (
    <div className="flex h-full min-h-0 w-[300px] shrink-0 flex-col gap-3 overflow-y-auto pr-3">
      <Field label="求职方案" hint="决定用哪套提示词与筛选规则">
        <Select
          aria-label="求职方案"
          className="w-full"
          value={profileId}
          onChange={onSelectProfile}
          options={profiles.map((profile) => ({ value: profile.id, label: profile.name }))}
        />
      </Field>

      <div className="border-t border-slate-100 pt-3">
        <Typography.Text className="text-xs font-semibold text-slate-500">岗位信息</Typography.Text>
      </div>

      <Field label="岗位名称">
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
      <div className="grid grid-cols-2 gap-2">
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
      <Field label="岗位描述" hint="JD 原文">
        <TextArea
          aria-label="岗位描述"
          placeholder="粘贴完整的岗位 JD…"
          rows={7}
          value={job.detail}
          onChange={(event) => onJobChange({ detail: event.target.value })}
        />
      </Field>

      <div className="border-t border-slate-100 pt-3">
        <Typography.Text className="text-xs font-semibold text-slate-500">回复上下文</Typography.Text>
      </div>

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
              <div className="text-[11px] leading-4 text-slate-400">{String(option.data.title)}</div>
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

      <div className="mt-auto flex flex-col gap-2 border-t border-slate-100 pt-3">
        <Tooltip title="只跑 ①② 两个环节，不会产生任何发送动作">
          <Button icon={<FilterOutlined />} loading={running} onClick={onRunScreen} block>
            跑筛选
          </Button>
        </Tooltip>
        <Button type="primary" icon={<ThunderboltOutlined />} loading={running} onClick={onRunGreet} block>
          跑打招呼
        </Button>
      </div>
    </div>
  );
}
