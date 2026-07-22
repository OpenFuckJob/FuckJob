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

export async function saveAppConfig(config: AppRuntimeConfig): Promise<void> {
  unwrapVoid(await invoke<CommandResult<null>>("save_app_config", { config }), "保存配置失败");
}

export async function importAppConfig(path: string): Promise<AppRuntimeConfig> {
  return unwrap(await invoke<CommandResult<AppRuntimeConfig>>("import_app_config", { path }));
}

export async function exportAppConfig(path: string, config: AppRuntimeConfig): Promise<void> {
  unwrapVoid(await invoke<CommandResult<null>>("export_app_config", { path, config }), "导出配置失败");
}
