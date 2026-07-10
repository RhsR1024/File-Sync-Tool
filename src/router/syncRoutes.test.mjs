import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const routerSource = readFileSync(new URL('./index.ts', import.meta.url), 'utf8');

test('legacy sync task entry points redirect directly to the unified console', () => {
  for (const path of ['/tasks', '/manual-copy']) {
    assert.match(
      routerSource,
      new RegExp(`path: '${path.replace('/', '\\/')}',[\\s\\S]{0,80}redirect: '/sync'`),
      `${path} should redirect directly to /sync`,
    );
  }
});

test('runtime logs live at the app root, outside the sync console tabs', () => {
  assert.match(routerSource, /path: '\/',[\s\S]{0,100}component: RuntimeLogsPage/);
  assert.match(routerSource, /path: '\/sync\/logs',[\s\S]{0,100}redirect: '\/'/);
});

test('sync console declares only sync-owned child tabs', () => {
  assert.match(routerSource, /path: '\/sync',[\s\S]*component: SyncConsolePage/);
  for (const path of ["''", "'tasks'", "'strategy'", "'delivery'"]) {
    assert.match(routerSource, new RegExp(`path: ${path.replace(/[']/g, "\\'")}`));
  }
  assert.doesNotMatch(routerSource, /name: 'sync-logs'/);
});
