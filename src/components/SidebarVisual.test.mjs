import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const sidebarSource = readFileSync(join(__dirname, 'Sidebar.vue'), 'utf8');
const scrollbarSource = readFileSync(join(__dirname, '..', 'styles', 'scrollbar.css'), 'utf8');

test('sidebar uses a hover-revealed dark scrollbar with a scroll fade', () => {
  assert.match(sidebarSource, /sidebar-scroll-region/);
  assert.match(sidebarSource, /scrollbar-sidebar/);
  assert.match(sidebarSource, /sidebar-scroll-fade/);
  assert.match(scrollbarSource, /\.scrollbar-sidebar/);
  assert.match(scrollbarSource, /scrollbar-color:\s*transparent transparent/);
  assert.match(scrollbarSource, /\.scrollbar-sidebar:hover/);
});

test('sidebar navigation rows use the restrained hover treatment from the handoff', () => {
  assert.match(sidebarSource, /hover:-translate-y-0\.5/);
  assert.match(sidebarSource, /hover:shadow-\[0_10px_24px_rgba\(2,6,23,0\.32\)\]/);
  assert.match(sidebarSource, /motion-reduce:hover:translate-y-0/);
});
