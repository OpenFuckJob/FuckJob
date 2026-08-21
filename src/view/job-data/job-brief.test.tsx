import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import JobBrief from "./JobBrief";
import type { JobDetail, ParsedJobDescription } from "../../types/job-detail";

// vitest 没开 globals，testing-library 的自动清理不会注册，
// 上一个用例的 DOM 会留到下一个用例里造成重复匹配
afterEach(cleanup);

const baseJob: JobDetail = {
  id: "demo",
  platform: "boss",
  title: "AI 应用架构师",
  company_name: "示例科技",
  salary: "30-55K·16薪",
  location: " 深圳·南山区 ",
  detail: "原文留在库里，页面渲染的是后端清洗后的结果",
  is_reply: false,
  is_send_resume: false,
  created_at: "2026-08-06 16:08:27",
  resume_sent_at: null,
  updated_at: "2026-08-12 09:30:11",
};

/** 与 Rust 侧 `job_description::parse` 的返回结构一致 */
function described(overrides: Partial<ParsedJobDescription> = {}): ParsedJobDescription {
  return {
    sections: [],
    highlights: [],
    workplace: null,
    recruiter: null,
    clean_text: "",
    empty: true,
    ...overrides,
  };
}

const mockDescription = (value: ParsedJobDescription) =>
  vi.mocked(invoke).mockResolvedValue({ success: true, data: value, error: null });

describe("JobBrief", () => {
  it("把后端切好的小节铺开", async () => {
    mockDescription(
      described({
        empty: false,
        clean_text: "职位描述\n负责 Agent 系统的架构设计；\n任职要求\n三年以上后端研发经验。",
        sections: [
          { title: "职位描述", items: ["负责 Agent 系统的架构设计；"] },
          { title: "任职要求", items: ["三年以上后端研发经验。"] },
        ],
        workplace: "深圳南山区示例园区 3 栋",
        recruiter: {
          name: "张明",
          status: "在线",
          company: "示例科技",
          role: "HR.招聘专员",
        },
      }),
    );

    render(<JobBrief job={baseJob} />);

    await waitFor(() => expect(screen.getByText("职位描述")).toBeInTheDocument());
    expect(screen.getByText("任职要求")).toBeInTheDocument();
    expect(screen.getByText("负责 Agent 系统的架构设计；")).toBeInTheDocument();
    expect(screen.getByText(/示例园区 3 栋/)).toBeInTheDocument();
    expect(screen.getByText(/张明/)).toBeInTheDocument();
  });

  it("岗位头部不等后端，本地字段直接渲染", () => {
    mockDescription(described());
    render(<JobBrief job={baseJob} />);

    // 描述还在路上时，标题公司薪资就该看得见
    expect(screen.getByText("AI 应用架构师")).toBeInTheDocument();
    expect(screen.getByText("示例科技")).toBeInTheDocument();
    expect(screen.getByText("30-55K·16薪")).toBeInTheDocument();
  });

  it("没抓到岗位描述时给出明确提示，而不是空白一片", async () => {
    mockDescription(described());
    render(<JobBrief job={baseJob} />);

    await waitFor(() =>
      expect(screen.getByText("这个岗位还没抓到岗位描述")).toBeInTheDocument(),
    );
  });

  it("猎聘没有 JD 正文，只提条件标签与招聘者，原文收进折叠区", async () => {
    mockDescription(
      described({
        empty: false,
        highlights: ["1-3年", "本科", "100-499人"],
        clean_text: "AI开发工程师 【 广州-黄埔区 】 8-15k 1-3年 本科 西麦科技 计算机软件新三板上市100-499人 杨女士·人事专员 1天前在线",
        recruiter: { name: "杨女士", role: "人事专员", status: "1天前在线", company: "" },
      }),
    );

    render(<JobBrief job={{ ...baseJob, platform: "liepin" }} />);

    await waitFor(() => expect(screen.getByText("1-3年")).toBeInTheDocument());
    expect(screen.getByText("本科")).toBeInTheDocument();
    expect(screen.getByText("100-499人")).toBeInTheDocument();
    expect(screen.getByText(/杨女士/)).toBeInTheDocument();
    // 原文默认收起，同样的内容不铺两遍
    expect(screen.getByText("查看抓取原文")).toBeInTheDocument();
    expect(screen.queryByText(/新三板上市/)).not.toBeInTheDocument();
  });

  it("解析取不到时退回空态，不把整页拖成错误", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("命令不可用"));
    render(<JobBrief job={baseJob} />);

    await waitFor(() =>
      expect(screen.getByText("这个岗位还没抓到岗位描述")).toBeInTheDocument(),
    );
    expect(screen.getByText("AI 应用架构师")).toBeInTheDocument();
  });
});
