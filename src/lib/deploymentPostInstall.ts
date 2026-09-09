import type { PostInstallActions } from './tauri';

export function createDefaultPostInstallActions(): PostInstallActions {
  return {
    enabled: true,
    enable_ssh: true,
    passwords: {
      enabled: true,
      framework: true,
      ums: true,
      cdm: true,
      ums_username: 'loadmin',
      framework_old_password: '123456',
      framework_new_password: 'admin_123',
      ums_old_password: 'admin_123',
      ums_new_password: 'admin_1234',
      cdm_old_password: 'admin',
      cdm_new_password: 'admin_123',
    },
    topology: {
      enabled: false,
      mode: 'ha',
      primary_ip: '',
      ha_replica_ip: '',
      virtual_ip: '',
      replica_ips: [],
      framework_password: 'admin_123',
    },
    poll_interval_secs: 30,
    poll_attempts: 20,
  };
}

export function normalizePostInstallActions(
  value: PostInstallActions | undefined,
): PostInstallActions {
  const defaults = createDefaultPostInstallActions();
  if (!value) return defaults;
  return {
    ...defaults,
    ...value,
    passwords: { ...defaults.passwords, ...value.passwords },
    topology: {
      ...defaults.topology,
      ...value.topology,
      replica_ips: [...(value.topology?.replica_ips ?? [])],
    },
  };
}
