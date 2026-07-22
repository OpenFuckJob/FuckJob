import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AboutDataPage from "./index";

Object.defineProperty(window, "matchMedia", { writable: true, value: vi.fn().mockImplementation(() => ({
  matches: false, addListener: vi.fn(), removeListener: vi.fn(), addEventListener: vi.fn(), removeEventListener: vi.fn(), dispatchEvent: vi.fn(),
})) });

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), confirm: vi.fn(), open: vi.fn(), save: vi.fn(), reveal: vi.fn(), openUrl: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => Promise.resolve("1.2.3") }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: mocks.confirm, open: mocks.open, save: mocks.save }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: mocks.reveal, openUrl: mocks.openUrl }));

describe("AboutDataPage", () => {
  afterEach(cleanup);
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.invoke.mockImplementation((command: string) => Promise.resolve(command === "get_data_directory"
      ? { success: true, data: "/tmp/fuckjob", error: null }
      : command === "get_llm_credential_status" ? { success: true, data: { configured: true, source: "environment" }, error: null }
      : { success: true, data: null, error: null }));
  });
  it("shows license and precise local-first privacy boundaries", async () => {
    render(<AboutDataPage />);
    expect(screen.getByText(/Apache License 2.0/)).toBeInTheDocument();
    expect(screen.getByText(/无需账号，不含遥测、自动更新或原项目服务器连接/)).toBeInTheDocument();
    expect(screen.getByText(/LLM 服务/)).toBeInTheDocument();
    expect(screen.getByText(/BOSS 直聘或猎聘/)).toBeInTheDocument();
    expect(await screen.findByText(/环境变量/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "查看完整许可" }));
    fireEvent.click(screen.getByRole("button", { name: "隐私与网络文档" }));
    fireEvent.click(screen.getByRole("button", { name: "模型配置文档" }));
    expect(mocks.openUrl).toHaveBeenNthCalledWith(1, "https://github.com/JWJW000/fuckJob_client/blob/master/LICENSE");
    expect(mocks.openUrl).toHaveBeenNthCalledWith(2, "https://github.com/JWJW000/fuckJob_client/blob/master/docs/privacy-and-network.md");
    expect(mocks.openUrl).toHaveBeenNthCalledWith(3, "https://github.com/JWJW000/fuckJob_client/blob/master/docs/model-configuration.md");
  });
  it("requires confirmation before restore and invokes restore after approval", async () => {
    mocks.open.mockResolvedValue("/tmp/backup.zip"); mocks.confirm.mockResolvedValue(true);
    mocks.invoke.mockImplementation((command: string) => Promise.resolve(command === "restore_data_backup"
      ? { success: true, data: { restart_required: true, message: "请手动重启", recovery_backup_path: "/tmp/recovery.zip" }, error: null }
      : command === "get_data_directory" ? { success: true, data: "/tmp/fuckjob", error: null }
      : { success: true, data: { configured: false, source: "none" }, error: null }));
    render(<AboutDataPage />); fireEvent.click(screen.getByRole("button", { name: "恢复备份" }));
    await waitFor(() => expect(mocks.confirm).toHaveBeenCalled());
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("restore_data_backup", { path: "/tmp/backup.zip" }));
  });
  it("does not clear logs when confirmation is declined", async () => {
    mocks.confirm.mockResolvedValue(false); render(<AboutDataPage />);
    fireEvent.click(screen.getByRole("button", { name: "清除日志" }));
    await waitFor(() => expect(mocks.confirm).toHaveBeenCalled());
    expect(mocks.invoke).not.toHaveBeenCalledWith("clear_logs", undefined);
  });
});
