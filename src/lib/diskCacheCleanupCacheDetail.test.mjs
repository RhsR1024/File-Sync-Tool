import assert from 'node:assert/strict';
import { test } from 'node:test';

import { buildCacheDetailPreview } from './diskCacheCleanupCacheDetail.ts';

test('buildCacheDetailPreview prefers storageId and traceId lines for JSON cache values', () => {
  const fullValue = '{"createTime":"2026-04-25 09:59:27","updateTime":"2026-04-25 09:59:27","storageId":"438633731847098368","status":10,"traceId":"f708f78f0a0745cc9dd1b406295b7f79"}';

  assert.equal(
    buildCacheDetailPreview(fullValue, fullValue),
    '"storageId":"438633731847098368"\n"traceId":"f708f78f0a0745cc9dd1b406295b7f79"',
  );
});

test('buildCacheDetailPreview falls back to the backend preview when key fields are absent', () => {
  const fullValue = '{"status":10}';
  const preview = '{"status":10}';

  assert.equal(buildCacheDetailPreview(fullValue, preview), preview);
});
