import { describe, expect, it } from "vitest";
import { parseMockInterviewReport } from "./mock-interview";

describe("parseMockInterviewReport", () => {
  it("parses a structured interview report", () => {
    const report = parseMockInterviewReport(JSON.stringify({
      overallScore: 80,
      overallSummary: "匹配度良好",
      dimensions: [{ dimension: "技术深度", score: 82, strengths: [], weaknesses: [], evidence: [] }],
      risks: [],
      optimizations: [],
    }));

    expect(report.overallScore).toBe(80);
    expect(report.dimensions[0].dimension).toBe("技术深度");
  });

  it("rejects an invalid report shape", () => {
    expect(() => parseMockInterviewReport('{"overallScore":80}')).toThrow("报告格式无效");
  });
});
