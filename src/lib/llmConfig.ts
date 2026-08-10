import { invoke } from '@tauri-apps/api/core'
import type { LlmConfig } from '../types/app-config'
import type { LlmCommandResult, LlmConnectionReport, LlmCredentialStatus } from '../types/llm'

export const getLlmCredentialStatus = () => invoke<LlmCommandResult<LlmCredentialStatus>>('get_llm_credential_status')
export const setLlmApiKey = (apiKey: string) => invoke<LlmCommandResult<LlmCredentialStatus>>('set_llm_api_key', { apiKey })
export const clearLlmApiKey = () => invoke<LlmCommandResult<LlmCredentialStatus>>('clear_llm_api_key')
export const listLlmModels = (config: Pick<LlmConfig, 'provider' | 'base_url'>) => invoke<LlmCommandResult<string[]>>('list_llm_models', { provider: config.provider, baseUrl: config.base_url })
export const testLlmConnection = () => invoke<LlmCommandResult<LlmConnectionReport>>('test_llm_connection')
