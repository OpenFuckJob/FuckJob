import { Select, Tag } from "antd";
import { RadioCardGroup } from "@/components/RadioCardGroup";
import type { JobProfile } from "@/types/app-config";
import {
  PLAYGROUND_CHAINS,
  chainMeta,
  stagesOfChain,
  type PlaygroundChain,
} from "@/types/playground";
import { Field, Section, StageTrail } from "./ui";

export interface ChainPickerProps {
  profiles: JobProfile[];
  profileId: string;
  onSelectProfile: (id: string) => void;
  chain: PlaygroundChain;
  onSelectChain: (next: PlaygroundChain) => void;
  defaultProfileId: string;
}

/**
 * 第一步：这一趟要拿哪套配置、测哪条链路。
 *
 * 先选链路而不是先填岗位，是因为链路决定了后面要填什么——回复链路才需要造对话，
 * 筛选链路连打招呼提示词都不该出现在眼前。这一步选完，第二步的表单就只剩必要项
 */
export function ChainPicker({
  profiles,
  profileId,
  onSelectProfile,
  chain,
  onSelectChain,
  defaultProfileId,
}: ChainPickerProps) {
  const meta = chainMeta(chain);

  return (
    <div className="space-y-4">
      <Section title="求职方案" hint="决定这一趟用哪套筛选规则、模板与提示词">
        <Field label="使用的方案">
          <Select
            aria-label="求职方案"
            className="w-full md:max-w-md"
            value={profileId}
            onChange={onSelectProfile}
            options={profiles.map((profile) => ({
              value: profile.id,
              label: `${profile.name}${profile.id === defaultProfileId ? " · 默认" : ""}`,
            }))}
          />
        </Field>
      </Section>

      <Section
        title="测试链路"
        hint="一次只跑一条，其余环节不会被触发"
        extra={<Tag color="blue">不落库 · 不发消息</Tag>}
      >
        <RadioCardGroup
          ariaLabel="测试链路"
          value={chain}
          onChange={(next) => onSelectChain(next as PlaygroundChain)}
          options={PLAYGROUND_CHAINS.map((item) => ({
            value: item.chain,
            label: item.label,
            description: item.summary,
          }))}
        />

        <div className="mt-4 rounded-xl border border-slate-200 bg-slate-50/70 p-3">
          <div className="mb-2 text-[11px] font-bold uppercase tracking-widest text-slate-500">
            这条链路会经过
          </div>
          <StageTrail stages={stagesOfChain(chain)} />
          {meta.promptKey === null && (
            <div className="mt-2 text-[11px] text-slate-400">这条链路是纯本地判断，不调用大模型。</div>
          )}
        </div>
      </Section>
    </div>
  );
}
