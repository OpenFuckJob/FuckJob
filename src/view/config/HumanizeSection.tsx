import { Radio } from "antd";
import { SafetyCertificateOutlined } from "@ant-design/icons";
import {
  type HumanizeConfig,
  type HumanizeIntensity,
} from "../../types/app-config";
import { SettingGroup, SettingToggle } from "@/components/SettingField";

/**
 * 强度档位只表达用户能感知到的行为差异，具体节奏仍由后端按人格种子推导。
 * 这样 UI 不会把某一组固定数字误认为是运行时的硬编码参数。
 *
 * 每档一句话说清「怎么投」和「少投多少」——挑档位时真正要权衡的就这两件事，
 * 图标、标签、单列的产出说明都只是在重复它。
 */
const INTENSITY_OPTIONS: Array<{
  value: HumanizeIntensity;
  label: string;
  description: string;
  recommended?: boolean;
}> = [
  {
    value: "light",
    label: "轻度",
    description: "节奏基本不变，产出几乎无损失",
  },
  {
    value: "standard",
    label: "标准",
    description: "投一批歇几分钟，产出降一到两成",
    recommended: true,
  },
  {
    value: "cautious",
    label: "谨慎",
    description: "休息更久、动作更慢，产出明显下降",
  },
];

interface Props {
  config: HumanizeConfig;
  onChange: (next: Partial<HumanizeConfig>) => void;
}

/**
 * 拟人化设置。
 *
 * 人格种子不是用户可编辑的参数：它由后端首次启用时生成，并在同一天内保持稳定。
 * 这里把重点放在「是否启用」和「行为倾向」上，避免把实现细节堆进配置表单。
 */
export default function HumanizeSection({ config, onChange }: Props) {
  return (
    <SettingGroup>
      <SettingToggle
        icon={<SafetyCertificateOutlined />}
        title="启用拟人化"
        description="模拟真人的操作节奏，同一账号每天的表现也会略有差异"
        checked={config.enabled}
        onChange={(enabled) => onChange({ enabled })}
      >
        {config.enabled && (
          <Radio.Group
            aria-label="拟人化强度"
            value={config.intensity}
            onChange={(event) => onChange({ intensity: event.target.value as HumanizeIntensity })}
            className="!block"
          >
            <div className="space-y-2">
              {INTENSITY_OPTIONS.map((option) => (
                <Radio
                  key={option.value}
                  value={option.value}
                  aria-label={option.label}
                  className={`!m-0 !flex !w-full !items-center !gap-2.5 !rounded-lg !border !px-3 !py-2.5 transition-colors ${
                    config.intensity === option.value
                      ? "!border-sky-400 !bg-sky-50/60"
                      : "!border-slate-200 !bg-white hover:!border-sky-200 hover:!bg-slate-50/60"
                  }`}
                >
                  <span className="flex min-w-0 flex-wrap items-baseline gap-x-2">
                    <span className="text-sm font-semibold leading-5 text-slate-900">
                      {option.label}
                    </span>
                    {option.recommended && (
                      <span className="rounded bg-sky-100 px-1 text-[10px] font-medium leading-4 text-sky-700">
                        推荐
                      </span>
                    )}
                    <span className="text-xs leading-5 text-slate-500">
                      {option.description}
                    </span>
                  </span>
                </Radio>
              ))}
            </div>
          </Radio.Group>
        )}
      </SettingToggle>
    </SettingGroup>
  );
}
