import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AiFeatureGate } from "./AiFeatureGate";

describe("AiFeatureGate", () => {
  afterEach(cleanup);
  it("renders the AI control when configured", () => {
    render(<AiFeatureGate configured onConfigure={() => {}}><button>开始分析</button></AiFeatureGate>);
    expect(screen.getByRole("button", { name: "开始分析" })).toBeEnabled();
  });

  it("blocks the control and links to model configuration", () => {
    const onConfigure = vi.fn();
    render(<AiFeatureGate configured={false} onConfigure={onConfigure}><button>开始分析</button></AiFeatureGate>);
    expect(screen.queryByRole("button", { name: "开始分析" })).not.toBeInTheDocument();
    expect(screen.getByText(/AI 是可选功能/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "配置大模型" }));
    expect(onConfigure).toHaveBeenCalledOnce();
  });
});
