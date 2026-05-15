import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));

const requiredFiles = [
  'README.md',
  'shared.css',
  'mock-data.js',
  'index.html',
  'editorial-workspace.html',
  'action-gallery.html',
  'content-first.html',
];

test('file share homepage design artifacts exist', () => {
  for (const file of requiredFiles) {
    assert.equal(existsSync(join(__dirname, file)), true, `${file} should exist`);
  }
});

test('file share homepage variants expose the planned titles', () => {
  const variants = [
    ['editorial-workspace.html', 'Editorial Workspace'],
    ['action-gallery.html', 'Action Gallery'],
    ['content-first.html', 'Content First'],
  ];

  for (const [file, title] of variants) {
    const source = readFileSync(join(__dirname, file), 'utf8');
    assert.match(source, new RegExp(title));
  }
});
