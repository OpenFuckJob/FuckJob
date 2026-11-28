import { describe, expect, it } from "vitest";
import {
  buildSafetyPresetPatch,
  describeSafetySummary,
  detectSafetyPreset,
  SAFETY_PRESETS,
} from "./safety-presets";

describe("safety presets", () => {
  it("applies and detects all three built-in presets round-trip", () => {
    for (const preset of SAFETY_PRESETS) {
      const patch = buildSafetyPresetPatch(preset.id, {
        periodic_delivery_config: undefined,
        humanize_config: { enabled: false, intensity: "standard", persona_seed: 42 },
      });
      expect(patch.humanize_config.persona_seed).toBe(42);
      expect(patch.humanize_config.enabled).toBe(true);
      expect(patch.max_greets_per_company).toBe(preset.perCompanyLimit);
      expect(
        detectSafetyPreset({
          periodic_delivery_config: patch.periodic_delivery_config,
          humanize_config: patch.humanize_config,
        }),
      ).toBe(preset.id);
      expect(
        describeSafetySummary({
          periodic_delivery_config: patch.periodic_delivery_config,
          humanize_config: patch.humanize_config,
        }),
      ).toContain(preset.label);
    }
  });

  it("reports custom mode when parameters diverge from built-in presets", () => {
    const custom = {
      periodic_delivery_config: {
        interval_minutes: 25,
        window_enabled: false,
        windows: [],
        max_run_hours: 0,
        max_greets_per_round: 18,
        max_greets_per_day: 55,
        max_round_minutes: 60,
      },
      humanize_config: { enabled: true, intensity: "standard" as const, persona_seed: 1 },
    };
    expect(detectSafetyPreset(custom)).toBe("custom");
    expect(describeSafetySummary(custom)).toBe("自定义节奏（55/天）");
  });

  it("detects custom mode when interval_minutes or active delivery windows diverge", () => {
    const standardPatch = buildSafetyPresetPatch("standard", {
      periodic_delivery_config: undefined,
      humanize_config: undefined,
    });

    expect(
      detectSafetyPreset({
        periodic_delivery_config: {
          ...standardPatch.periodic_delivery_config,
          interval_minutes: 15,
        },
        humanize_config: standardPatch.humanize_config,
      }),
    ).toBe("custom");

    expect(
      detectSafetyPreset({
        periodic_delivery_config: {
          ...standardPatch.periodic_delivery_config,
          windows: [{ start_minute: 10 * 60, end_minute: 20 * 60 }],
        },
        humanize_config: standardPatch.humanize_config,
      }),
    ).toBe("custom");

    // Adjacent split windows covering the same 9:00-18:00 span still match standard
    expect(
      detectSafetyPreset({
        periodic_delivery_config: {
          ...standardPatch.periodic_delivery_config,
          windows: [
            { start_minute: 12 * 60, end_minute: 18 * 60 },
            { start_minute: 9 * 60, end_minute: 12 * 60 },
          ],
        },
        humanize_config: standardPatch.humanize_config,
      }),
    ).toBe("standard");

    // When window_enabled is false (active preset), inactive windows are ignored
    const activePatch = buildSafetyPresetPatch("active", {
      periodic_delivery_config: undefined,
      humanize_config: undefined,
    });
    expect(
      detectSafetyPreset({
        periodic_delivery_config: {
          ...activePatch.periodic_delivery_config,
          windows: [],
        },
        humanize_config: activePatch.humanize_config,
      }),
    ).toBe("active");
  });
});
