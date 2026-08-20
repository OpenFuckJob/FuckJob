import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import HumanizeSection from "./HumanizeSection";
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
  });

  it("每档只说清投递节奏和产出代价，不暴露人格种子", () => {
    render(
      <HumanizeSection
        config={config({ enabled: true, persona_seed: 123456 })}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByText("节奏基本不变，产出几乎无损失")).toBeTruthy();
    expect(screen.getByText("投一批歇几分钟，产出降一到两成")).toBeTruthy();
    expect(screen.getByText("休息更久、动作更慢，产出明显下降")).toBeTruthy();
    expect(screen.queryByText(/123456/)).toBeNull();
  });
});
