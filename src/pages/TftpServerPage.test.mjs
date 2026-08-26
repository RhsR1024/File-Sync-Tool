import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const pageSource = readFileSync(new URL('./TftpServerPage.vue', import.meta.url), 'utf8');

test('TFTP overlaps interface discovery with status hydration without racing the shared root', () => {
  assert.match(pageSource, /const interfacesPromise = screenShareListInterfaces\(\)/);
  assert.match(pageSource, /await Promise\.all\(\[interfacesPromise, refreshStatus\(true\)\]\);/);
  assert.match(
    pageSource,
    /await Promise\.all\(\[interfacesPromise, refreshStatus\(true\)\]\);\s+await refreshFiles\(false\);/,
  );
});
