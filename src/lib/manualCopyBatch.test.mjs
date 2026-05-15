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

test('resolveBatchTargets disambiguates the 9-path UMS/VMS example at depth 1', () => {
  const sources = [
    '\\\\nt03\\iCPD\\版本\\UMS\\正式版本\\V100R001B02\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\UMS\\正式版本\\V100R001B08\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\UMS\\正式版本\\V100R002B03\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\VMS\\正式版本\\V200R001B01\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\VMS\\正式版本\\V200R001B02\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\VMS\\正式版本\\V200R001B05\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\VMS\\正式版本\\V200R001B11\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\VMS\\正式版本\\V200R001B17\\1.3.9.P10',
    '\\\\nt03\\iCPD\\版本\\UMS-IPSAN\\1.3.9.P10',
  ];

  const result = resolveBatchTargets(sources, 'E:\\UMS_TEMP\\1.3.9.P10');

  assert.equal(result.length, 9);
  result.forEach((r) => assert.equal(r.status, 'ok'));

  const finals = result.map((r) => r.finalTarget);
  assert.deepEqual(finals, [
    'E:\\UMS_TEMP\\1.3.9.P10\\V100R001B02\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V100R001B08\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V100R002B03\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V200R001B01\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V200R001B02\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V200R001B05\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V200R001B11\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\V200R001B17\\1.3.9.P10',
    'E:\\UMS_TEMP\\1.3.9.P10\\UMS-IPSAN\\1.3.9.P10',
  ]);
});
