import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const consoleSource = readFileSync(join(__dirname, 'SyncConsolePage.vue'), 'utf8');
const overviewSource = readFileSync(join(__dirname, 'SyncOverviewPage.vue'), 'utf8');
const tableSource = readFileSync(join(__dirname, '..', '..', 'components', 'tasks', 'TaskGroupsTable.vue'), 'utf8');

test('sync console shell and overview share the same full-width workspace', () => {
  assert.doesNotMatch(consoleSource, /max-w-7xl/);
  assert.match(consoleSource, /sync-console-workspace/);
  assert.match(overviewSource, /sync-console-workspace/);
});

test('sync overview renders as one integrated console surface', () => {
  assert.doesNotMatch(overviewSource, /<h2[^>]*>\{\{ t\('sync\.tabs\.overview'\) \}\}<\/h2>/);
  assert.match(overviewSource, /sync-overview-summary/);
  assert.match(overviewSource, /sync-overview-panel/);
  assert.doesNotMatch(tableSource, /bg-white border border-slate-200 rounded-xl shadow-sm overflow-hidden/);
});
