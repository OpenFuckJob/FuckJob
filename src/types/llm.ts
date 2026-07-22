import type { CommandResult } from './command'

export type CredentialSource = 'keychain' | 'environment' | 'none'
export interface LlmCredentialStatus { configured: boolean; source: CredentialSource }
export interface LlmConnectionReport { model: string; response: string; latency_ms?: number }
export type LlmCommandResult<T> = CommandResult<T>
