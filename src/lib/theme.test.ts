import { describe, expect, it } from "vitest";
import { nextThemeMode, resolveIsDark } from "./theme";

describe("theme mode helpers", () => {
  it("cycles system -> light -> dark -> system", () => {
    expect(nextThemeMode("system")).toBe("light");
    expect(nextThemeMode("light")).toBe("dark");
    expect(nextThemeMode("dark")).toBe("system");
  });

  it("resolves dark flag accurately for explicit and system modes", () => {
    expect(resolveIsDark("dark", false)).toBe(true);
    expect(resolveIsDark("light", true)).toBe(false);
    expect(resolveIsDark("system", true)).toBe(true);
    expect(resolveIsDark("system", false)).toBe(false);
  });
});
