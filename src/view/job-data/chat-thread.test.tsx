import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import ChatThreadModal, { buildThread, isSystemMessage } from "./ChatThread";
import type { ChatMessageRecord, JobDetail } from "../../types/job-detail";

// vitest 没开 globals，testing-library 的自动清理不会注册
afterEach(cleanup);

const DAY = 86400000;
const BASE = new Date("2026-08-12T09:00:00").getTime();

function message(
  id: string,
  received: boolean,
  text: string,
  time: number,
): ChatMessageRecord {
  return {
    id,
    job_id: "demo",
    mid: Number(id),
    received,
    text,
    time,
    from_name: received ? "张明" : "我",
  };
}

const job: JobDetail = {
  id: "demo",
  platform: "boss",
  title: "AI 应用架构师",
  company_name: "示例科技",
  detail: "",
  salary: "30-55K",
  location: "深圳",
  is_reply: true,
  is_send_resume: true,
  created_at: "2026-08-06 16:08:27",
  resume_sent_at: null,
  updated_at: "2026-08-12 09:30:11",
};

describe("buildThread", () => {
  it("跨天才插日期分隔条，同一天连着聊不会被切碎", () => {
    const thread = buildThread([
      message("1", true, "你好", BASE - DAY),
      message("2", false, "你好", BASE - DAY + 60000),
      message("3", true, "明天面试", BASE),
    ]);

    expect(thread.filter((item) => item.kind === "day")).toHaveLength(2);
    expect(thread[0].kind).toBe("day");
    expect(thread[3].kind).toBe("day");
  });

  it("平台回执单独成条，不混进任何一侧的气泡", () => {
    const thread = buildThread([
      message("1", true, "对方已查看了您的附件简历", BASE),
      message("2", false, "简历文件：示例.pdf", BASE + 1000),
      message("3", true, "明天方便吗", BASE + 2000),
    ]);

    const kinds = thread.filter((item) => item.kind !== "day").map((item) => item.kind);
    expect(kinds).toEqual(["system", "system", "message"]);
  });

  it("认得出平台回执，普通对话不受影响", () => {
    expect(isSystemMessage("对方已同意，您的附件简历已发送给对方")).toBe(true);
    expect(isSystemMessage("简历文件：已交换简历")).toBe(true);
    expect(isSystemMessage("对方已查看了您的附件简历")).toBe(true);
    expect(isSystemMessage("你好，方便聊聊这个岗位吗")).toBe(false);
  });
});

describe("ChatThreadModal", () => {
  it("按收发方分列渲染：对方在左，我方在右", async () => {
    vi.mocked(invoke).mockResolvedValue({
      success: true,
      data: [
        message("1", false, "您好，想应聘这个岗位", BASE),
        message("2", true, "方便聊聊吗", BASE + 60000),
      ],
      error: null,
    });

    render(<ChatThreadModal job={job} open onClose={() => {}} />);

    await waitFor(() =>
      expect(screen.getByText("您好，想应聘这个岗位")).toBeInTheDocument(),
    );

    // Modal 渲染在 body 上的 portal 里，不在 render 返回的容器中
    const rows = document.querySelectorAll(".chat-row");
    expect(rows).toHaveLength(2);
    expect(rows[0].className).toContain("is-mine");
    expect(rows[1].className).toContain("is-peer");
  });

  it("没有沟通记录时给出空态而不是空白", async () => {
    vi.mocked(invoke).mockResolvedValue({ success: true, data: [], error: null });
    render(<ChatThreadModal job={job} open onClose={() => {}} />);
    await waitFor(() =>
      expect(screen.getByText("暂无沟通记录")).toBeInTheDocument(),
    );
  });
});
