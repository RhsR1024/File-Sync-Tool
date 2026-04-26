import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'FileSharePage.vue'), 'utf8');

test('file share stop action executes directly without a confirmation prompt', () => {
  assert.match(pageSource, /const stopShare = async \(\) =>/);
  assert.doesNotMatch(pageSource, /window\.confirm\(t\('tools\.fileShare\.stopConfirm'\)\)/);
  assert.match(pageSource, /@click="stopShare"/);
  assert.doesNotMatch(pageSource, /@click="confirmStopShare"/);
});
