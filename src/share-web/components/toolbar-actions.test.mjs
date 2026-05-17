import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(__dirname, 'ToolbarActions.vue'), 'utf8');

test('download all stays visible for writable directories when archive download is allowed', () => {
  const showDownloadAllBlock = source.match(/const showDownloadAll = computed\(\(\) => \([\s\S]*?\)\);/)?.[0] ?? '';

  assert.match(showDownloadAllBlock, /props\.hasEntries/);
  assert.match(showDownloadAllBlock, /props\.permissions\?\.download_archive/);
  assert.doesNotMatch(showDownloadAllBlock, /!canUploadFiles/);
  assert.doesNotMatch(showDownloadAllBlock, /!canUploadDirectory/);
  assert.doesNotMatch(showDownloadAllBlock, /!canCreateDirectory/);
  assert.doesNotMatch(showDownloadAllBlock, /!canCreateText/);
});
