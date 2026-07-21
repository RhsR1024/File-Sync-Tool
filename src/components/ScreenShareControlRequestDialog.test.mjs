import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const source = readFileSync(
  new URL('./ScreenShareControlRequestDialog.vue', import.meta.url),
  'utf8',
);

test('remote-control approval is a centered modal with explicit safe actions', () => {
  assert.match(source, /fixed inset-0[^\"]*items-center justify-center/);
  assert.match(source, /role="dialog"/);
  assert.match(source, /aria-modal="true"/);
  assert.match(source, /ref="denyButton"/);
  assert.match(source, /nextTick\(\(\) => denyButton\.value\?\.focus\(\)\)/);
  assert.match(source, /@keydown\.esc\.stop\.prevent="deny"/);
  assert.doesNotMatch(source, /@(?:click|mousedown)\.self/);
});

test('remote-control approval traps focus and disables both decisions while responding', () => {
  assert.match(source, /@keydown\.tab\.stop="keepFocusInside"/);
  assert.match(source, /@click="emit\('allow'\)"/);
  assert.ok((source.match(/:disabled="busy"/g) ?? []).length >= 3);
  assert.ok((source.match(/min-h-11|h-11/g) ?? []).length >= 3);
});
