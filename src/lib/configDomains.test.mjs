import assert from 'node:assert/strict';
import { test } from 'node:test';

import { buildAppPatch, buildSyncPatch } from './configDomains.ts';

const config = {
  tasks: [{ id: 'task-1' }],
  local_path: 'D:/sync',
  interval_minutes: 15,
  time_ranges: ['09:00-18:00'],
  file_extensions: ['tar.gz'],
  filename_includes: ['VMS'],
  deploy_enabled: true,
  servers: [{ id: 'server-1' }],
  command_groups: [{ id: 'remote-group' }],
  local_command_groups: [{ id: 'local-group' }],
  stability_check_secs: 180,
  recent_file_guard_mins: 5,
  copy_buffer_size_kb: 8192,
  copy_mode: 'windows_shell',
  launch_and_auto_scan: true,
  launch_and_auto_start_file_share: false,
  close_to_tray: true,
  sync_task_notifications_enabled: true,
  max_log_lines: 500,
  max_task_records: 250,
  appliance_ssh_api_timeout_secs: 10,
  framework_password_api_timeout_secs: 11,
  disk_cleanup_http_timeout_secs: 12,
  disk_cleanup_linux_mode: 'mainline',
  update_server_url: 'http://updates.example.test',
  notify_on_new_version: true,
  last_update_check_at: '2026-07-10T12:00:00+08:00',
  pending_update: { target_version: '9.9.9' },
  clipboard: { enabled: false },
  device_simulator: {
    asset_server_url_override: null,
    selected_interface_id: null,
    last_platform: 'ums',
    last_start_ip: '192.168.1.100',
    last_device_groups: [],
    last_http_port: 81,
    last_rtsp_ports: { main: 554, sub: 555, third: 556 },
    auto_check_asset_updates: true,
    manage_firewall: true,
  },
};

const backendOnlyFields = ['last_update_check_at', 'pending_update'];

test('config domain patches are disjoint and cover every writable AppConfig field', () => {
  const syncPatch = buildSyncPatch(config);
  const appPatch = buildAppPatch(config);
  const syncFields = Object.keys(syncPatch).sort();
  const appFields = Object.keys(appPatch).sort();

  assert.deepEqual(syncFields.filter((field) => appFields.includes(field)), []);
  assert.deepEqual(
    [...syncFields, ...appFields, ...backendOnlyFields].sort(),
    Object.keys(config).sort(),
  );
  assert.equal('last_update_check_at' in syncPatch, false);
  assert.equal('pending_update' in appPatch, false);
});

test('config domain patches preserve the exact values owned by each domain', () => {
  const syncPatch = buildSyncPatch(config);
  const appPatch = buildAppPatch(config);

  assert.equal(syncPatch.tasks, config.tasks);
  assert.equal(syncPatch.copy_buffer_size_kb, 8192);
  assert.equal(syncPatch.copy_mode, 'windows_shell');
  assert.equal(appPatch.clipboard, config.clipboard);
  assert.equal(appPatch.device_simulator, config.device_simulator);
  assert.equal(appPatch.update_server_url, 'http://updates.example.test');
  assert.equal(appPatch.sync_task_notifications_enabled, true);
});
