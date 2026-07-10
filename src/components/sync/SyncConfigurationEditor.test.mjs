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
