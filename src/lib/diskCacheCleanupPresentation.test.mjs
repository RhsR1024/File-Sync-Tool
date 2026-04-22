import assert from 'node:assert/strict';

import { getSuggestedDiskCleanupHosts } from './diskCacheCleanupPresentation.ts';

assert.deepEqual(
  getSuggestedDiskCleanupHosts(
    [
      { enabled: true, host: ' 192.168.1.10 ' },
      { enabled: false, host: '192.168.1.11' },
      { enabled: true, host: '192.168.1.10' },
      { enabled: true, host: '' },
      { enabled: true, host: '10.0.0.8' },
    ],
    ['10.0.0.8'],
  ),
  ['192.168.1.10'],
  'getSuggestedDiskCleanupHosts should only return enabled, unique, trimmed hosts not already in recent history',
);

assert.deepEqual(
  getSuggestedDiskCleanupHosts(
    [
      { enabled: true, host: '10.0.0.8' },
      { enabled: true, host: '10.0.0.9' },
    ],
    ['10.0.0.9', '10.0.0.8'],
  ),
  [],
  'getSuggestedDiskCleanupHosts should return an empty array when every enabled host is already present in recent history',
);

console.log('diskCacheCleanupPresentation tests PASSED');
