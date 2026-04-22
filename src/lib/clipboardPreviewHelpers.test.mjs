import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  clampImagePreviewScale,
  resolveHoverPreviewTarget,
  stepImagePreviewScale,
} from './clipboardPreviewHelpers.ts';

test('resolveHoverPreviewTarget routes image items to the image preview window', () => {
  assert.deepEqual(resolveHoverPreviewTarget({ id: 7, kind: 'image' }), {
    id: 7,
    kind: 'image',
  });
});

test('resolveHoverPreviewTarget routes text-like items to the text preview window', () => {
  assert.deepEqual(resolveHoverPreviewTarget({ id: 8, kind: 'text' }), {
    id: 8,
    kind: 'text',
  });
  assert.deepEqual(resolveHoverPreviewTarget({ id: 9, kind: 'html' }), {
    id: 9,
    kind: 'text',
  });
  assert.deepEqual(resolveHoverPreviewTarget({ id: 10, kind: 'rtf' }), {
    id: 10,
    kind: 'text',
  });
});

test('resolveHoverPreviewTarget ignores non-previewable clipboard kinds', () => {
  assert.equal(resolveHoverPreviewTarget({ id: 11, kind: 'file' }), null);
  assert.equal(resolveHoverPreviewTarget(null), null);
});

test('stepImagePreviewScale applies the zoom step percentage and clamps the result', () => {
  assert.equal(stepImagePreviewScale(1, 1, 25), 1.25);
  assert.equal(stepImagePreviewScale(1.25, -1, 25), 1);
  assert.equal(stepImagePreviewScale(5.9, 1, 25), 6);
  assert.equal(stepImagePreviewScale(0.3, -1, 25), 0.25);
});

test('clampImagePreviewScale keeps arbitrary values within the supported bounds', () => {
  assert.equal(clampImagePreviewScale(0.1), 0.25);
  assert.equal(clampImagePreviewScale(1.5), 1.5);
  assert.equal(clampImagePreviewScale(9), 6);
});
