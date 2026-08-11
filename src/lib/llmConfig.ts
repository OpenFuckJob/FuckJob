import { invoke } from '@tauri-apps/api/core'
import type { LlmConfig } from '../types/app-config'
import type { LlmCommandResult, LlmConnectionReport, LlmCredentialStatus, LlmEntryCredentialStatus } from '../types/llm'

export const getLlmCredentialStatus = () => invoke<LlmCommandResult<LlmCredentialStatus>>('get_llm_credential_status')
export const setLlmApiKey = (apiKey: string) => invoke<LlmCommandResult<LlmCredentialStatus>>('set_llm_api_key', { apiKey })
export const clearLlmApiKey = () => invoke<LlmCommandResult<LlmCredentialStatus>>('clear_llm_api_key')
export const listLlmModels = (config: Pick<LlmConfig, 'provider' | 'base_url'>) => invoke<LlmCommandResult<string[]>>('list_llm_models', { provider: config.provider, baseUrl: config.base_url })
export const testLlmConnection = () => invoke<LlmCommandResult<LlmConnectionReport>>('test_llm_connection')

// 以下命令按降级链条目标识读写：密钥独立存储，互不覆盖。
// 注意 test_llm_entry_connection / list_llm_models_for 读的是已落盘的配置，
// 界面上必须先保存再调用，否则会拿到与当前编辑内容不一致的结果。
export const getLlmCredentialStatusFor = (entryId: string) => invoke<LlmCommandResult<LlmCredentialStatus>>('get_llm_credential_status_for', { entryId })
export const setLlmApiKeyFor = (entryId: string, apiKey: string) => invoke<LlmCommandResult<LlmCredentialStatus>>('set_llm_api_key_for', { entryId, apiKey })
export const clearLlmApiKeyFor = (entryId: string) => invoke<LlmCommandResult<LlmCredentialStatus>>('clear_llm_api_key_for', { entryId })
export const listLlmCredentialStatus = (entryIds: string[]) => invoke<LlmCommandResult<LlmEntryCredentialStatus[]>>('list_llm_credential_status', { entryIds })
export const testLlmEntryConnection = (entryId: string) => invoke<LlmCommandResult<LlmConnectionReport>>('test_llm_entry_connection', { entryId })
export const listLlmModelsFor = (entryId: string) => invoke<LlmCommandResult<string[]>>('list_llm_models_for', { entryId })
