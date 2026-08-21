import { SafetyCertificateOutlined } from "@ant-design/icons";
import {
  type HumanizeConfig,
  type HumanizeIntensity,
} from "../../types/app-config";
import { SettingGroup, SettingToggle } from "@/components/SettingField";
import { RadioCardGroup, type RadioCardOption } from "@/components/RadioCardGroup";

/**
 * 强度档位只表达用户能感知到的行为差异，具体节奏仍由后端按人格种子推导。
 * 这样 UI 不会把某一组固定数字误认为是运行时的硬编码参数。
 *
 * 每档一句话说清「怎么投」和「少投多少」——挑档位时真正要权衡的就这两件事，
 * 图标、标签、单列的产出说明都只是在重复它。
 */
const INTENSITY_OPTIONS: RadioCardOption<HumanizeIntensity>[] = [
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
          <RadioCardGroup
            ariaLabel="拟人化强度"
            value={config.intensity}
            options={INTENSITY_OPTIONS}
            onChange={(intensity) => onChange({ intensity })}
          />
        )}
      </SettingToggle>
    </SettingGroup>
  );
}
