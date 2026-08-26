import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const sidebarSource = readFileSync(join(__dirname, 'Sidebar.vue'), 'utf8');

test('sidebar inherits the rounded app shell height and uses responsive desktop widths', () => {
  assert.match(sidebarSource, /h-full w-72[^\"]*xl:w-80/);
  assert.doesNotMatch(sidebarSource, /h-screen w-64/);
});

test('sidebar keeps defensive truncation and exposes the full translated label', () => {
  assert.match(sidebarSource, /:title="item\.label"/);
  assert.match(sidebarSource, /truncate[^\"]*">\s*\{\{ item\.label \}\}/);
});

test('sidebar warms lazy routes before activation and during browser idle time', () => {
  assert.match(sidebarSource, /@pointerenter="warmRoute\(item\.path\)"/);
  assert.match(sidebarSource, /@focus="warmRoute\(item\.path\)"/);
  assert.match(sidebarSource, /@pointerdown="warmRoute\(item\.path\)"/);
  assert.match(sidebarSource, /window\.requestIdleCallback/);
  assert.match(sidebarSource, /await preloadRoute\(path\)/);
});
