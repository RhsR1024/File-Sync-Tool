import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  buildClipboardPanelGroupRows,
  resolveClipboardPanelGroupLabel,
} from './clipboardPanelGroupsMenu.ts';

const labels = {
  defaultGroup: 'Default',
  createGroup: 'Add group',
};

function createGroup(id, name) {
  return {
    id,
    name,
    sort_index: id,
    created_at: 0,
  };
}

test('resolveClipboardPanelGroupLabel falls back to the default label when no custom group is active', () => {
  assert.equal(
    resolveClipboardPanelGroupLabel(
      [createGroup(2, 'Work')],
      null,
      labels.defaultGroup,
    ),
    'Default',
  );

  assert.equal(
    resolveClipboardPanelGroupLabel(
      [createGroup(2, 'Work')],
      99,
      labels.defaultGroup,
    ),
    'Default',
  );
});

test('buildClipboardPanelGroupRows keeps the default row first, preserves custom groups, and ends with create', () => {
  assert.deepEqual(
    buildClipboardPanelGroupRows(
      [createGroup(3, 'Work'), createGroup(8, 'Personal')],
      8,
      labels,
    ),
    [
      {
        kind: 'group',
        id: null,
        name: 'Default',
        selected: false,
        isDefault: true,
        showSeparatorAbove: false,
      },
      {
        kind: 'group',
        id: 3,
        name: 'Work',
        selected: false,
        isDefault: false,
        showSeparatorAbove: true,
      },
      {
        kind: 'group',
        id: 8,
        name: 'Personal',
        selected: true,
        isDefault: false,
        showSeparatorAbove: false,
      },
      {
        kind: 'create',
        label: 'Add group',
        showSeparatorAbove: true,
      },
    ],
  );
});
