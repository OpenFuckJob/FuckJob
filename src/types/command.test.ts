import { describe, expect, it } from "vitest";

import { commandErrorMessage, unwrap, unwrapOptional } from "./command";

describe("commandErrorMessage", () => {
  it("returns the structured error message", () => {
    expect(
      commandErrorMessage({
        code: "configuration",
        message: "请配置模型",
      }),
    ).toBe("请配置模型");
  });

  it("returns the fallback for a missing error", () => {
    expect(commandErrorMessage(null, "加载失败")).toBe("加载失败");
  });

  it("returns the fallback for an empty error message", () => {
    expect(
      commandErrorMessage(
        { code: "internal", message: "" },
        "操作失败",
      ),
    ).toBe("操作失败");
  });
});

describe("unwrap", () => {
  it("throws the structured error message for a failed command", () => {
    expect(() =>
      unwrap({
        data: null,
        success: false,
        error: {
          code: "network",
          message: "连接失败",
        },
      }),
    ).toThrow(new Error("连接失败"));
  });

  it("returns data from a successful command", () => {
    expect(
      unwrap({ data: "完成", success: true, error: null }),
    ).toBe("完成");
  });
});

describe("unwrapOptional", () => {
  it("returns null from a successful command without data", () => {
    expect(
      unwrapOptional<string>({ data: null, success: true, error: null }),
    ).toBeNull();
  });

  it("throws the structured error message for a failed command", () => {
    expect(() =>
      unwrapOptional({
        data: null,
        success: false,
        error: {
          code: "credential",
          message: "请重新登录",
        },
      }),
    ).toThrow(new Error("请重新登录"));
  });
});
