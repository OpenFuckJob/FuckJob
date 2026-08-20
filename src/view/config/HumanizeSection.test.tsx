import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import HumanizeSection, { describePersona } from "./HumanizeSection";
import {
  DEFAULT_HUMANIZE_CONFIG,
  type HumanizeConfig,
} from "../../types/app-config";

afterEach(cleanup);

function config(overrides: Partial<HumanizeConfig> = {}): HumanizeConfig {
  return { ...DEFAULT_HUMANIZE_CONFIG, ...overrides };
}

describe("HumanizeSection", () => {
  it("renders the three intensity choices when enabled", () => {
    render(
      <HumanizeSection
        config={config({ enabled: true })}
        onChange={vi.fn()}
      />,
    );

    expect(screen.queryByText("让 AI 操作更贴近真人行为，降低被识别风险，提升任务稳定性")).toBeNull();
    expect(screen.getByRole("radio", { name: "轻度" })).toBeTruthy();
    expect(screen.getByRole("radio", { name: "标准" })).toBeTruthy();
    expect(screen.getByRole("radio", { name: "谨慎" })).toBeTruthy();
  });

  it("persists the enabled state through the change callback", () => {
    const onChange = vi.fn();
    render(
      <HumanizeSection config={config()} onChange={onChange} />,
    );

    fireEvent.click(screen.getByRole("switch", { name: "启用拟人化" }));

    expect(onChange).toHaveBeenCalledWith({ enabled: true });
  });

  it("persists the selected intensity", () => {
    const onChange = vi.fn();
    render(
      <HumanizeSection
        config={config({ enabled: true })}
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole("radio", { name: "谨慎" }));

    expect(onChange).toHaveBeenCalledWith({ intensity: "cautious" });
  });

  it("folds the intensity choices while disabled", () => {
    render(<HumanizeSection config={config()} onChange={vi.fn()} />);

    expect(screen.queryByRole("radio", { name: "轻度" })).toBeNull();
    expect(screen.queryByRole("radio", { name: "标准" })).toBeNull();
    expect(screen.queryByRole("radio", { name: "谨慎" })).toBeNull();
    expect(screen.queryByText("拟人化已关闭，任务会按原有节奏执行。")).toBeNull();
  });
});

describe("describePersona", () => {
  it("describes disabled and not-yet-generated states", () => {
    expect(describePersona(config())).toBe("拟人化已关闭，任务会按原有节奏执行。");
    expect(describePersona(config({ enabled: true }))).toBe(
      "启用后系统会生成一套专属的操作习惯，保存配置即生效。",
    );
  });

  it("describes a generated persona without exposing its seed", () => {
    const description = describePersona(
      config({ enabled: true, persona_seed: 123456 }),
    );

    expect(description).toContain("系统已按你的专属编号生成一套操作习惯");
    expect(description).not.toContain("123456");
  });
});
