import { Button, Typography } from "antd";
import {
  ClockCircleOutlined,
  DeploymentUnitOutlined,
  FieldTimeOutlined,
  HourglassOutlined,
  MessageOutlined,
} from "@ant-design/icons";
import {
  DEFAULT_REPLY_POLLING_CONFIG,
  HOURS_PER_DAY,
  type ReplyPollingConfig,
} from "../../types/app-config";
import HourGrid from "@/components/HourGrid";
import {
  SettingGroup,
  SettingHeading,
  SettingSlider,
  SettingToggle,
} from "@/components/SettingField";

const { Text } = Typography;

const REPLY_WINDOW_PRESETS: Array<{ label: string; start: number; end: number }> = [
  { label: "工作时间 9-18", start: 9, end: 18 },
  { label: "白天 9-22", start: 9, end: 22 },
  { label: "夜间 22-08", start: 22, end: 8 },
  { label: "全天", start: 0, end: 0 },
];

const clampStartHour = (hour: number) =>
  Math.min(Math.max(Math.round(hour), 0), HOURS_PER_DAY - 1);
const clampEndHour = (hour: number) =>
  Math.min(Math.max(Math.round(hour), 0), HOURS_PER_DAY);
const hourLabel = (hour: number) =>
  `${String(hour).padStart(2, "0")}:00`;

export function replyActiveHoursToGrid(
  config: Pick<ReplyPollingConfig, "active_start_hour" | "active_end_hour">,
): boolean[] {
  const start = clampStartHour(config.active_start_hour);
  const end = clampEndHour(config.active_end_hour);
  if (start === end) return Array.from({ length: HOURS_PER_DAY }, () => true);
  return Array.from({ length: HOURS_PER_DAY }, (_, hour) =>
    start < end ? hour >= start && hour < end : hour >= start || hour < end,
  );
}

export function replyGridToActiveHours(hours: boolean[]): Pick<
  ReplyPollingConfig,
  "active_start_hour" | "active_end_hour"
> {
  const selected = hours
    .slice(0, HOURS_PER_DAY)
    .map((enabled, hour) => (enabled ? hour : null))
    .filter((hour): hour is number => hour !== null);
  if (selected.length === 0 || selected.length === HOURS_PER_DAY) {
    return { active_start_hour: 0, active_end_hour: 0 };
  }

  let largestGap = -1;
  let start = selected[0];
  let end = (selected[selected.length - 1] + 1) % HOURS_PER_DAY;
  for (let index = 0; index < selected.length; index += 1) {
    const current = selected[index];
    const next = selected[(index + 1) % selected.length];
    const gap = (next - current - 1 + HOURS_PER_DAY) % HOURS_PER_DAY;
    if (gap > largestGap) {
      largestGap = gap;
      start = next;
      end = (current + 1) % HOURS_PER_DAY;
    }
  }

  return { active_start_hour: start, active_end_hour: end };
}

export function describeReplyActiveHours(
  config: Pick<ReplyPollingConfig, "active_start_hour" | "active_end_hour">,
): string {
  const start = clampStartHour(config.active_start_hour);
  const end = clampEndHour(config.active_end_hour);
  return start === end || (start === 0 && end === HOURS_PER_DAY)
    ? "全天回复"
    : `${hourLabel(start)}-${hourLabel(end)} 回复`;
}

interface Props {
  config: ReplyPollingConfig;
  onChange: (next: Partial<ReplyPollingConfig>) => void;
}

/**
 * 自动回复的轮询节奏。
 *
 * 默认值整体偏保守，取舍点是「不被认成机器人」而不是「回得最快」：
 * 固定节律、秒回、凌晨活跃，这三样单拎出来都是明显的自动化特征。
 */
