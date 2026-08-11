import type { CommandResult } from './command'

export type CredentialSource = 'keychain' | 'environment' | 'none'
export interface LlmCredentialStatus { configured: boolean; source: CredentialSource }
/** 批量查询降级链密钥状态时的单条结果，entry_id 与配置里的服务标识一一对应 */
export interface LlmEntryCredentialStatus extends LlmCredentialStatus { entry_id: string }
export interface LlmConnectionReport { model: string; response: string; latency_ms?: number }
export type LlmCommandResult<T> = CommandResult<T>
