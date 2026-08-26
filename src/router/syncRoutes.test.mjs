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

test('the retired copy history page is no longer routable', () => {
  assert.doesNotMatch(routerSource, /HistoryPage/);
  assert.doesNotMatch(routerSource, /path: '\/history'/);
});

test('the legacy strategy URL redirects to the combined tasks and strategy page', () => {
  assert.match(
    routerSource,
    /path: 'strategy',[\s\S]{0,100}name: 'sync-strategy',[\s\S]{0,100}redirect: '\/sync\/tasks'/,
  );
  assert.doesNotMatch(routerSource, /path: 'strategy',[\s\S]{0,100}component: SyncStrategyPage/);
});

test('sidebar routes can preload their existing lazy component loaders without duplicate work', () => {
  for (const path of ['/', '/sync', '/settings', '/tools', '/tools/screen-share']) {
    assert.match(routerSource, new RegExp(`'${path.replaceAll('/', '\\/')}': \\[`));
  }
  assert.match(routerSource, /export async function preloadRoute\(path: string\): Promise<void>/);
  assert.match(routerSource, /new WeakMap<RouteComponentLoader, Promise<unknown>>\(\)/);
  assert.match(routerSource, /routeComponentPreloadCache\.delete\(loader\)/);
});