export default function ReplyPollingSection({ config, onChange }: Props) {
  const delayInverted =
    config.humanize_delay_max_seconds < config.humanize_delay_min_seconds;

  return (
    <div className="rounded-2xl border border-slate-200/80 bg-white/85 p-6 space-y-4">
      <div>
        <Text className="text-slate-900 font-bold block">轮询节奏</Text>
        <Text className="text-slate-500 text-xs">
          「轮询回复」任务和「周期投递」的等待期都按这套节奏检查新消息
        </Text>
      </div>

      <SettingGroup>
        <SettingSlider
          icon={<ClockCircleOutlined />}
          title="检查间隔"
          description="多久去看一次有没有新消息，越短系统负载越高"
          min={1}
          max={60}
          fallback={DEFAULT_REPLY_POLLING_CONFIG.interval_minutes}
          value={config.interval_minutes}
          unit="分钟"
          onChange={(value) => onChange({ interval_minutes: value })}
        />
        <SettingSlider
          icon={<DeploymentUnitOutlined />}
          title="随机浮动"
          description="在间隔上叠加的随机量，固定节律本身就像机器"
          min={0}
          max={300}
          step={5}
          fallback={DEFAULT_REPLY_POLLING_CONFIG.jitter_seconds}
          value={config.jitter_seconds}
          unit="秒内"
          valueLabel={(value) => (value === 0 ? "不浮动" : null)}
          onChange={(value) => onChange({ jitter_seconds: value })}
        />
        <SettingSlider
          icon={<MessageOutlined />}
          title="单轮最多处理"
          description="未读堆积时超出的部分留到下一轮，避免跑得比间隔还久"
          min={1}
          max={100}
          fallback={DEFAULT_REPLY_POLLING_CONFIG.max_conversations_per_round}
          value={config.max_conversations_per_round}
          unit="个会话"
          onChange={(value) =>
            onChange({ max_conversations_per_round: value })
          }
        />
      </SettingGroup>

      <SettingGroup>
        <SettingToggle
          icon={<FieldTimeOutlined />}
          title="只在指定时段回复"
          description="凌晨三点秒回 HR 是最明显的机器人特征。只管回复，投递不受影响"
          checked={config.active_hours_enabled}
          onChange={(checked) => onChange({ active_hours_enabled: checked })}
        >
          {config.active_hours_enabled && (
            <div className="space-y-2">
              <HourGrid
                label="回复时段"
                value={replyActiveHoursToGrid(config)}
                onChange={(hours) => onChange(replyGridToActiveHours(hours))}
              />
              <div className="flex flex-wrap items-center gap-2 text-xs">
                <span className="text-slate-400">快捷</span>
                {REPLY_WINDOW_PRESETS.map((preset) => (
                  <Button
                    key={preset.label}
                    size="small"
                    onClick={() => onChange({
                      active_start_hour: preset.start,
                      active_end_hour: preset.end,
                    })}
                  >
                    {preset.label}
                  </Button>
                ))}
              </div>
              <div className="text-xs text-slate-500">
                当前：{describeReplyActiveHours(config)}
              </div>
            </div>
          )}
        </SettingToggle>
      </SettingGroup>

      <SettingGroup>
        <SettingHeading
          icon={<HourglassOutlined />}
          title="回复前等待"
          description="指「对方发出消息」到「我方回复」的目标间隔。检查间隔本身通常已经把它填满，只有刚好撞上对方发完消息才真的等"
        />
        <SettingSlider
          inset
          title="最少等待"
          min={0}
          max={180}
          step={5}
          fallback={DEFAULT_REPLY_POLLING_CONFIG.humanize_delay_min_seconds}
          value={config.humanize_delay_min_seconds}
          unit="秒"
          valueLabel={(value) => (value === 0 ? "立即回复" : null)}
          onChange={(value) => onChange({ humanize_delay_min_seconds: value })}
        />
        <SettingSlider
          inset
          title="最多等待"
          hint={delayInverted ? "小于最少值时按最少值固定等待" : undefined}
          min={0}
          max={300}
          step={5}
          fallback={DEFAULT_REPLY_POLLING_CONFIG.humanize_delay_max_seconds}
          value={config.humanize_delay_max_seconds}
          unit="秒"
          onChange={(value) => onChange({ humanize_delay_max_seconds: value })}
        />
      </SettingGroup>
    </div>
  );
}
