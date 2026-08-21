import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn(), open: vi.fn() }));

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import PlaygroundPage, { type PlaygroundPageProps } from "./index";
import { makeConfig } from "./fixtures";
import type { CommandResult } from "@/types/command";
import type { PlaygroundChain, StepReport } from "@/types/playground";

afterEach(cleanup);

const emptyReport: StepReport = { steps: [], trace_ids: [] };

function ok<T>(data: T): CommandResult<T> {
  return { success: true, data, error: null };
}

/** 除非某个用例要看轨迹，否则所有命令都回一份空报告 */
function stubInvoke(report: StepReport = emptyReport) {
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "playground_traces") return ok([]);
    return ok(report);
  });
}

function renderPage(overrides: Partial<PlaygroundPageProps> = {}) {
  const props: PlaygroundPageProps = {
    config: makeConfig(),
    llmConfigured: true,
    llmActive: true,
    onOpenLlmConfig: vi.fn(),
    onSavePrompts: vi.fn(),
    ...overrides,
  };
  return { props, ...render(<PlaygroundPage {...props} />) };
}

const CHAIN_LABEL: Record<PlaygroundChain, string> = {
  screen: "岗位筛选",
  greet: "打招呼",
  reply: "自动回复",
};

/** 从第一步走到第二步，顺带选好这一趟要测的链路 */
function goToScenario(chain: PlaygroundChain) {
  fireEvent.click(screen.getByRole("radio", { name: CHAIN_LABEL[chain] }));
  fireEvent.click(screen.getByRole("button", { name: /下一步/ }));
}

/** 取某条命令最后一次调用的参数 */
function lastArgs(command: string): Record<string, unknown> {
  const call = vi.mocked(invoke).mock.calls.filter((args) => args[0] === command).pop();
  if (!call) throw new Error(`命令 ${command} 没有被调用`);
  return (call[1] ?? {}) as Record<string, unknown>;
}

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  stubInvoke();
});

describe("PlaygroundPage 拦截", () => {
  it("大模型未配置时只显示引导，不放行界面", () => {
    renderPage({ llmActive: false, llmConfigured: false });

    expect(screen.getByText("AI 是可选功能，当前尚未配置大模型")).toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: "自动回复" })).toBeNull();
  });

  it("配置了但停用时提示去启用", () => {
    renderPage({ llmActive: false, llmConfigured: true });
    expect(screen.getByText("大模型已停用")).toBeInTheDocument();
  });
});

describe("按链路裁剪场景表单", () => {
  it("筛选链路不显示对话与会话现场，也不显示别的链路的提示词", () => {
    renderPage();
    goToScenario("screen");

    expect(screen.getByLabelText("岗位名称")).toBeInTheDocument();
    expect(screen.getByLabelText("语义筛选意图")).toBeInTheDocument();
    expect(screen.queryByLabelText("消息内容")).toBeNull();
    expect(screen.queryByLabelText("简历投递入口")).toBeNull();
    expect(screen.queryByLabelText("打招呼提示词")).toBeNull();
    expect(screen.queryByLabelText("回复提示词")).toBeNull();
  });

  it("回复链路才摆出沙盘与会话现场", () => {
    renderPage();
    goToScenario("reply");

    expect(screen.getByLabelText("消息内容")).toBeInTheDocument();
    expect(screen.getByLabelText("简历投递入口")).toBeInTheDocument();
    expect(screen.getByLabelText("窗口内已回复条数")).toBeInTheDocument();
    expect(screen.getByLabelText("回复提示词")).toBeInTheDocument();
    expect(screen.queryByLabelText("语义筛选意图")).toBeNull();
  });
});

