import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { PipelineView } from "./PipelineView";
import type { StepResult } from "@/types/playground";

// vitest 没开 globals，testing-library 的自动清理不会注册
afterEach(cleanup);

const steps: StepResult[] = [
  { stage: "regex_filter", outcome: { kind: "pass" }, detail: { matched_rule: "后端优先" } },
  { stage: "semantic_match", outcome: { kind: "pass" }, detail: { score: 82, verdict: "方向吻合" } },
  { stage: "greet_decide", outcome: { kind: "pass" }, detail: { action: "send" } },
  { stage: "greet_compose", outcome: { kind: "pass" }, detail: { messages: ["你好", "我是候选人", "方便聊聊吗"] } },
  { stage: "greet_vet", outcome: { kind: "block", reason: "话术里还留着 {company} 占位符" }, detail: { issues: ["占位符未替换"] } },
];

describe("PipelineView", () => {
  it("没跑过时给引导而不是十行灰条", () => {
    render(<PipelineView steps={[]} hasRun={false} />);
    expect(screen.getByText(/跑筛选/)).toBeInTheDocument();
    expect(screen.queryByTestId("pipeline")).toBeNull();
  });

  it("把每个环节渲染成对应状态，没跑到的留灰", () => {
    render(<PipelineView steps={steps} hasRun />);

    // 十个环节一个不少，跑到的五个 + 没跑到的五个
    expect(screen.getByText("正则与关键词筛选")).toBeInTheDocument();
    expect(screen.getByText("回复闸门")).toBeInTheDocument();

    expect(screen.getByRole("button", { name: /正则与关键词筛选/ })).toBeEnabled();
    // 没执行的环节不可点开，避免让人以为里面藏了内容
    expect(screen.getByRole("button", { name: /回复闸门/ })).toBeDisabled();

    expect(screen.getByText("命中规则 后端优先")).toBeInTheDocument();
    expect(screen.getByText("82 分 · 方向吻合")).toBeInTheDocument();
    expect(screen.getByText("3 条消息")).toBeInTheDocument();
  });

  it("被拦下的原因不用展开就能看到", () => {
    render(<PipelineView steps={steps} hasRun />);
    expect(screen.getByText(/拦下原因：话术里还留着 \{company\} 占位符/)).toBeInTheDocument();
  });

  it("跳过的原因与拦下的原因用不同措辞", () => {
    render(
      <PipelineView
        steps={[{ stage: "semantic_match", outcome: { kind: "skip", reason: "方案没开语义筛选" }, detail: null }]}
        hasRun
      />,
    );
    expect(screen.getByText(/跳过原因：方案没开语义筛选/)).toBeInTheDocument();
  });

  it("点开环节能看到结构化产物，未知形状也不会崩", () => {
    render(
      <PipelineView
        steps={[
          {
            stage: "route",
            outcome: { kind: "pass" },
            // 后端随时可能往 detail 里塞新字段，界面照样得铺得出来
            detail: { route: "llm", nested: { a: 1 }, tags: ["x", "y"], score: 7 },
          },
        ]}
        hasRun
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /回复路由/ }));
    const detail = screen.getByTestId("step-detail-route");
    expect(detail).toHaveTextContent("nested");
    expect(detail).toHaveTextContent("tags");
    expect(detail).toHaveTextContent("score");
  });
});
