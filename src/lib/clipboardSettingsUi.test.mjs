import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(__dirname, 'clipboardSettingsUi.ts'), 'utf8');

test('clipboard settings tabs omit the deprecated about tab', () => {
  assert.match(source, /id: 'general'/);
  assert.match(source, /id: 'display'/);
  assert.match(source, /id: 'shortcuts'/);
  assert.match(source, /id: 'data'/);
  assert.match(source, /id: 'preview'/);
  assert.match(source, /id: 'appFilter'/);
  assert.doesNotMatch(source, /id: 'about'/);
});

test('clipboard toolbar actions stay fixed and non-configurable', () => {
  assert.match(source, /CLIPBOARD_TOOLBAR_ACTION_IDS = \[\s*'batch',\s*'settings',\s*'lock',\s*\]/s);
  assert.doesNotMatch(source, /normalizeClipboardToolbarItems|buildClipboardToolbarLayout|moveClipboardToolbarItem/);
});
