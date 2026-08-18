import { describe, expect, it } from "vitest";
import {
  copyJobProfile,
  getDefaultJobProfile,
  getJobProfiles,
  getReplyPollingConfig,
  selectProfileAfterRemoval,
  DEFAULT_AUTO_REPLY_WINDOW_HOURS,
  DEFAULT_REPLY_POLLING_CONFIG,
  type AppRuntimeConfig,
} from "./app-config";

const legacyConfig = {
  default_job_profile_id: undefined,
  job_filter_config: { query: "Rust" },
  platform_filter_config: { liepin: {} },
  resume_config: { resume_content: "resume" },
  greet_config: { default_template: [] },
  replay_config: { templates: [] },
} as unknown as AppRuntimeConfig;

describe("job profile compatibility", () => {
  it("projects legacy top-level settings into one default profile", () => {
    const profiles = getJobProfiles(legacyConfig);

    expect(profiles).toHaveLength(1);
    expect(profiles[0]).toMatchObject({
      id: "default",
      name: "默认求职方案",
      archived: false,
      job_filter_config: { query: "Rust" },
      resume_config: { resume_content: "resume" },
    });
    expect(getDefaultJobProfile(legacyConfig)).toStrictEqual(profiles[0]);
  });

  it("prefers the configured default profile", () => {
    const fallback = getJobProfiles(legacyConfig)[0];
    const config = {
      ...legacyConfig,
      default_job_profile_id: "second",
      job_profiles: [fallback, { ...fallback, id: "second", name: "第二方案" }],
    };

    expect(getDefaultJobProfile(config).name).toBe("第二方案");
  });

  it("does not select an archived configured default profile", () => {
    const fallback = getJobProfiles(legacyConfig)[0];
    const config = {
      ...legacyConfig,
      default_job_profile_id: "archived",
      job_profiles: [{ ...fallback, id: "archived", archived: true }, { ...fallback, id: "active", name: "可用方案" }],
    };

    expect(getDefaultJobProfile(config).id).toBe("active");
  });

  it("copies strategy content under a fresh active identity", () => {
    const source = getJobProfiles(legacyConfig)[0];
    const copied = copyJobProfile({ ...source, archived: true }, "copy-id", "副本");

    expect(copied).toMatchObject({ id: "copy-id", name: "默认求职方案 · 副本", archived: false });
    expect(copied.job_filter_config).not.toBe(source.job_filter_config);
  });

  it("selects a non-archived profile after archive or deletion", () => {
    const source = getJobProfiles(legacyConfig)[0];
    const profiles = [
      { ...source, id: "removed" },
      { ...source, id: "archived", archived: true },
      { ...source, id: "active" },
    ];

    expect(selectProfileAfterRemoval(profiles, "removed")?.id).toBe("active");
    expect(selectProfileAfterRemoval([profiles[0]], "removed")).toBeNull();
  });
});

describe("reply polling config", () => {
  it("falls back to the default cadence when the block is missing entirely", () => {
    expect(getReplyPollingConfig({ reply_polling_config: undefined }))
      .toEqual(DEFAULT_REPLY_POLLING_CONFIG);
  });

  // 部分字段缺失时只补缺的那几个，用户已经调过的值不能被默认值盖掉
  it("keeps user-tuned fields while filling in the missing ones", () => {
    const merged = getReplyPollingConfig({
      reply_polling_config: { interval_minutes: 15 } as never,
    });

    expect(merged.interval_minutes).toBe(15);
    expect(merged.max_conversations_per_round)
      .toBe(DEFAULT_REPLY_POLLING_CONFIG.max_conversations_per_round);
  });

  // 默认值必须和 Rust 侧的 serde default 对齐。对不上时表现是「界面显示一套、
  // 实际跑另一套」，而两边都不报错
  it("mirrors the serde defaults declared on the Rust side", () => {
    expect(DEFAULT_REPLY_POLLING_CONFIG).toEqual({
      interval_minutes: 5,
      jitter_seconds: 120,
      active_hours_enabled: true,
      active_start_hour: 9,
      active_end_hour: 22,
      max_conversations_per_round: 10,
      humanize_delay_min_seconds: 30,
      humanize_delay_max_seconds: 120,
    });
    expect(DEFAULT_AUTO_REPLY_WINDOW_HOURS).toBe(24);
  });
});
