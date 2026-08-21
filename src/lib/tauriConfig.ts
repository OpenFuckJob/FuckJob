import { invoke } from "@tauri-apps/api/core";
import type { AppRuntimeConfig } from "@/types/app-config";
import type { CommandResult } from "@/types/command";
import { commandErrorMessage, unwrap } from "@/types/command";

const unwrapVoid = (result: CommandResult<null>, fallback: string) => {
  if (!result.success) throw new Error(commandErrorMessage(result.error, fallback));
};

export async function loadAppConfig(): Promise<AppRuntimeConfig> {
  return unwrap(await invoke<CommandResult<AppRuntimeConfig>>("load_app_config"));
}

/**
 * 保存配置，返回**落盘后**的那份。
 *
 * 后端在保存路径上会做迁移、夹取上下界、补生成拟人化的人格种子，落盘内容与
 * 提交内容并不相同。调用方应当用返回值替换手里的配置，否则会带着旧值继续编辑
 */
export async function saveAppConfig(config: AppRuntimeConfig): Promise<AppRuntimeConfig> {
  return unwrap(await invoke<CommandResult<AppRuntimeConfig>>("save_app_config", { config }), "保存配置失败");
}

export async function importAppConfig(path: string): Promise<AppRuntimeConfig> {
  return unwrap(await invoke<CommandResult<AppRuntimeConfig>>("import_app_config", { path }));
}

export async function exportAppConfig(path: string, config: AppRuntimeConfig): Promise<void> {
  unwrapVoid(await invoke<CommandResult<null>>("export_app_config", { path, config }), "导出配置失败");
}
