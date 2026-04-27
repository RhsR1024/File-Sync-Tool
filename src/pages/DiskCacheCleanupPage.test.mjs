import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'DiskCacheCleanupPage.vue'), 'utf8');

test('disk cache cleanup does not render cache values inline inside the table rows', () => {
  assert.doesNotMatch(pageSource, /cachePreviewText\(/);
});

test('disk cache cleanup provides an explicit details action for cache values', () => {
  assert.match(pageSource, /viewDetails/);
});

test('disk cache cleanup empty states do not render the unused focus-host action button', () => {
  assert.doesNotMatch(pageSource, /noHostAction/);
  assert.doesNotMatch(pageSource, /@action="focusHostInput"/);
});

test('disk cache cleanup renders an IPSAN sub-view switch for devices and resource groups', () => {
  assert.match(pageSource, /ipsanSubTab/);
  assert.match(pageSource, /resourceGroups/);
});

test('disk cache cleanup delegates cache-status details to a shared inline presenter', () => {
  assert.match(pageSource, /CacheStateInline/);
  assert.doesNotMatch(pageSource, /\{\{\s*t\('diskCacheCleanup\.actions\.viewDetails'\)\s*\}\}/);
});
