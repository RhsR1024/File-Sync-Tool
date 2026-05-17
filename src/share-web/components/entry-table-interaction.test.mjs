import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(__dirname, 'EntryTable.vue'), 'utf8');
const styleSource = readFileSync(join(__dirname, '..', 'style.css'), 'utf8');

test('list file names are selectable text instead of download/open buttons', () => {
  assert.match(source, /v-if="entry\.is_dir"[\s\S]*class="name-cell interactive"[\s\S]*@click="emit\('open', entry\)"/);
  assert.match(source, /v-else[\s\S]*class="name-cell"[\s\S]*<span class="name-text">/);
  assert.doesNotMatch(source, /<button type="button" class="name-cell" @click="emit\('open', entry\)">/);
});

test('row action buttons are large enough for comfortable repeated use', () => {
  const rowActionBlock = styleSource.match(/\.row-action\s*\{[\s\S]*?\n\}/)?.[0] ?? '';
  const rowActionSvgBlock = styleSource.match(/\.row-action svg\s*\{[\s\S]*?\n\}/)?.[0] ?? '';

  assert.match(rowActionBlock, /width:\s*34px/);
  assert.match(rowActionBlock, /height:\s*34px/);
  assert.match(rowActionSvgBlock, /width:\s*17px/);
  assert.match(rowActionSvgBlock, /height:\s*17px/);
});
