import { Radio, Typography } from "antd";
import { ExperimentOutlined, SafetyCertificateOutlined } from "@ant-design/icons";
import {
  type HumanizeConfig,
  type HumanizeIntensity,
} from "../../types/app-config";
import { SettingGroup, SettingToggle } from "@/components/SettingField";

const { Text } = Typography;

/**
 * 强度档位。
 *
 * 每档只说「会发生什么」和「代价是什么」——具体数字由后端按当天的人格现算，
 * 写死在这里迟早和实际行为对不上，而对不上的说明比没有说明更糟
 */
const INTENSITY_OPTIONS: Array<{
  value: HumanizeIntensity;
  label: string;
  description: string;
  cost: string;
}> = [
  {
    value: "light",
    label: "轻度",
    description: "只在既有节奏上小幅抖动，投递量基本不变",
    cost: "产出几乎无损失",
  },
  {
    value: "standard",
    label: "标准",
    description: "投一批歇几分钟，偶尔跳过一个岗位、停下来发会儿呆",
    cost: "产出约降一到两成",
  },
  {
    value: "cautious",
    label: "谨慎",
    description: "休息更勤更久、跳过更多、动作更慢，适合已经被限制过的账号",
    cost: "产出明显下降",
  },
];

interface Props {
  config: HumanizeConfig;
  onChange: (next: Partial<HumanizeConfig>) => void;
}

/**
 * 拟人化。
 *
 * 这里刻意只有一个开关和三个档位：休息阈值、停顿长度、打字速度这些具体数字
 * 不做成旋钮，而是由系统按一个长期不变的「人格种子」每天现算一套。
 * 一组固定的数字——哪怕是用户自己填的——本身就是可被识别的模式。
 */
export default function HumanizeSection({ config, onChange }: Props) {
  return (
    <div className="space-y-4 rounded-2xl border border-slate-200/80 bg-white/85 p-6">
      <div>
        <Text className="block font-bold text-slate-900">拟人化</Text>
        <Text className="text-xs text-slate-500">
          让投递节奏和鼠标、键盘动作带上真人的不确定性。已在跑的任务不受影响
        </Text>
      </div>

      <SettingGroup>
        <SettingToggle
          icon={<SafetyCertificateOutlined />}
          title="启用拟人化"
          description="平台看的不是单次动作像不像人，而是长期模式：每条都隔 4 秒、每轮都正好 30 条，连起来就是一条没有呼吸的直线"
          checked={config.enabled}
          onChange={(enabled) => onChange({ enabled })}
        >
          {config.enabled && (
            <div className="space-y-3">
              <Radio.Group
                value={config.intensity}
                onChange={(event) => onChange({ intensity: event.target.value as HumanizeIntensity })}
                className="w-full"
              >
                <div className="space-y-2">
                  {INTENSITY_OPTIONS.map((option) => (
                    <Radio
                      key={option.value}
                      value={option.value}
                      className="flex w-full items-start rounded-xl border border-slate-200 bg-white px-3 py-2"
                    >
                      <div className="min-w-0">
                        <div className="text-sm font-semibold text-slate-900">{option.label}</div>
                        <div className="mt-0.5 text-xs leading-relaxed text-slate-500">
                          {option.description}
                        </div>
                        <div className="mt-0.5 text-xs text-amber-600">{option.cost}</div>
                      </div>
                    </Radio>
                  ))}
                </div>
              </Radio.Group>

              <div className="flex items-start gap-2 rounded-xl bg-slate-50 px-3 py-2 text-xs leading-relaxed text-slate-500">
                <ExperimentOutlined className="mt-0.5 shrink-0 text-slate-400" />
                <span>{describePersona(config)}</span>
              </div>
            </div>
          )}
        </SettingToggle>
      </SettingGroup>
    </div>
  );
}

/**
 * 说明当前这套策略从哪来。
 *
 * 不展示具体数字：界面上的数字是渲染那一刻算的，而真正生效的是任务启动时
 * 后端按当天日期算的那套，两者对不上时用户只会怀疑功能坏了
 */
export function describePersona(config: HumanizeConfig): string {
  if (!config.enabled) {
    return "关闭时投递节奏与改造前完全一致。";
  }
  if (!config.persona_seed) {
    return "启用后系统会生成一套专属的操作习惯，保存配置即生效。";
  }
  return (
    "系统已按你的专属编号生成一套操作习惯：手速、歇多久、什么时候跳过一个岗位，" +
    "当天固定不变，每天自动换一套。具体节奏会写在任务日志里。"
  );
}
