export type ApplianceSshEnableState = 'enabled' | 'disabled' | 'unknown';

export function getApplianceSshEnableState(value?: number | null): ApplianceSshEnableState {
  if (value === 1) {
    return 'enabled';
  }

  if (value === 0) {
    return 'disabled';
  }

  return 'unknown';
}

export function isValidSshPort(value: unknown): boolean {
  return (
    typeof value === 'number' &&
    Number.isInteger(value) &&
    value >= 1 &&
    value <= 65535
  );
}
