import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  buildClipboardToolbarLayout,
  moveClipboardToolbarItem,
  normalizeClipboardToolbarItems,
} from './clipboardSettingsUi.ts';

test('normalizeClipboardToolbarItems removes duplicates and unknown ids while preserving order', () => {
  assert.deepEqual(
    normalizeClipboardToolbarItems([
      'search',
      'batch',
      'unknown',
      'batch',
      'filter',
      'settings',
      'lock',
    ]),
    ['search', 'batch', 'filter', 'settings', 'lock'],
  );
});

test('moveClipboardToolbarItem reorders only active toolbar items', () => {
  assert.deepEqual(
    moveClipboardToolbarItem(['search', 'filter', 'batch', 'settings'], 'settings', -1),
    ['search', 'filter', 'settings', 'batch'],
  );
  assert.deepEqual(
    moveClipboardToolbarItem(['search', 'filter', 'batch', 'settings'], 'search', -1),
    ['search', 'filter', 'batch', 'settings'],
  );
});

test('buildClipboardToolbarLayout derives section visibility and supported action buttons', () => {
  assert.deepEqual(
    buildClipboardToolbarLayout(
      {
        visible: true,
        items: ['filter', 'search', 'lock', 'batch', 'settings'],
      },
      ['batch', 'settings'],
    ),
    {
      showSearch: true,
      showFilter: true,
      actionItems: ['batch', 'settings'],
    },
  );

  assert.deepEqual(
    buildClipboardToolbarLayout(
      {
        visible: false,
        items: ['search', 'filter', 'batch'],
      },
      ['batch', 'settings', 'lock'],
    ),
    {
      showSearch: false,
      showFilter: false,
      actionItems: [],
    },
  );
});
