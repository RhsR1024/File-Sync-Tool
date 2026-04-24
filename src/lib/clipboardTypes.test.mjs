import assert from 'node:assert/strict';
import { test } from 'node:test';

import { normalizeClipboardSettings } from './clipboardTypes.ts';

test('normalizeClipboardSettings prefers nested preview text settings over legacy flat fields', () => {
  const normalized = normalizeClipboardSettings({
    enable_text_preview: false,
    preview: {
      text_enabled: true,
    },
  });

  assert.equal(normalized.preview.text_enabled, true);
  assert.equal(normalized.enable_text_preview, true);
});

test('normalizeClipboardSettings prefers nested preview delay over legacy preview_delay_ms', () => {
  const normalized = normalizeClipboardSettings({
    preview_delay_ms: 500,
    preview: {
      delay_ms: 150,
    },
  });

  assert.equal(normalized.preview.delay_ms, 150);
  assert.equal(normalized.preview_delay_ms, 150);
});
