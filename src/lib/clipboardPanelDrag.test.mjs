import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  CLIPBOARD_PANEL_USE_NATIVE_DRAG_REGION,
  shouldStartClipboardPanelDrag,
} from './clipboardPanelDrag.ts';

test('clipboard panel drag policy disables the native drag-region attribute to avoid double drag handling', () => {
  assert.equal(CLIPBOARD_PANEL_USE_NATIVE_DRAG_REGION, false);
});

test('shouldStartClipboardPanelDrag only allows left-button drags from non-interactive targets', () => {
  assert.equal(
    shouldStartClipboardPanelDrag({
      button: 0,
      target: {
        closest: () => null,
      },
    }),
    true,
  );

  assert.equal(
    shouldStartClipboardPanelDrag({
      button: 1,
      target: {
        closest: () => null,
      },
    }),
    false,
  );

  assert.equal(
    shouldStartClipboardPanelDrag({
      button: 0,
      target: {
        closest: (selector) => selector === 'button, input, a, [data-no-drag]' ? {} : null,
      },
    }),
    false,
  );
});
