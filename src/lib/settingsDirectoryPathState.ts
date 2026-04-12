export function getDirectoryInputValue(value: string | null | undefined): string {
  return value ?? '';
}

export function toOptionalDirectoryValue(value: string): string | null {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

export function getTaskLocalPathPlaceholder(globalLocalPath: string): string {
  return globalLocalPath.trim();
}

export function getTaskLocalPathHint(message: string): string {
  return message;
}
