import { Radio } from "antd";
import {
  AimOutlined,
  BarChartOutlined,
  CoffeeOutlined,
  InfoCircleOutlined,
  LineChartOutlined,
  SafetyCertificateOutlined,
  UserOutlined,
} from "@ant-design/icons";
import type { ReactNode } from "react";
import {
  type HumanizeConfig,
  type HumanizeIntensity,
} from "../../types/app-config";
import { SettingGroup, SettingToggle } from "@/components/SettingField";

/**
 * 强度档位只表达用户能感知到的行为差异，具体节奏仍由后端按人格种子推导。
 * 这样 UI 不会把某一组固定数字误认为是运行时的硬编码参数。
 */
const INTENSITY_OPTIONS: Array<{
  value: HumanizeIntensity;
  label: string;
  description: string;
  cost: string;
  icon: ReactNode;
  badge: string;
  badgeIcon: ReactNode;
}> = [
  {
    value: "light",
    label: "轻度",
    description: "仅在既有节奏上小幅抖动，投递节奏基本不变",
    cost: "产出几乎无损失",
    icon: <AimOutlined />,
    badge: "稳定优先",
    badgeIcon: <BarChartOutlined />,
  },
  {
    value: "standard",
    label: "标准",
    description: "投一批歇几分钟，偶尔跳过一个岗位、停下来发会儿呆",
    cost: "产出约降一到两成",
    icon: <UserOutlined />,
    badge: "平衡推荐",
    badgeIcon: <LineChartOutlined />,
  },
  {
    value: "cautious",
    label: "谨慎",
    description: "休息更勤更久、跳过更多、动作更慢，适合已被限制过的账号",
    cost: "产出明显下降",
    icon: <CoffeeOutlined />,
    badge: "风控优先",
    badgeIcon: <SafetyCertificateOutlined />,
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
        description="模拟真人的操作节奏和行为模式，减少机械化特征"
        checked={config.enabled}
        onChange={(enabled) => onChange({ enabled })}
      >
        {config.enabled && (
          <div className="space-y-2">
            <Radio.Group
              aria-label="拟人化强度"
              value={config.intensity}
              onChange={(event) => onChange({ intensity: event.target.value as HumanizeIntensity })}
              className="!block"
            >
              <div className="space-y-1.5">
                {INTENSITY_OPTIONS.map((option) => {
                  const selected = config.intensity === option.value;
                  return (
                    <Radio
                      key={option.value}
                      value={option.value}
                      aria-label={option.label}
                      className={`!m-0 !flex !w-full !items-center !gap-2 !rounded-lg !border !px-2.5 !py-2 transition-colors ${
                        selected
                          ? "!border-sky-400 !bg-sky-50/60"
                          : "!border-slate-200 !bg-white hover:!border-sky-200 hover:!bg-slate-50/60"
                      }`}
                    >
                      <div className="flex min-w-0 flex-1 items-center gap-2">
                        <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-sky-50 text-sm text-sky-600">
                          {option.icon}
                        </span>
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-baseline gap-x-2">
                            <span className="text-sm font-semibold leading-5 text-slate-900">
                              {option.label}
                            </span>
                            <span className="text-xs leading-5 text-slate-500">
                              {option.description}
                            </span>
                          </div>
                          <span className="block text-[11px] leading-4 text-amber-600">
                            {option.cost}
                          </span>
                        </div>
                        <span className={`inline-flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium ${
                          selected ? "bg-sky-100 text-sky-700" : "bg-slate-50 text-slate-500"
                        }`}>
                          {option.badgeIcon}
                          {option.badge}
                        </span>
                      </div>
                    </Radio>
                  );
                })}
              </div>
            </Radio.Group>

            <div className="flex items-start gap-2 rounded-lg bg-slate-50 px-2.5 py-2 text-xs leading-5 text-slate-500">
              <InfoCircleOutlined className="mt-0.5 shrink-0 text-blue-600" />
              <span className="whitespace-pre-line">{describePersona(config)}</span>
            </div>
          </div>
        )}
      </SettingToggle>
    </SettingGroup>
  );
}

/**
 * 说明当前人格策略的来源，不展示运行时的具体随机数字。
 * 任务启动时后端会根据人格种子和当天日期推导实际节奏。
 */
export function describePersona(config: HumanizeConfig): string {
  if (!config.enabled) {
    return "拟人化已关闭，任务会按原有节奏执行。";
  }
  if (!config.persona_seed) {
    return "启用后系统会生成一套专属的操作习惯，保存配置即生效。";
  }
  return (
    "同一账号在不同天的表现也会随机化而略有差异。"
  );
}
