import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const dialogSource = readFileSync(join(__dirname, 'UpdateDialog.vue'), 'utf8');

test('update dialog keeps long changelogs inside a viewport-bound scroll area', () => {
  assert.match(
    dialogSource,
    /max-h-\[calc\(100vh-2rem\)\][^"]*flex[^"]*flex-col[^"]*overflow-hidden/,
    'dialog panel should be height constrained and use a column layout',
  );
  assert.match(
    dialogSource,
    /flex[^"]*min-h-0[^"]*flex-1[^"]*flex-col[^"]*gap-\d+[^"]*px-6[^"]*py-7/,
    'dialog body should allow its middle content to shrink before scrolling',
  );
  assert.match(
    dialogSource,
    /scrollbar-light[^"]*min-h-0[^"]*flex-1[^"]*overflow-y-auto[^"]*overscroll-contain/,
    'release notes card should scroll instead of pushing action buttons off-screen',
  );
  assert.match(
    dialogSource,
    /class="shrink-0 flex justify-end gap-3"/,
    'found-update actions should stay outside the scroll area',
  );
});
