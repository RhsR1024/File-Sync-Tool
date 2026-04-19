import assert from 'node:assert/strict';
import { test } from 'node:test';

import { parseSearch } from './clipboardSearchParser.ts';

test('parses type operator', () => {
  const r = parseSearch('type:image hello');
  assert.equal(r.filters.type, 'image');
  assert.deepEqual(r.keywords, ['hello']);
});

test('invalid type falls back to keyword', () => {
  const r = parseSearch('type:video react');
  assert.equal(r.filters.type, undefined);
  assert.deepEqual(r.keywords, ['type:video', 'react']);
});

test('parses from/to dates', () => {
  const r = parseSearch('from:2026-04-01 to:2026-04-10');
  assert.equal(r.filters.from, '2026-04-01');
  assert.equal(r.filters.to, '2026-04-10');
  assert.deepEqual(r.keywords, []);
});

test('parses app and fav', () => {
  const r = parseSearch('app:chrome fav:');
  assert.equal(r.filters.app, 'chrome');
  assert.equal(r.filters.fav, true);
});

test('parses size operators', () => {
  const r = parseSearch('size:>1024 size:<9999');
  assert.equal(r.filters.sizeGt, 1024);
  assert.equal(r.filters.sizeLt, 9999);
});

test('plain keywords pass through', () => {
  const r = parseSearch('react hook');
  assert.deepEqual(r.keywords, ['react', 'hook']);
  assert.deepEqual(r.filters, {});
});

console.log('clipboardSearchParser tests PASSED');
