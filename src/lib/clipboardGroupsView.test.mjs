import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  partitionClipboardItemsForDisplay,
  resolveActiveClipboardGroupId,
} from './clipboardGroupsView.ts';

function createItem(id, overrides = {}) {
  return {
    id,
    kind: 'text',
    content_preview: `preview-${id}`,
    content_full: `full-${id}`,
    rtf_content: null,
    html: null,
    image_path: null,
    image_width: null,
    image_height: null,
    file_paths: null,
    byte_size: 32,
    char_count: 8,
    hash: `hash-${id}`,
    source_app: 'Explorer',
    source_app_icon: null,
    group_id: null,
    is_favorite: false,
    is_pinned: false,
    favorite_sort_index: null,
    created_at: 0,
    updated_at: 0,
    ...overrides,
  };
}

test('partitionClipboardItemsForDisplay keeps pinned items in a dedicated section without duplicates', () => {
  const { pinnedItems, regularItems } = partitionClipboardItemsForDisplay([
    createItem(1, { is_pinned: true }),
    createItem(2),
    createItem(3, { is_pinned: true }),
    createItem(4),
  ]);

  assert.deepEqual(pinnedItems.map((item) => item.id), [1, 3]);
  assert.deepEqual(regularItems.map((item) => item.id), [2, 4]);
});

test('resolveActiveClipboardGroupId falls back to all groups when the selected group no longer exists', () => {
  assert.equal(
    resolveActiveClipboardGroupId(
      [{ id: 3, name: 'Work', sort_index: 0, created_at: 0 }],
      3,
    ),
    3,
  );
  assert.equal(
    resolveActiveClipboardGroupId(
      [{ id: 3, name: 'Work', sort_index: 0, created_at: 0 }],
      9,
    ),
    null,
  );
});
