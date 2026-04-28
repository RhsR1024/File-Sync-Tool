import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'FrameworkPasswordPage.vue'), 'utf8');

test('framework password result table keeps the status on one line and lets long messages wrap', () => {
  assert.match(pageSource, /const frameworkResultStatusWrapClass = 'flex items-center gap-2 whitespace-nowrap';/);
  assert.match(pageSource, /<table class="w-full table-fixed">/);
  assert.match(pageSource, /<td :class="frameworkResultMessageCellClass">\s*\{\{ result\.message \}\}\s*<\/td>/);
  assert.match(pageSource, /const frameworkResultMessageCellClass = 'px-6 py-3 text-sm text-slate-600 break-all';/);
});
