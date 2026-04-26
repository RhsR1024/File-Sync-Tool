import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const modalSource = readFileSync(join(__dirname, 'ManualCopyModal.vue'), 'utf8');

test('manual copy modal closes only from the explicit header X action', () => {
  const closeBindings = modalSource.match(/@click="closeModal"/g) ?? [];

  assert.equal(closeBindings.length, 1, 'expected only the header close button to dismiss the modal');
  assert.doesNotMatch(modalSource, /@click\.self="onBackdropClick"/);
  assert.doesNotMatch(modalSource, /event\.key === 'Escape'/);
});

test('manual copy modal no longer asks for dirty-state confirmation before closing', () => {
  assert.doesNotMatch(modalSource, /window\.confirm\(t\('tasks\.modal\.dirtyConfirm'\)\)/);
  assert.doesNotMatch(modalSource, /isFormDirty/);
});
