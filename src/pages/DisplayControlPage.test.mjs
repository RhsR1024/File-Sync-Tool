import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const pageSource = readFileSync(new URL('./DisplayControlPage.vue', import.meta.url), 'utf8');
const tauriSource = readFileSync(new URL('../lib/tauri.ts', import.meta.url), 'utf8');
const routerSource = readFileSync(new URL('../router/index.ts', import.meta.url), 'utf8');
const navigationSource = readFileSync(new URL('../lib/sidebarNavigation.ts', import.meta.url), 'utf8');
const toolsHubSource = readFileSync(new URL('./ToolsHubPage.vue', import.meta.url), 'utf8');

test('display control page exposes monitor selection and both hardware controls', () => {
  assert.match(pageSource, /monitorControlApi\.listMonitors\(\)/);
  assert.match(pageSource, /monitorControlApi\.setFeature\(\{[\s\S]*monitor_id:/);
  assert.match(pageSource, /feature === 'brightness'/);
  assert.match(pageSource, /contrastDraft/);
  assert.match(pageSource, /role="listbox"/);
  assert.match(pageSource, /function onMonitorKeydown/);
  assert.match(pageSource, /event\.key === 'ArrowDown'/);
  assert.match(pageSource, /:disabled="!selectedMonitor\.brightness_supported/);
  assert.match(pageSource, /:disabled="!selectedMonitor\.contrast_supported/);
});

test('display control API uses the stable Tauri command contract', () => {
  assert.match(tauriSource, /monitor_control_list/);
  assert.match(tauriSource, /monitor_control_set/);
  assert.match(tauriSource, /monitor_id: string/);
  assert.match(tauriSource, /MonitorControlFeature = 'brightness' \| 'contrast'/);
});

test('display control is reachable from the router, sidebar, and tools hub', () => {
  assert.match(routerSource, /path: '\/tools\/display-control'/);
  assert.match(navigationSource, /key: 'display-control'/);
  assert.match(navigationSource, /labelKey: 'sidebar\.displayControl'/);
  assert.match(toolsHubSource, /key: 'display-control'/);
  assert.match(toolsHubSource, /path: '\/tools\/display-control'/);
});

console.log('display control page tests PASSED');
