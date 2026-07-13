import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const editorSource = readFileSync(new URL('./SyncConfigurationEditor.vue', import.meta.url), 'utf8');

test('sync sections share one editor backed by the sync-domain store action', () => {
  assert.match(editorSource, /configStore\.saveSync\(\)/);
  assert.match(editorSource, /section\?: SyncConfigurationSection/);
  assert.match(editorSource, /shows\('tasks'\)/);
  assert.match(editorSource, /shows\('strategy'\)/);
  assert.match(editorSource, /shows\('delivery'\)/);
  assert.doesNotMatch(editorSource, /saveConfig\(/);
  assert.doesNotMatch(editorSource, /getConfig\(/);
});

test('combined tasks and strategy section preserves every production configuration area', () => {
  assert.match(editorSource, /'tasks-strategy'/);
  assert.match(editorSource, /props\.section === 'tasks-strategy'/);
  assert.match(editorSource, /sync-tasks-strategy-stack/);
  assert.doesNotMatch(editorSource, /sync-tasks-strategy-grid/);
  assert.doesNotMatch(editorSource, /xl:grid-cols-\[minmax\(0,1\.15fr\)_minmax\(360px,1fr\)\]/);

  for (const feature of [
    'taskLocalPathInput',
    'server_bindings',
    'local_script_binding',
    'post_copy_execution_order',
    'config.time_ranges',
    'config.file_extensions',
    'config.filename_includes',
  ]) {
    assert.match(editorSource, new RegExp(feature.replaceAll('.', '\\.')));
  }
});

test('delivery workspace retains advanced server, manual deployment, command and local script controls', () => {
  assert.match(editorSource, /sync-delivery-grid/);
  assert.match(editorSource, /serverForm\.ssh_timeout_secs/);
  assert.match(editorSource, /testAllServers/);
  assert.match(editorSource, /manualServerBindings/);
  assert.match(editorSource, /commandGroupForm/);
  assert.match(editorSource, /localGroupForm/);
});