describe("聊天沙盘的身份切换", () => {
  it("以 HR 与我方两种身份造出的消息，received 各不相同", async () => {
    renderPage();
    goToScenario("reply");

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "后端工程师" } });
    fireEvent.change(screen.getByLabelText("消息内容"), { target: { value: "在吗" } });
    fireEvent.click(screen.getByRole("button", { name: /发送/ }));

    fireEvent.click(screen.getByRole("radio", { name: "我" }));
    fireEvent.change(screen.getByLabelText("消息内容"), { target: { value: "在的，方便聊" } });
    fireEvent.click(screen.getByRole("button", { name: /发送/ }));

    fireEvent.click(screen.getByRole("button", { name: /让 AI 回复/ }));

    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("playground_reply", expect.anything()));
    expect(lastArgs("playground_reply").messages).toEqual([
      { text: "在吗", received: true },
      { text: "在的，方便聊", received: false },
    ]);
  });

  it("沙盘一条消息都没有时不发请求", async () => {
    renderPage();
    goToScenario("reply");

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "后端工程师" } });
    fireEvent.click(screen.getByRole("button", { name: /让 AI 回复/ }));

    await waitFor(() => expect(screen.getByText(/先在沙盘里造一条消息/)).toBeInTheDocument());
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("playground_reply", expect.anything());
  });

  it("体检通过的回复才会作为我方气泡追加进对话", async () => {
    stubInvoke({
      steps: [{ stage: "reply_vet", outcome: { kind: "pass" }, detail: { text: "好的，我这边随时可以" } }],
      trace_ids: [],
    });
    renderPage();
    goToScenario("reply");

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "后端工程师" } });
    fireEvent.change(screen.getByLabelText("消息内容"), { target: { value: "在吗" } });
    fireEvent.click(screen.getByRole("button", { name: /发送/ }));
    fireEvent.click(screen.getByRole("button", { name: /让 AI 回复/ }));

    // 跑完停在结果页，回上一步才看得到沙盘
    await waitFor(() => expect(screen.getByText(/生成的回复已追加进沙盘对话/)).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: /上一步/ }));

    expect(screen.getAllByTestId("bubble-me")).toHaveLength(1);
    expect(screen.getAllByTestId("bubble-me")[0]).toHaveTextContent("好的，我这边随时可以");
  });

  it("体检被拦下的文本不会污染后续上下文", async () => {
    stubInvoke({
      steps: [{ stage: "reply_vet", outcome: { kind: "block", reason: "超出单条字数上限" }, detail: { text: "一段超长的话" } }],
      trace_ids: [],
    });
    renderPage();
    goToScenario("reply");

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "后端工程师" } });
    fireEvent.change(screen.getByLabelText("消息内容"), { target: { value: "在吗" } });
    fireEvent.click(screen.getByRole("button", { name: /发送/ }));
    fireEvent.click(screen.getByRole("button", { name: /让 AI 回复/ }));

    await waitFor(() => expect(screen.getByText(/拦下原因：超出单条字数上限/)).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: /上一步/ }));
    expect(screen.queryByTestId("bubble-me")).toBeNull();
  });
});

describe("回复上下文", () => {
  it("简历状态下拉的选择原样传给命令", async () => {
    renderPage();
    goToScenario("reply");

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "后端工程师" } });
    fireEvent.mouseDown(screen.getByLabelText("简历投递入口"));
    fireEvent.click(await screen.findByText("对方正在索要"));

    fireEvent.change(screen.getByLabelText("消息内容"), { target: { value: "能发下简历吗" } });
    fireEvent.click(screen.getByRole("button", { name: /发送/ }));
    fireEvent.click(screen.getByRole("button", { name: /让 AI 回复/ }));

    await waitFor(() => expect(lastArgs("playground_reply").resumeState).toBe("RequestedByPeer"));
  });

  it("窗口内已回复条数随请求带出去", async () => {
    renderPage();
    goToScenario("reply");

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "后端工程师" } });
    fireEvent.change(screen.getByLabelText("窗口内已回复条数"), { target: { value: "5" } });
    fireEvent.change(screen.getByLabelText("消息内容"), { target: { value: "在吗" } });
    fireEvent.click(screen.getByRole("button", { name: /发送/ }));
    fireEvent.click(screen.getByRole("button", { name: /让 AI 回复/ }));

    await waitFor(() => expect(lastArgs("playground_reply").repliesInWindow).toBe(5));
  });
});

describe("提示词覆盖", () => {
  it("载入选中方案里这条链路的提示词", () => {
    renderPage();
    goToScenario("greet");
    expect(screen.getByLabelText("打招呼提示词")).toHaveValue("原打招呼提示词");
  });

  it("改了以后有未保存提示，运行时带上 overrides 且不落盘", async () => {
    const { props } = renderPage();
    goToScenario("greet");
    expect(screen.queryByText("已修改未保存")).toBeNull();

    fireEvent.change(screen.getByLabelText("打招呼提示词"), { target: { value: "改过的打招呼提示词" } });
    expect(screen.getByText("已修改未保存")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "后端工程师" } });
    fireEvent.click(screen.getByRole("button", { name: /跑打招呼/ }));

    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("playground_greet", expect.anything()));
    const args = lastArgs("playground_greet");
    expect(args.profileId).toBe("p1");
    expect(args.overrides).toEqual({
      greet_prompt: "改过的打招呼提示词",
      reply_prompt: "原回复提示词",
      semantic_filter_intent: "只要后端岗",
    });
    // 编辑本身不写配置，只有点「保存回方案」才会
    expect(props.onSavePrompts).not.toHaveBeenCalled();
  });

  it("保存回方案按 profile id 回调，另外两条原样带回去", () => {
    const { props } = renderPage();
    goToScenario("reply");

    fireEvent.change(screen.getByLabelText("回复提示词"), { target: { value: "新的回复提示词" } });
    fireEvent.click(screen.getByRole("button", { name: /保存回方案/ }));

    expect(props.onSavePrompts).toHaveBeenCalledWith("p1", {
      greet_prompt: "原打招呼提示词",
      reply_prompt: "新的回复提示词",
      semantic_filter_intent: "只要后端岗",
    });
  });

  it("还原能把草稿退回方案里的原值", () => {
    renderPage();
    goToScenario("reply");

    fireEvent.change(screen.getByLabelText("回复提示词"), { target: { value: "随手改的" } });
    fireEvent.click(screen.getByRole("button", { name: /还原/ }));

    expect(screen.getByLabelText("回复提示词")).toHaveValue("原回复提示词");
    expect(screen.queryByText("已修改未保存")).toBeNull();
  });
});

