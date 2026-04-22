import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  pruneClipboardSelection,
  resolveQuickPasteTargetId,
  toggleClipboardSelection,
} from './clipboardInteractionHelpers.ts';

test('toggleClipboardSelection toggles a single id and updates the anchor', () => {
  const result = toggleClipboardSelection({
    visibleIds: [10, 11, 12, 13],
    selectedIds: new Set([11]),
    anchorId: 11,
    targetId: 12,
  });

  assert.deepEqual(Array.from(result.selectedIds), [11, 12]);
  assert.equal(result.anchorId, 12);
});

test('toggleClipboardSelection adds the whole range when shift is held', () => {
  const result = toggleClipboardSelection({
    visibleIds: [10, 11, 12, 13, 14],
    selectedIds: new Set([10]),
    anchorId: 10,
    targetId: 13,
    shiftKey: true,
  });

  assert.deepEqual(Array.from(result.selectedIds), [10, 11, 12, 13]);
  assert.equal(result.anchorId, 13);
});

test('pruneClipboardSelection removes hidden ids and clears an invisible anchor', () => {
  const result = pruneClipboardSelection([12, 13], {
    selectedIds: new Set([10, 12, 13]),
    anchorId: 10,
  });

  assert.deepEqual(Array.from(result.selectedIds), [12, 13]);
  assert.equal(result.anchorId, null);
});

test('resolveQuickPasteTargetId maps Alt+number to visible row order', () => {
  const items = [{ id: 21 }, { id: 22 }, { id: 23 }];

  assert.equal(resolveQuickPasteTargetId(items, '2', true), 22);
  assert.equal(resolveQuickPasteTargetId(items, '4', true), null);
  assert.equal(resolveQuickPasteTargetId(items, '2', false), null);
  assert.equal(resolveQuickPasteTargetId(items, '0', true), null);
});
