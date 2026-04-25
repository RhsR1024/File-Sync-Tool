import assert from 'node:assert/strict';
import { test } from 'node:test';

import { compareVersionsAsc, formatReleaseDate, isCurrentVersion } from './version.ts';

test('compareVersionsAsc sorts semver values in ascending order', () => {
  const values = [{ version: '1.0.8' }, { version: '1.0.6' }, { version: '1.0.7' }];
  values.sort(compareVersionsAsc);
  assert.deepEqual(values.map((item) => item.version), ['1.0.6', '1.0.7', '1.0.8']);
});

test('compareVersionsAsc keeps invalid versions before valid versions', () => {
  const values = [{ version: 'bad' }, { version: '1.0.7' }];
  values.sort(compareVersionsAsc);
  assert.deepEqual(values.map((item) => item.version), ['bad', '1.0.7']);
});

test('formatReleaseDate normalizes ISO-like dates for display', () => {
  assert.equal(formatReleaseDate('2026-04-25'), '2026.04.25');
  assert.equal(formatReleaseDate(''), '');
});

test('isCurrentVersion matches exact version strings after trimming', () => {
  assert.equal(isCurrentVersion('1.0.7', '1.0.7'), true);
  assert.equal(isCurrentVersion(' 1.0.7 ', '1.0.7'), true);
  assert.equal(isCurrentVersion('1.0.8', '1.0.7'), false);
});
