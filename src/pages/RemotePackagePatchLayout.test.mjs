import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'RemotePackagePatchPage.vue'), 'utf8');

test('remote package patch uses a wide centered tool workspace', () => {
  assert.match(pageSource, /mx-auto flex w-full max-w-\[1440px\] flex-col gap-5 px-6 py-6/);
});

test('complex remote package steps use a responsive eight-four desktop grid', () => {
  assert.match(pageSource, /grid grid-cols-12 gap-5/);
  assert.match(pageSource, /col-span-12 space-y-5 xl:col-span-8/);
  assert.match(pageSource, /col-span-12 xl:col-span-4/);
  assert.match(pageSource, /space-y-5 xl:sticky xl:top-4/);
});

test('remote package panels use the shared card hierarchy', () => {
  assert.match(pageSource, /overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm/);
  assert.match(pageSource, /border-b border-slate-100 px-5 py-4/);
});
