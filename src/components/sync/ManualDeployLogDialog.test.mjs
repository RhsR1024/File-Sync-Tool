import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const dialogSource = readFileSync(new URL('./ManualDeployLogDialog.vue', import.meta.url), 'utf8');

test('deployment log dialog filters the exact run and optional server', () => {
  assert.match(dialogSource, /log\.task_group_id === props\.session!\.task_group_id/);
  assert.match(dialogSource, /log\.run_id === props\.session!\.run_id/);
  assert.match(dialogSource, /!selectedServerId\.value \|\| log\.server_id === selectedServerId\.value/);
  assert.match(dialogSource, /run\.value\?\.deploy_attempts/);
});

test('deployment log dialog is accessible and preserves user scroll position', () => {
  assert.match(dialogSource, /role="dialog"/);
  assert.match(dialogSource, /aria-modal="true"/);
  assert.match(dialogSource, /closeButtonRef\.value\?\.focus/);
  assert.match(dialogSource, /event\.key === 'Escape'/);
  assert.match(dialogSource, /followingTail/);
  assert.match(dialogSource, /scrollHeight - element\.scrollTop - element\.clientHeight < 48/);
  assert.match(dialogSource, /onDeactivated\(close\)/);
});

test('deployment log dialog exposes per-server recovery without replacing runtime logs', () => {
  assert.match(dialogSource, /emit\('editServer', item\.id\)/);
  assert.match(dialogSource, /manualDeployLog\.description/);
  assert.match(dialogSource, /statusLabel\(item\.status\)/);
});
