import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(__dirname, 'UpdateRedDot.vue'), 'utf8');

test('update red dot uses a small dot plus NEW text instead of an arrow icon', () => {
  assert.doesNotMatch(source, /ArrowUp/);
  assert.match(source, />\s*NEW\s*</);
  assert.match(source, /rounded-full bg-rose-400/);
  assert.match(source, /text-rose-300/);
});
