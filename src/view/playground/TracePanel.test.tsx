import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TracePanel } from "./TracePanel";
import { splitRework } from "./trace";
import { makeRound, makeTrace } from "./fixtures";

afterEach(cleanup);

const noop = () => {};

describe("splitRework", () => {
  it("只把尾部追加的返工段标出来", () => {
    expect(splitRework("基础提示词", "基础提示词\n上一轮输出不合格：缺少字段")).toEqual({
      shared: "基础提示词",
      appended: "\n上一轮输出不合格：缺少字段",
    });
  });

  it("前缀对不上时整段算新内容，宁可多标也不漏标", () => {
    expect(splitRework("旧的提示词", "换了一套拼装方式")).toEqual({
      shared: "",
      appended: "换了一套拼装方式",
    });
  });
});

describe("TracePanel", () => {
  it("没有调用时给空态", () => {
    render(<TracePanel traces={[]} loading={false} onRefresh={noop} onClear={noop} onExport={noop} />);
    expect(screen.getByText("还没有模型调用")).toBeInTheDocument();
  });

  it("列出任务名、轮次与耗时", () => {
    render(
      <TracePanel
        traces={[makeTrace({ rounds: [makeRound(), makeRound({ round: 2 })] })]}
        loading={false}
        onRefresh={noop}
        onClear={noop}
        onExport={noop}
      />,
    );

    expect(screen.getByText("打招呼决策")).toBeInTheDocument();
    expect(screen.getByText("2 轮")).toBeInTheDocument();
    expect(screen.getByText("1.4s")).toBeInTheDocument();
    expect(screen.getByText("成功")).toBeInTheDocument();
  });

  it("多轮时能切换轮次，并高亮第二轮追加的返工段", () => {
    const trace = makeTrace({
      stop: "recovered",
      rounds: [
        makeRound({ prompt: "基础提示词", raw: "{\"a\":1}" }),
        makeRound({
          round: 2,
          prompt: "基础提示词\n【返工】上一轮少了 action 字段",
          raw: "{\"action\":\"send\"}",
          verdict: { kind: "passed" },
          model: "gpt-y",
        }),
      ],
    });

    render(<TracePanel traces={[trace]} loading={false} onRefresh={noop} onClear={noop} onExport={noop} />);
    fireEvent.click(screen.getByRole("button", { name: "轨迹 打招呼决策" }));

    // 第一轮没有上一轮可比，不该出现高亮
    expect(screen.getByText("模型：gpt-x")).toBeInTheDocument();
    expect(screen.queryByTestId("rework-segment")).toBeNull();

    fireEvent.click(screen.getByRole("radio", { name: "第 2 轮" }));

    expect(screen.getByText("模型：gpt-y")).toBeInTheDocument();
    expect(screen.getByTestId("rework-segment")).toHaveTextContent("【返工】上一轮少了 action 字段");
  });

  it("展示 token 用量与失败原因", () => {
    render(
      <TracePanel
        traces={[
          makeTrace({
            id: "t2",
            task_name: "回复决策",
            stop: null,
            error: "模型连续两轮都没给出合法 JSON",
            rounds: [makeRound({ verdict: { kind: "failed", reason: "解析失败" }, raw: "" })],
          }),
        ]}
        loading={false}
        onRefresh={noop}
        onClear={noop}
        onExport={noop}
      />,
    );

    expect(screen.getByText("失败")).toBeInTheDocument();
    expect(screen.getByText("模型连续两轮都没给出合法 JSON")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "轨迹 回复决策" }));
    expect(screen.getByText(/token：150/)).toBeInTheDocument();
  });

  it("清空与导出交给上层处理", () => {
    const onClear = vi.fn();
    const onExport = vi.fn();
    render(
      <TracePanel traces={[makeTrace()]} loading={false} onRefresh={noop} onClear={onClear} onExport={onExport} />,
    );

    fireEvent.click(screen.getByRole("button", { name: /清空/ }));
    fireEvent.click(screen.getByRole("button", { name: /导出轨迹/ }));

    expect(onClear).toHaveBeenCalled();
    expect(onExport).toHaveBeenCalled();
  });
});
