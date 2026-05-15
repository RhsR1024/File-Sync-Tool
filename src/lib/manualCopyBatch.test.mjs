import assert from 'node:assert/strict';
import { test } from 'node:test';

import { resolveBatchTargets } from './manualCopyBatch.ts';

test('resolveBatchTargets returns a single OK entry for a single source with no collision', () => {
  const result = resolveBatchTargets(
    ['\\\\nt03\\share\\UMS\\1.3.9.P10'],
    'E:\\UMS_TEMP',
  );

  assert.equal(result.length, 1);
  assert.equal(result[0].status, 'ok');
  assert.equal(result[0].tail, '1.3.9.P10');
  assert.deepEqual(result[0].disambiguatorSegments, []);
  assert.equal(result[0].effectiveTargetRoot, 'E:\\UMS_TEMP');
  assert.equal(result[0].finalTarget, 'E:\\UMS_TEMP\\1.3.9.P10');
});
