import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'RemotePackagePatchPage.vue'), 'utf8');

test('remote package patch uses the full tool workspace', () => {
  assert.doesNotMatch(pageSource, /max-w-7xl/);
  assert.doesNotMatch(pageSource, /mx-auto flex w-full/);
  assert.match(pageSource, /flex w-full flex-col gap-6 px-6 py-6/);
});

test('complex remote package steps span the desktop grid', () => {
  assert.match(pageSource, /xl:col-span-2[^\"]*">[\s\S]*?stepBadgeClass\(3\)/);
  assert.match(pageSource, /xl:col-span-2[^\"]*">[\s\S]*?stepBadgeClass\(4\)/);
  assert.match(pageSource, /2xl:grid-cols-\[minmax\(320px,0\.8fr\)_minmax\(0,1\.2fr\)\]/);
});

test('remote package panels use the shared card hierarchy', () => {
  assert.match(pageSource, /rounded-xl border border-slate-200 bg-white shadow-sm/);
  assert.match(pageSource, /border-b border-slate-200 bg-slate-50 px-5 py-4/);
});
