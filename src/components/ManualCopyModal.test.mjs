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

test('manual copy modal switches the source input to a textarea for batch paste', () => {
  // The single-line <input id="manual-copy-source" ...> was replaced with a
  // <textarea> so users can paste multiple paths separated by newlines.
  assert.match(modalSource, /<textarea[^>]*id="manual-copy-source"/);
  assert.doesNotMatch(modalSource, /<input[^>]*id="manual-copy-source"/);
});

test('manual copy modal renders batch preview controls', () => {
  // Preview button + back-to-edit live inside the v-if="isBatchMode" region.
  assert.match(modalSource, /isBatchMode/);
  assert.match(modalSource, /manualCopy\.batch\.previewButton/);
  assert.match(modalSource, /manualCopy\.batch\.submitButton/);
  assert.match(modalSource, /manualCopy\.batch\.backToEdit/);
});

test('manual copy modal exposes batch row status helpers', () => {
  assert.match(modalSource, /batchStatusLabel/);
  assert.match(modalSource, /batchStatusClass/);
  assert.match(modalSource, /manualCopy\.batch\.statusOk/);
  assert.match(modalSource, /manualCopy\.batch\.statusTargetExists/);
  assert.match(modalSource, /manualCopy\.batch\.statusDuplicateInBatch/);
});
