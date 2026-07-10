import assert from 'node:assert/strict';
import { test } from 'node:test';

import { createConfigStore } from './configStoreCore.ts';

function makeConfig(overrides = {}) {
  return {
    tasks: [],
    local_path: 'D:/sync',
    interval_minutes: 15,
    time_ranges: [],
    file_extensions: [],
    filename_includes: [],
    deploy_enabled: false,
    servers: [],
    command_groups: [],
    local_command_groups: [],
    stability_check_secs: 180,
    recent_file_guard_mins: 5,
    copy_buffer_size_kb: 4096,
    launch_and_auto_scan: false,
    launch_and_auto_start_file_share: false,
    close_to_tray: false,
    max_log_lines: 200,
    max_task_records: 100,
    appliance_ssh_api_timeout_secs: 5,
    framework_password_api_timeout_secs: 5,
    disk_cleanup_http_timeout_secs: 5,
    disk_cleanup_linux_mode: 'componentized',
    update_server_url: 'http://updates.example.test',
    notify_on_new_version: false,
    last_update_check_at: null,
    pending_update: null,
    clipboard: { enabled: true },
    ...overrides,
  };
}

test('config store deduplicates concurrent initial loads', async () => {
  let resolveLoad;
  let loadCount = 0;
  const store = createConfigStore({
    getConfig: () => {
      loadCount += 1;
      return new Promise((resolve) => { resolveLoad = resolve; });
    },
    updateSyncConfig: async () => {},
    updateAppConfig: async () => {},
    restartSchedulerInterval: async () => {},
    addConfigEvent: () => {},
    setMaxLogLines: () => {},
  });

  const first = store.ensureLoaded();
  const second = store.ensureLoaded();
  resolveLoad(makeConfig());
  await Promise.all([first, second]);

  assert.equal(loadCount, 1);
  assert.equal(store.isLoaded, true);
  assert.equal(store.config?.local_path, 'D:/sync');
});

test('saveSync sends only the sync patch, refreshes, then restarts the scheduler', async () => {
  const calls = [];
  const firstConfig = makeConfig({ interval_minutes: 15, pending_update: { target_version: '9.9.9' } });
  const refreshedConfig = makeConfig({ interval_minutes: 20, pending_update: { target_version: '9.9.9' } });
  let loadCount = 0;
  const store = createConfigStore({
    getConfig: async () => (++loadCount === 1 ? firstConfig : refreshedConfig),
    updateSyncConfig: async (patch) => { calls.push(['sync', patch]); },
    updateAppConfig: async () => {},
    restartSchedulerInterval: async () => { calls.push(['restart']); },
    addConfigEvent: () => { calls.push(['event']); },
    setMaxLogLines: () => {},
  });

  await store.ensureLoaded();
  await store.saveSync();

  assert.equal('pending_update' in calls[0][1], false);
  assert.equal(calls[0][1].interval_minutes, 15);
  assert.deepEqual(calls.slice(1), [['restart'], ['event']]);
  assert.equal(store.config?.interval_minutes, 20);
});

test('saveApp updates max log lines from the refreshed configuration', async () => {
  const maxLogLines = [];
  let loadCount = 0;
  const store = createConfigStore({
    getConfig: async () => (++loadCount === 1 ? makeConfig() : makeConfig({ max_log_lines: 600 })),
    updateSyncConfig: async () => {},
    updateAppConfig: async () => {},
    restartSchedulerInterval: async () => {},
    addConfigEvent: () => {},
    setMaxLogLines: (value) => { maxLogLines.push(value); },
  });

  await store.ensureLoaded();
  await store.saveApp();

  assert.deepEqual(maxLogLines, [600]);
});