describe("链路执行", () => {
  it("没填岗位名称时不发请求", async () => {
    renderPage();
    goToScenario("screen");
    fireEvent.click(screen.getByRole("button", { name: /跑筛选/ }));

    await waitFor(() => expect(screen.getByText(/先填岗位名称/)).toBeInTheDocument());
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("playground_screen", expect.anything());
  });

  it("跑完停在结果页，环节明细与轨迹分页签放", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "playground_traces") {
        return ok([{
          id: "t1", task_name: "语义复核 Agent", started_at: "2026-08-20T10:00:00+08:00",
          duration_ms: 900, stop: "first_try", error: null,
          rounds: [{
            round: 1, prompt: "p", raw: "{}", model: "gpt-x",
            usage: { prompt_tokens: 10, completion_tokens: 2, total_tokens: 12 },
            duration_ms: 900, verdict: { kind: "passed" },
          }],
        }]);
      }
      return ok({
        steps: [{ stage: "semantic_match", outcome: { kind: "pass" }, detail: { score: 91 } }],
        trace_ids: ["t1"],
      });
    });
    renderPage();
    goToScenario("screen");

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "后端工程师" } });
    fireEvent.click(screen.getByRole("button", { name: /跑筛选/ }));

    await waitFor(() => expect(screen.getByText("91 分")).toBeInTheDocument());
    expect(screen.getByText("岗位筛选链路走完，没有被拦下")).toBeInTheDocument();
    expect(lastArgs("playground_traces").ids).toEqual(["t1"]);

    // 轨迹默认收在另一个页签里，不跟环节明细抢版面
    expect(screen.queryByText("语义复核 Agent")).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: /模型调用/ }));
    expect(screen.getByText("语义复核 Agent")).toBeInTheDocument();
  });

  it("被拦下时结论横幅直接点出断在哪一步", async () => {
    stubInvoke({
      steps: [{ stage: "regex_filter", outcome: { kind: "block", reason: "命中排除关键词 外包" }, detail: {} }],
      trace_ids: [],
    });
    renderPage();
    goToScenario("screen");

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "外包后端" } });
    fireEvent.click(screen.getByRole("button", { name: /跑筛选/ }));

    await waitFor(() => expect(screen.getByText("在 ① 正则与关键词筛选 被拦下")).toBeInTheDocument());
  });

  it("命令失败时把后端的错误原样报出来", async () => {
    vi.mocked(invoke).mockResolvedValue({
      success: false,
      data: null,
      error: { code: "provider", message: "上游返回 429" },
    });
    renderPage();
    goToScenario("greet");

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "后端工程师" } });
    fireEvent.click(screen.getByRole("button", { name: /跑打招呼/ }));

    await waitFor(() => expect(screen.getByText("上游返回 429")).toBeInTheDocument());
  });

  it("跑完能一键接着测下一条链路，岗位不用重填", async () => {
    stubInvoke({
      steps: [{ stage: "regex_filter", outcome: { kind: "pass" }, detail: {} }],
      trace_ids: [],
    });
    renderPage();
    goToScenario("screen");

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "后端工程师" } });
    fireEvent.click(screen.getByRole("button", { name: /跑筛选/ }));
    await waitFor(() => expect(screen.getByTestId("pipeline")).toBeInTheDocument());

    fireEvent.click(screen.getByRole("button", { name: /接着测打招呼/ }));

    expect(screen.getByLabelText("岗位名称")).toHaveValue("后端工程师");
    expect(screen.getByLabelText("打招呼提示词")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /跑打招呼/ })).toBeInTheDocument();
  });
});

describe("三条链路共用同一份场景", () => {
  it("在第二步直接换链路，岗位与对话原样留着", () => {
    renderPage();
    goToScenario("reply");

    fireEvent.change(screen.getByLabelText("岗位名称"), { target: { value: "后端工程师" } });
    fireEvent.change(screen.getByLabelText("消息内容"), { target: { value: "在吗" } });
    fireEvent.click(screen.getByRole("button", { name: /发送/ }));

    // 不必退回第一步：链路开关就挂在场景页顶上
    fireEvent.click(screen.getByRole("radio", { name: "打招呼" }));
    expect(screen.getByLabelText("岗位名称")).toHaveValue("后端工程师");
    expect(screen.queryByLabelText("消息内容")).toBeNull();

    fireEvent.click(screen.getByRole("radio", { name: "自动回复" }));
    expect(screen.getAllByTestId("bubble-hr")).toHaveLength(1);
  });
});
