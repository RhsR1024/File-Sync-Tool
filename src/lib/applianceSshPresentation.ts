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
