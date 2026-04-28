import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const tableSource = readFileSync(join(__dirname, 'TaskGroupsTable.vue'), 'utf8');

test('task groups table enlarges the start time text and centers inactive metric placeholders', () => {
  assert.match(
    tableSource,
    /const startTimeTextClass = 'text-\[12px\] text-slate-500 font-medium tabular-nums whitespace-nowrap';/,
  );
  assert.match(
    tableSource,
    /const inactiveMetricPlaceholderClass = 'inline-flex w-full items-center justify-center text-\[12px\] text-slate-300';/,
  );
  assert.match(tableSource, />--<\/span>/);
});
