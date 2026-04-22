import assert from 'node:assert/strict';

import { mergeRecentItems, normalizeRecentItems, removeRecentItems } from './recentHistory.ts';

assert.deepEqual(
  normalizeRecentItems([' 192.168.1.10 ', '', '10.0.0.5', '192.168.1.10', '   '], 10),
  ['192.168.1.10', '10.0.0.5'],
  'normalizeRecentItems should trim, drop empties, and de-duplicate in order',
);

assert.deepEqual(
  normalizeRecentItems(['1', '2', '3', '4'], 3),
  ['1', '2', '3'],
  'normalizeRecentItems should keep the newest saved items within the limit',
);

assert.deepEqual(
  mergeRecentItems(['10.0.0.5', '172.16.0.2'], [' 192.168.1.10 ', '10.0.0.5'], 10),
  ['192.168.1.10', '10.0.0.5', '172.16.0.2'],
  'mergeRecentItems should prepend new values and keep existing uniques behind them',
);

assert.deepEqual(
  mergeRecentItems(['1', '2', '3'], ['4', '2', '5'], 4),
  ['4', '2', '5', '1'],
  'mergeRecentItems should apply the limit after merging',
);

assert.deepEqual(
  removeRecentItems(['10.0.0.5', '172.16.0.2', '192.168.1.10'], '172.16.0.2', 10),
  ['10.0.0.5', '192.168.1.10'],
  'removeRecentItems should drop one saved item and keep the remaining order',
);

assert.deepEqual(
  removeRecentItems(['1', '2', '3', '4'], ['2', '4'], 2),
  ['1', '3'],
  'removeRecentItems should support removing multiple values and still honor the limit',
);

console.log('recentHistory tests PASSED');
