import { Button, Typography } from "antd";
import {
  ClockCircleOutlined,
  FieldTimeOutlined,
  HourglassOutlined,
  PoweroffOutlined,
  SendOutlined,
  UndoOutlined,
} from "@ant-design/icons";
import {
  DEFAULT_PERIODIC_DELIVERY_CONFIG,
  hoursToWindows,
  isSamePeriodicDelivery,
  MAX_GREETS_PER_ROUND,
  MAX_ROUND_MINUTES,
  MAX_RUN_HOURS,
  windowsToHours,
  type DailyWindow,
  type PeriodicDeliveryConfig,
} from "../../types/app-config";
import { describeWindows } from "../../types/rpa";
import HourGrid from "@/components/HourGrid";
import { SettingGroup, SettingSlider, SettingToggle } from "@/components/SettingField";

const { Text } = Typography;

/** 常用时段组合。一格一格点出「上午投、午休停、下午再投」很费事，预设兜住高频场景 */
const WINDOW_PRESETS: Array<{ label: string; windows: DailyWindow[] }> = [
  { label: "工作时间 9-18", windows: [{ start_minute: 9 * 60, end_minute: 18 * 60 }] },
  {
    label: "上下午 9-12 / 14-18",
    windows: [
      { start_minute: 9 * 60, end_minute: 12 * 60 },
      { start_minute: 14 * 60, end_minute: 18 * 60 },
    ],
  },
  { label: "夜间 20-24", windows: [{ start_minute: 20 * 60, end_minute: 24 * 60 }] },
];

interface Props {
  config: PeriodicDeliveryConfig;
  onChange: (next: Partial<PeriodicDeliveryConfig>) => void;
  /**
   * `card` 是配置页里的独立卡片，自带标题与「恢复默认」；
   * `plain` 只出字段，外框和按钮交给调用方——启动弹窗的参数抽屉用这个，
   * 它自己的标题栏和 footer 已经承担了同样的职责，再套一层就是两层壳
   */
  variant?: "card" | "plain";
  /**
   * 「恢复默认」还原成的那份配置，与当前值相同时按钮置灰。仅 `card` 形态渲染。
   *
   * 两处的「默认」不是同一件事：配置页恢复的是出厂值，启动弹窗恢复的是用户
   * 自己在配置页存下的那套。所以基准由调用方给，组件不自作主张
   */
  resetTo?: PeriodicDeliveryConfig;
  resetLabel?: string;
}

/** 当前值是否已经等于基准，用来决定「恢复默认」该不该置灰 */
export function canResetPeriodicDelivery(
  config: PeriodicDeliveryConfig,
  resetTo: PeriodicDeliveryConfig,
): boolean {
  return !isSamePeriodicDelivery(config, resetTo);
}

/**
 * 周期投递的运行节奏。
 *
 * 配置页和启动弹窗的参数抽屉共用同一套控件：抽屉里改的是那一次任务，配置页改的
 * 是下次的初值。两处若各写一份表单，迟早会出现「抽屉有这项、配置页没有」的漂移。
 *
 * 「单轮上限」这两项不是可有可无的调优旋钮——岗位列表几乎是无限的，一轮不设上界
 * 就可能跑几个小时，两轮之间的空闲期永远轮不到，表现就是它一直在投递、从不回复。
 */
