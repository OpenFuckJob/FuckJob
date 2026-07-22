import { invoke } from "@tauri-apps/api/core";
import type { AppRuntimeConfig } from "@/types/app-config";
import type { CommandResult } from "@/types/command";
import { commandErrorMessage, unwrap } from "@/types/command";

export interface RestoreResult { restart_required: boolean; message: string; recovery_backup_path: string }
export interface ClearLogsResult { path: string; cleared: boolean }
export interface CredentialStatus { configured: boolean; source: "keychain" | "environment" | "none" }

const voidCommand = async (name: string, args?: Record<string, unknown>) => {
  const result = await invoke<CommandResult<null>>(name, args);
  if (!result.success) throw new Error(commandErrorMessage(result.error));
};

export const getDataDirectory = () => invoke<CommandResult<string>>("get_data_directory").then(unwrap);
export const exportDataBackup = (path: string) => voidCommand("export_data_backup", { path });
export const restoreDataBackup = (path: string) => invoke<CommandResult<RestoreResult>>("restore_data_backup", { path }).then(unwrap);
export const clearLogs = () => invoke<CommandResult<ClearLogsResult>>("clear_logs").then(unwrap);
export const resetAppConfig = () => invoke<CommandResult<AppRuntimeConfig>>("reset_app_config").then(unwrap);
export const getCredentialStatus = () => invoke<CommandResult<CredentialStatus>>("get_llm_credential_status").then(unwrap);
export const clearModelKey = () => invoke<CommandResult<CredentialStatus>>("clear_llm_api_key").then(unwrap);
