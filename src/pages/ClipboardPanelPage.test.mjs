import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'ClipboardPanelPage.vue'), 'utf8');
const styleSource = readFileSync(join(__dirname, '../style.css'), 'utf8');

test('clipboard panel shell keeps the border and rounded clipping on the same outer frame', () => {
  assert.match(
    pageSource,
    /clipboard-panel-shell[\s\S]*rounded-\[16px\][\s\S]*border border-slate-200[\s\S]*bg-white/,
  );
  assert.doesNotMatch(
    pageSource,
    /<div class="flex h-screen w-screen overflow-hidden bg-slate-200 p-px">/,
  );
});

test('clipboard panel window styles can make the window background transparent', () => {
  assert.match(pageSource, /clipboard-panel-window/);
  assert.match(
    styleSource,
    /html\.clipboard-panel-window,\s*body\.clipboard-panel-window,\s*body\.clipboard-panel-window #app\s*\{/,
  );
  assert.match(styleSource, /background:\s*transparent\b/);
});
