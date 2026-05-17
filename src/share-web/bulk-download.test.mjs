import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(__dirname, 'App.vue'), 'utf8');

test('bulk download creates a single archive request for selected nodes', () => {
  const bulkDownloadBlock = source.match(/function bulkDownload\(\) \{[\s\S]*?\n\}/)?.[0] ?? '';

  assert.match(bulkDownloadBlock, /downloadSelectionArchiveUrl/);
  assert.match(bulkDownloadBlock, /items\.map\(\(entry\) => entry\.node_id\)/);
  assert.doesNotMatch(bulkDownloadBlock, /for \(const entry of items\)[\s\S]*triggerDownload/);
});
