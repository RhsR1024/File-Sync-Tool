import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const imagePreviewPageSource = readFileSync(
  join(__dirname, '../../public/clipboard-image-preview.html'),
  'utf8',
);
const textPreviewPageSource = readFileSync(
  join(__dirname, '../../public/clipboard-text-preview.html'),
  'utf8',
);
const previewBackendSource = readFileSync(
  join(__dirname, '../../src-tauri/src/clipboard/preview.rs'),
  'utf8',
);
const mainSource = readFileSync(join(__dirname, '../../src-tauri/src/main.rs'), 'utf8');
const tauriConfig = JSON.parse(
  readFileSync(join(__dirname, '../../src-tauri/tauri.conf.json'), 'utf8'),
);
const clipboardPanelCapability = JSON.parse(
  readFileSync(join(__dirname, '../../src-tauri/capabilities/clipboard-panel.json'), 'utf8'),
);
const clipboardPreviewCapability = JSON.parse(
  readFileSync(join(__dirname, '../../src-tauri/capabilities/clipboard-preview.json'), 'utf8'),
);

test('clipboard preview backend uses standalone HTML preview windows instead of hash-route pages', () => {
  assert.match(previewBackendSource, /clipboard-image-preview\.html/);
  assert.match(previewBackendSource, /clipboard-text-preview\.html/);
  assert.doesNotMatch(previewBackendSource, /#\/clipboard-preview\/image/);
  assert.doesNotMatch(previewBackendSource, /#\/clipboard-preview\/text/);
});

test('clipboard preview windows are created lazily instead of being prewarmed on startup', () => {
  assert.doesNotMatch(mainSource, /clipboard::preview::ensure_preview_windows\(app\)\?/);
});

test('clipboard panel capability remains scoped to the Alt+C panel window only', () => {
  assert.deepEqual(clipboardPanelCapability.windows, ['clipboard-panel']);
});

test('clipboard preview capability is isolated to the standalone preview windows', () => {
  assert.deepEqual(
    clipboardPreviewCapability.windows,
    ['clipboard-image-preview', 'clipboard-text-preview'],
  );
  assert.ok(
    clipboardPreviewCapability.permissions.includes('core:event:allow-listen'),
    'expected preview windows to be able to subscribe to backend update events',
  );
});

test('tauri keeps the global bridge disabled so the preview migration does not affect the clipboard panel window', () => {
  assert.notEqual(tauriConfig.app?.withGlobalTauri, true);
});

test('clipboard image preview page listens for update and clear events and resolves image assets through Tauri internals', () => {
  assert.match(imagePreviewPageSource, /image-preview-update/);
  assert.match(imagePreviewPageSource, /image-preview-clear/);
  assert.match(imagePreviewPageSource, /__TAURI_INTERNALS__/);
  assert.match(imagePreviewPageSource, /convertFileSrc/);
});

test('clipboard text preview page listens for update and clear events through Tauri internals', () => {
  assert.match(textPreviewPageSource, /text-preview-update/);
  assert.match(textPreviewPageSource, /text-preview-clear/);
  assert.match(textPreviewPageSource, /__TAURI_INTERNALS__/);
});

test('clipboard preview backend makes standalone preview windows ignore cursor events so the panel stays interactive', () => {
  assert.match(
    previewBackendSource,
    /show_preview_window[\s\S]*set_ignore_cursor_events\(true\)/,
  );
});
