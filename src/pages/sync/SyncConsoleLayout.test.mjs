import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const consoleSource = readFileSync(join(__dirname, 'SyncConsolePage.vue'), 'utf8');
const overviewSource = readFileSync(join(__dirname, 'SyncOverviewPage.vue'), 'utf8');
const tasksSource = readFileSync(join(__dirname, 'SyncTasksPage.vue'), 'utf8');
const tableSource = readFileSync(join(__dirname, '..', '..', 'components', 'tasks', 'TaskGroupsTable.vue'), 'utf8');
const configurationSource = readFileSync(
  join(__dirname, '..', '..', 'components', 'sync', 'SyncConfigurationEditor.vue'),
  'utf8',
);
const scanTimingSource = configurationSource.match(
  /<!-- Scan Timing -->([\s\S]*?)<!-- File Filters -->/,
)?.[1] ?? '';

test('sync console shell and overview share the same full-width workspace', () => {
  assert.doesNotMatch(consoleSource, /max-w-7xl/);
  assert.match(consoleSource, /sync-console-workspace/);
  assert.match(overviewSource, /sync-console-workspace/);
});

test('sync console exposes three tabs and keeps scheduler controls in the shared header', () => {
  assert.match(consoleSource, /key: 'overview'/);
  assert.match(consoleSource, /key: 'tasks'/);
  assert.match(consoleSource, /key: 'delivery'/);
  assert.doesNotMatch(consoleSource, /key: 'strategy'/);
  assert.match(consoleSource, /appStore\.isRunning/);
  assert.doesNotMatch(consoleSource, /appStore\.nextRunTime/);
  assert.match(consoleSource, /startScheduler\(\)/);
  assert.match(consoleSource, /stopScheduler\(\)/);
  assert.doesNotMatch(consoleSource, /SyncStrategyPage/);
});

test('sync console uses deployment configuration copy and a single prominent task-record heading', () => {
  assert.match(consoleSource, /sync\.description/);
  assert.doesNotMatch(overviewSource, /console\.stageScan/);
  assert.doesNotMatch(overviewSource, /console\.stageCopy/);
  assert.doesNotMatch(overviewSource, /console\.stageDeploy/);
  assert.match(overviewSource, /<h2 class="text-(?:base|lg)[^"]*font-bold[^>]*>\{\{ t\('console\.taskRecords'\) \}\}<\/h2>/);
});

test('sync overview renders as one integrated console surface', () => {
  assert.doesNotMatch(overviewSource, /<h2[^>]*>\{\{ t\('sync\.tabs\.overview'\) \}\}<\/h2>/);
  assert.match(overviewSource, /sync-overview-summary/);
  assert.match(overviewSource, /sync-overview-panel/);
  assert.doesNotMatch(tableSource, /bg-white border border-slate-200 rounded-xl shadow-sm overflow-hidden/);
});

test('sync overview keeps production actions and renders all four handoff metrics', () => {
  for (const marker of ['console.status', 'console.nextRun', 'console.speed', 'console.taskRecords']) {
    assert.match(overviewSource, new RegExp(marker.replace('.', '\\.')));
  }
  assert.match(overviewSource, /appStore\.progress\?\.speed/);
  assert.match(overviewSource, /<TaskGroupsTable/);
  assert.match(overviewSource, /<TaskGroupDetailPanel/);
  assert.match(overviewSource, /<ManualCopyModal/);
  assert.doesNotMatch(overviewSource, /appStore\.isRunning \? stopScheduler\(\) : startScheduler\(\)/);
});

test('sync configuration tabs use the full-width console workspace', () => {
  assert.match(configurationSource, /sync-console-workspace/);
  assert.doesNotMatch(configurationSource, /max-w-4xl/);
  assert.doesNotMatch(configurationSource, /min-h-full[^\"]*mx-auto/);
  assert.match(configurationSource, /xl:grid-cols-2/);
});

test('tasks route renders the combined tasks and strategy workspace', () => {
  assert.match(tasksSource, /section="tasks-strategy"/);
  assert.match(configurationSource, /'tasks-strategy'/);
  assert.match(configurationSource, /sync-tasks-strategy-stack grid grid-cols-1 items-start gap-4/);
  assert.doesNotMatch(configurationSource, /sync-tasks-strategy-grid/);
  assert.doesNotMatch(configurationSource, /xl:grid-cols-\[minmax\(0,1\.15fr\)_minmax\(360px,1fr\)\]/);
});

test('combined strategy panel follows the compact prototype without nested viewport columns', () => {
  assert.match(configurationSource, /sync-strategy-card/);
  assert.match(configurationSource, /t\('sync\.tabs\.strategy'\)/);
  assert.match(configurationSource, /-mt-4 row-start-3 rounded-t-none/);
  assert.match(configurationSource, /row-start-4/);
  assert.match(scanTimingSource, /sync-scan-timing-stack/);
  assert.match(scanTimingSource, /grid-cols-\[repeat\(auto-fit,minmax\(220px,1fr\)\)\]/);
  assert.doesNotMatch(scanTimingSource, /xl:grid-cols-2/);
  assert.doesNotMatch(scanTimingSource, /2xl:grid-cols-2/);
});
