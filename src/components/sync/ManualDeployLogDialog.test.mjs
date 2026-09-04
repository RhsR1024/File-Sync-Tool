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

test('deployment log dialog copies timestamp and message as one line', () => {
  assert.match(dialogSource, /return `\$\{formatTime\(log\.timestamp\)\}\$\{serverLabel\} \$\{log\.message\}`/);
  assert.match(dialogSource, /filteredLogs\.value\.map\(formatLogLine\)\.join\('\\n'\)/);
  assert.match(dialogSource, /navigator\.clipboard\.writeText/);
  assert.match(dialogSource, /settings\.manualDeployLog\.copyLogs/);
  assert.doesNotMatch(dialogSource, /class="flex gap-2"/);
});

test('deployment log dialog highlights severity keywords case-insensitively without raw HTML', () => {
  assert.match(dialogSource, /LOG_KEYWORD_SPLIT_PATTERN = \/\(failed\|fail\|error\|warning\|warn\|info\)\/gi/);
  assert.match(dialogSource, /normalizedKeyword === 'error' \|\| normalizedKeyword === 'fail' \|\| normalizedKeyword === 'failed'/);
  assert.match(dialogSource, /normalizedKeyword === 'info'/);
  assert.match(dialogSource, /font-semibold text-rose-300/);
  assert.match(dialogSource, /font-semibold text-emerald-300/);
  assert.match(dialogSource, /font-semibold text-amber-300/);
  assert.match(dialogSource, /segmentLogMessage\(log\.message\)/);
  assert.match(dialogSource, /\{\{ segment\.text \}\}/);
  assert.doesNotMatch(dialogSource, /v-html/);
});
