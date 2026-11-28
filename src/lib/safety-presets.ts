import {
  DEFAULT_MAX_GREETS_PER_DAY,
  getHumanizeConfig,
  getPeriodicDeliveryConfig,
  type AppRuntimeConfig,
  type DailyWindow,
  type HumanizeConfig,
  type PeriodicDeliveryConfig,
} from "@/types/app-config";

export type SafetyPresetId = "conservative" | "standard" | "active";

export interface SafetyPreset {
  id: SafetyPresetId;
  label: string;
  badge?: string;
  tagline: string;
  summary: string;
  dailyLimit: number;
  roundLimit: number;
  perCompanyLimit: number;
  intervalMinutes: number;
  windowEnabled: boolean;
  windows: PeriodicDeliveryConfig["windows"];
  humanizeIntensity: HumanizeConfig["intensity"];
}

export const SAFETY_PRESETS: SafetyPreset[] = [
  {
    id: "conservative",
    label: "保守防封",
    tagline: "新号或刚被提醒过时优先选它",
    summary: "每日 ≤ 20 条 · 单轮 15 条 · 9:00-18:00 · 谨慎拟人化",
    dailyLimit: 20,
    roundLimit: 15,
    perCompanyLimit: 2,
    intervalMinutes: 45,
    windowEnabled: true,
    windows: [{ start_minute: 9 * 60, end_minute: 18 * 60 }],
    humanizeIntensity: "cautious",
  },
  {
    id: "standard",
    label: "标准防封",
    badge: "推荐",
    tagline: "兼顾投递效率与账号安全",
    summary: "每日 ≤ 40 条 · 单轮 25 条 · 9:00-18:00 · 标准拟人化",
    dailyLimit: 40,
    roundLimit: 25,
    perCompanyLimit: 2,
    intervalMinutes: 30,
    windowEnabled: true,
    windows: [{ start_minute: 9 * 60, end_minute: 18 * 60 }],
    humanizeIntensity: "standard",
  },
  {
    id: "active",
    label: "积极投递",
    tagline: "老号冲刺期，提升每日覆盖面",
    summary: "每日 ≤ 70 条 · 单轮 35 条 · 全天可投 · 轻度拟人化",
    dailyLimit: 70,
    roundLimit: 35,
    perCompanyLimit: 3,
    intervalMinutes: 20,
    windowEnabled: false,
    windows: [{ start_minute: 9 * 60, end_minute: 21 * 60 }],
    humanizeIntensity: "light",
  },
];

export function getSafetyPreset(id: SafetyPresetId): SafetyPreset {
  return SAFETY_PRESETS.find((preset) => preset.id === id) ?? SAFETY_PRESETS[1];
}

export function buildSafetyPresetPatch(
  presetId: SafetyPresetId,
  current: Pick<AppRuntimeConfig, "periodic_delivery_config" | "humanize_config">,
): {
  periodic_delivery_config: PeriodicDeliveryConfig;
  humanize_config: HumanizeConfig;
  max_greets_per_company: number;
} {
  const preset = getSafetyPreset(presetId);
  const periodic = getPeriodicDeliveryConfig(current);
  const humanize = getHumanizeConfig(current);

  return {
    periodic_delivery_config: {
      ...periodic,
      interval_minutes: preset.intervalMinutes,
      window_enabled: preset.windowEnabled,
      windows: preset.windows.map((w) => ({ ...w })),
      max_greets_per_round: preset.roundLimit,
      max_greets_per_day: preset.dailyLimit,
    },
    humanize_config: {
      ...humanize,
      enabled: true,
      intensity: preset.humanizeIntensity,
    },
    max_greets_per_company: preset.perCompanyLimit,
  };
}

function normalizeWindows(windows: DailyWindow[]): DailyWindow[] {
  const sorted = windows
    .map((w) => ({ start_minute: w.start_minute, end_minute: w.end_minute }))
    .sort((a, b) => a.start_minute - b.start_minute || a.end_minute - b.end_minute);
  const merged: DailyWindow[] = [];
  for (const w of sorted) {
    const prev = merged[merged.length - 1];
    if (
      prev &&
      w.start_minute <= prev.end_minute &&
      w.start_minute < w.end_minute &&
      prev.start_minute < prev.end_minute
    ) {
      prev.end_minute = Math.max(prev.end_minute, w.end_minute);
    } else {
      merged.push(w);
    }
  }
  return merged;
}

function sameNormalizedWindows(left: DailyWindow[], right: DailyWindow[]): boolean {
  const a = normalizeWindows(left);
  const b = normalizeWindows(right);
  return (
    a.length === b.length &&
    a.every(
      (window, index) =>
        window.start_minute === b[index].start_minute &&
        window.end_minute === b[index].end_minute,
    )
  );
}

export function detectSafetyPreset(
  config: Pick<AppRuntimeConfig, "periodic_delivery_config" | "humanize_config">,
): SafetyPresetId | "custom" {
  const periodic = getPeriodicDeliveryConfig(config);
  const humanize = getHumanizeConfig(config);
  const daily = periodic.max_greets_per_day ?? DEFAULT_MAX_GREETS_PER_DAY;

  for (const preset of SAFETY_PRESETS) {
    if (
      daily === preset.dailyLimit &&
      periodic.max_greets_per_round === preset.roundLimit &&
      periodic.interval_minutes === preset.intervalMinutes &&
      periodic.window_enabled === preset.windowEnabled &&
      (!preset.windowEnabled || sameNormalizedWindows(periodic.windows, preset.windows)) &&
      humanize.enabled &&
      humanize.intensity === preset.humanizeIntensity
    ) {
      return preset.id;
    }
  }
  return "custom";
}

export function describeSafetySummary(
  config: Pick<AppRuntimeConfig, "periodic_delivery_config" | "humanize_config">,
): string {
  const presetId = detectSafetyPreset(config);
  const periodic = getPeriodicDeliveryConfig(config);
  const daily = periodic.max_greets_per_day ?? DEFAULT_MAX_GREETS_PER_DAY;
  if (presetId !== "custom") {
    const preset = getSafetyPreset(presetId);
    return `${preset.label}（${preset.dailyLimit}/天）`;
  }
  return daily > 0 ? `自定义节奏（${daily}/天）` : "自定义节奏（不限日额）";
}
