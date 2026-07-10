import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const routerSource = readFileSync(new URL('./index.ts', import.meta.url), 'utf8');

test('legacy sync entry points redirect directly to the unified console', () => {
  for (const path of ['/', '/tasks', '/manual-copy']) {
    assert.match(
      routerSource,
      new RegExp(`path: '${path.replace('/', '\\/')}',[\\s\\S]{0,80}redirect: '/sync'`),
      `${path} should redirect directly to /sync`,
    );
  }
});

test('sync console declares every planned child tab', () => {
  assert.match(routerSource, /path: '\/sync',[\s\S]*component: SyncConsolePage/);
  for (const path of ["''", "'tasks'", "'strategy'", "'delivery'", "'logs'"]) {
    assert.match(routerSource, new RegExp(`path: ${path.replace(/[']/g, "\\'")}`));
  }
});