export default function PeriodicDeliverySection({
  config,
  onChange,
  variant = "card",
  resetTo,
  resetLabel = "恢复默认",
}: Props) {
  const fields = (
    <div className="space-y-3">
      <SettingGroup>
        <SettingSlider
          icon={<ClockCircleOutlined />}
          title="投递间隔"
          description="上一轮结束到下一轮开始之间的间隔，这段时间用来自动回复未读"
          min={1}
          max={1440}
          sliderMax={180}
          step={5}
          fallback={DEFAULT_PERIODIC_DELIVERY_CONFIG.interval_minutes}
          value={config.interval_minutes}
          unit="分钟"
          onChange={(value) => onChange({ interval_minutes: value })}
        />
        <SettingSlider
          icon={<SendOutlined />}
          title="单轮最多打招呼"
          description="到顶就结束本轮、进入空闲期，下一轮继续。不设上限时一轮能跑几个小时，中间的未读消息全得排队等着"
          min={0}
          max={MAX_GREETS_PER_ROUND}
          fallback={DEFAULT_PERIODIC_DELIVERY_CONFIG.max_greets_per_round}
          value={config.max_greets_per_round}
          unit="条"
          valueLabel={(value) => (value === 0 ? "不限制" : null)}
          onChange={(value) => onChange({ max_greets_per_round: value })}
        />
        <SettingSlider
          icon={<HourglassOutlined />}
          title="单轮最长运行"
          description="另一道保险：岗位再多，本轮跑满这么久也会收尾"
          min={0}
          max={MAX_ROUND_MINUTES}
          step={5}
          fallback={DEFAULT_PERIODIC_DELIVERY_CONFIG.max_round_minutes}
          value={config.max_round_minutes}
          unit="分钟"
          valueLabel={(value) => (value === 0 ? "不限制" : null)}
          onChange={(value) => onChange({ max_round_minutes: value })}
        />
      </SettingGroup>

      <SettingGroup>
        <SettingToggle
          icon={<FieldTimeOutlined />}
          title="只在指定时段投递"
          description="时段外不投递，但仍然继续自动回复未读；到点自动恢复投递"
          checked={config.window_enabled}
          onChange={(checked) => onChange({ window_enabled: checked })}
        >
          {config.window_enabled && (
            <div className="space-y-2">
              <HourGrid
                value={windowsToHours(config.windows)}
                onChange={(hours) => onChange({ windows: hoursToWindows(hours) })}
              />
              <div className="flex flex-wrap items-center gap-2 text-xs">
                <span className="text-slate-400">快捷</span>
                {WINDOW_PRESETS.map((preset) => (
                  <Button
                    key={preset.label}
                    size="small"
                    onClick={() => onChange({ windows: preset.windows })}
                  >
                    {preset.label}
                  </Button>
                ))}
              </div>
              <div className={config.windows.length === 0 ? "text-xs text-amber-600" : "text-xs text-slate-500"}>
                {config.windows.length === 0
                  ? "一格都没选，任务不会投递；点上方格子选出可投的小时"
                  : `当前：${describeWindows(config.windows)}`}
              </div>
            </div>
          )}
        </SettingToggle>
        <SettingSlider
          icon={config.max_run_hours > 0 ? <PoweroffOutlined /> : <HourglassOutlined />}
          title="自动结束"
          description="从任务启动开始计时，到点整个任务收工。不设则一直跑到你手动停止"
          min={0}
          max={MAX_RUN_HOURS}
          fallback={DEFAULT_PERIODIC_DELIVERY_CONFIG.max_run_hours}
          value={config.max_run_hours}
          unit="小时后"
          valueLabel={(value) => (value === 0 ? "不自动结束" : null)}
          onChange={(value) => onChange({ max_run_hours: value })}
        />
      </SettingGroup>
    </div>
  );

  if (variant === "plain") {
    return fields;
  }

  return (
    <div className="space-y-4 rounded-2xl border border-slate-200/80 bg-white/85 p-6">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <Text className="block font-bold text-slate-900">周期投递</Text>
          <Text className="text-xs text-slate-500">
            这里改的是启动任务时的初值，每次启动仍可单独调整；已在跑的任务不受影响
          </Text>
        </div>
        {resetTo && (
          <Button
            type="text"
            size="small"
            icon={<UndoOutlined />}
            disabled={!canResetPeriodicDelivery(config, resetTo)}
            onClick={() => onChange(resetTo)}
          >
            {resetLabel}
          </Button>
        )}
      </div>
      {fields}
    </div>
  );
}
