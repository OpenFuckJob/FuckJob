export type CommandErrorCode =
  | "configuration"
  | "credential"
  | "network"
  | "provider"
  | "storage"
  | "browser"
  | "validation"
  | "cancelled"
  | "internal";

export interface CommandError {
  code: CommandErrorCode;
  message: string;
  detail?: null;
}

export interface CommandResult<T> {
  data: T | null;
  success: boolean;
  error: CommandError | null;
}

export function commandErrorMessage(
  error: CommandError | null | undefined,
  fallback = "操作失败",
): string {
  return error?.message || fallback;
}

export function unwrap<T>(result: CommandResult<T>): T {
  if (!result.success || result.data === null) {
    throw new Error(commandErrorMessage(result.error));
  }
  return result.data;
}

export function unwrapOptional<T>(result: CommandResult<T | null>): T | null {
  if (!result.success) {
    throw new Error(commandErrorMessage(result.error));
  }
  return result.data;
}
