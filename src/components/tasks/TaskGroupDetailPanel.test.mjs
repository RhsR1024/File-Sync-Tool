import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const detailSource = readFileSync(join(__dirname, 'TaskGroupDetailPanel.vue'), 'utf8');

test('task detail run history uses one header style and one body style', () => {
  assert.match(
    detailSource,
    /const runHistoryHeadCellClass = 'py-2 px-2 text-left text-\[11px\] font-semibold uppercase tracking-wider text-slate-500';/,
  );
  assert.match(
    detailSource,
    /const runHistoryBodyCellClass = 'py-2\.5 px-2 text-\[12px\] leading-5 text-slate-700';/,
  );
  assert.match(
    detailSource,
    /const runHistoryTimeCellClass = `\$\{runHistoryBodyCellClass\} tabular-nums`;/,
  );
  assert.doesNotMatch(detailSource, /font-mono tabular-nums\}\}\s*<\/td>/);
});

test('server deployment failures include a non-empty server identity and localized status', () => {
  assert.match(detailSource, /serverDisplayLabel\(rollup\)/);
  assert.match(detailSource, /console\.serverDeployFailed/);
  assert.match(detailSource, /attemptStatusLabel\(rollup\.latest_status\)/);
  assert.match(detailSource, /serverStatusRowClass\(rollup\.latest_status\)/);
  assert.match(detailSource, /total: rollup\.attempt_ids\.length/);
  assert.doesNotMatch(detailSource, /failure\.serverName \}\}:/);
});
