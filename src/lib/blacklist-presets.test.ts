import { describe, expect, it } from "vitest";
import { BLACKLIST_PRESETS, mergeUniqueKeywords } from "./blacklist-presets";

describe("blacklist presets", () => {
  it("provides curated non-empty anti-noise presets", () => {
    expect(BLACKLIST_PRESETS.length).toBeGreaterThanOrEqual(4);
    for (const preset of BLACKLIST_PRESETS) {
      expect(preset.keywords.length).toBeGreaterThan(0);
    }
  });

  it("merges incoming keywords incrementally without duplicates or blank entries", () => {
    const merged = mergeUniqueKeywords(
      ["软通动力", "外包"],
      ["  外包 ", "OD", "od", "", "中软国际"],
    );
    expect(merged).toEqual(["软通动力", "外包", "OD", "中软国际"]);
  });
});
