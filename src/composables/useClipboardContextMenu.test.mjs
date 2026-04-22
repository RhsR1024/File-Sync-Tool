import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  buildClipboardMenuItems,
  buildImageSaveTargetPath,
  decodeMergeSeparatorInput,
} from './clipboardContextMenuHelpers.ts';

function createItem(kind, overrides = {}) {
  return {
    id: 7,
    kind,
    content_preview: 'preview',
    content_full: 'full',
    rtf_content: null,
    html: null,
    image_path: kind === 'image' ? 'C:\\assets\\clipboard-image.png' : null,
    image_width: null,
    image_height: null,
    file_paths: kind === 'file' ? ['C:\\temp\\report.txt'] : null,
    byte_size: 128,
    char_count: 7,
    hash: 'hash-7',
    source_app: 'Explorer',
    source_app_icon: null,
    group_id: null,
    is_favorite: false,
    is_pinned: false,
    favorite_sort_index: null,
    created_at: Date.UTC(2026, 3, 21, 8, 9, 10),
    updated_at: Date.UTC(2026, 3, 21, 8, 9, 10),
    ...overrides,
  };
}

test('buildClipboardMenuItems returns text actions with plain paste and copy', () => {
  const ids = buildClipboardMenuItems({
    item: createItem('text'),
  }).map((item) => item.id);

  assert.deepEqual(ids, [
    'paste',
    'pastePlain',
    'copy',
    'toggleFavorite',
    'togglePin',
    'delete',
  ]);
});

test('buildClipboardMenuItems adds pin and move-to-group actions when groups are available', () => {
  const items = buildClipboardMenuItems({
    item: createItem('text', {
      is_pinned: true,
      group_id: 3,
    }),
    groups: [
      { id: 3, name: 'Work', sort_index: 0, created_at: 0 },
      { id: 4, name: 'Personal', sort_index: 1, created_at: 1 },
    ],
  });

  assert.deepEqual(
    items.map((item) => item.id),
    [
      'paste',
      'pastePlain',
      'copy',
      'toggleFavorite',
      'togglePin',
      'moveToGroup:none',
      'moveToGroup:3',
      'moveToGroup:4',
      'delete',
    ],
  );
  assert.equal(items.find((item) => item.id === 'togglePin')?.labelKey, 'clipboard.actions.unpin');
  assert.equal(items.find((item) => item.id === 'moveToGroup:4')?.labelParams?.name, 'Personal');
});

test('buildClipboardMenuItems disables file-system actions when every file path is missing', () => {
  const items = buildClipboardMenuItems({
    item: createItem('file'),
    fileStatuses: [
      { path: 'C:\\temp\\missing-a.txt', exists: false, size: null },
      { path: 'C:\\temp\\missing-b.txt', exists: false, size: null },
    ],
  });

  assert.equal(items.find((item) => item.id === 'pasteAsFiles')?.disabled, true);
  assert.equal(items.find((item) => item.id === 'openInExplorer')?.disabled, true);
  assert.equal(items.find((item) => item.id === 'showFileDetails')?.disabled, false);
  assert.equal(items.find((item) => item.id === 'pasteAsPath')?.disabled, false);
});

test('buildImageSaveTargetPath uses the picked directory and sanitizes the suggested file name', () => {
  const target = buildImageSaveTargetPath(
    'C:\\Users\\Admin\\Pictures',
    createItem('image', {
      image_path: 'C:\\assets\\capture:name?.png',
    }),
  );

  assert.equal(
    target,
    'C:\\Users\\Admin\\Pictures\\capture-name-.png',
  );
});

test('decodeMergeSeparatorInput resolves common escaped separators', () => {
  assert.equal(decodeMergeSeparatorInput('\\n\\n'), '\n\n');
  assert.equal(decodeMergeSeparatorInput('\\t|'), '\t|');
  assert.equal(decodeMergeSeparatorInput(''), '\n');
});
