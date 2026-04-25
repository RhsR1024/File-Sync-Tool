import assert from 'node:assert/strict';
import { test } from 'node:test';

import { parseKeyword, parseRange, parseSingle } from './validation.ts';

test('parseSingle accepts plain decimal', () => {
  assert.deepEqual(parseSingle('110'), { ok: true, code: 110 });
  assert.deepEqual(parseSingle('  300005  '), { ok: true, code: 300005 });
});

test('parseSingle rejects non-numeric', () => {
  assert.deepEqual(parseSingle('abc'), { ok: false, error: 'invalid_single' });
  assert.deepEqual(parseSingle(''), { ok: false, error: 'invalid_single' });
  assert.deepEqual(parseSingle('-5'), { ok: false, error: 'invalid_single' });
  assert.deepEqual(parseSingle('1.5'), { ok: false, error: 'invalid_single' });
});

test('parseRange accepts START-END within span', () => {
  assert.deepEqual(parseRange('300000-301000'), { ok: true, start: 300000, end: 301000 });
  assert.deepEqual(parseRange(' 100 - 200 '), { ok: true, start: 100, end: 200 });
});

test('parseRange rejects bad format', () => {
  assert.deepEqual(parseRange('300000'), { ok: false, error: 'invalid_range_format' });
  assert.deepEqual(parseRange('a-b'), { ok: false, error: 'invalid_range_format' });
  assert.deepEqual(parseRange('300000-'), { ok: false, error: 'invalid_range_format' });
});

test('parseRange rejects reversed endpoints', () => {
  assert.deepEqual(parseRange('500-100'), { ok: false, error: 'range_reversed' });
});

test('parseRange rejects span > 1000', () => {
  assert.deepEqual(parseRange('0-1001'), { ok: false, error: 'range_too_large' });
  assert.deepEqual(parseRange('0-1000'), { ok: true, start: 0, end: 1000 });
});

test('parseKeyword trims and accepts 1..50 chars', () => {
  assert.deepEqual(parseKeyword('  hello  '), { ok: true, keyword: 'hello' });
  assert.deepEqual(parseKeyword(''), { ok: false, error: 'invalid_keyword' });
  assert.deepEqual(parseKeyword('   '), { ok: false, error: 'invalid_keyword' });
  assert.deepEqual(parseKeyword('x'.repeat(50)), { ok: true, keyword: 'x'.repeat(50) });
  assert.deepEqual(parseKeyword('x'.repeat(51)), { ok: false, error: 'invalid_keyword' });
});
