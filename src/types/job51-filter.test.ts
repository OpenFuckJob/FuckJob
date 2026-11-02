import { describe, expect, it } from "vitest";
import { getPlatformFilterConfig, type AppRuntimeConfig } from "./app-config";

describe("51job platform filters", () => {
  it("adds empty 51job filters to old profiles without borrowing BOSS options", () => {
    const source = { platform_filter_config: { boss: { active_filter_enabled: false }, liepin: {} } } as unknown as AppRuntimeConfig;
    expect(getPlatformFilterConfig(source).job51).toEqual({ salary: [], functions: [], company_size: [] });
  });
  it("keeps all three independent choices through profile normalization", () => {
    const chosen = { salary: ["201", "06"], functions: ["0121", "A0JQ"], company_size: ["01", "02"] };
    const source = { platform_filter_config: { job51: chosen, boss: {}, liepin: {} } } as unknown as AppRuntimeConfig;
    const normalized = getPlatformFilterConfig(source);
    expect(normalized.job51).toEqual(chosen);
    expect(normalized.boss).not.toHaveProperty("salary");
  });
});
