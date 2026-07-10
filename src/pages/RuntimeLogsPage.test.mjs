import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'RuntimeLogsPage.vue'), 'utf8');

test('runtime logs empty state omits the extra scheduler hint copy', () => {
  assert.match(pageSource, /console\.empty\.title/);
  assert.doesNotMatch(pageSource, /console\.empty\.description/);
});
