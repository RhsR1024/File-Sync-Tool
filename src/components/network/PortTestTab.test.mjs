import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(__dirname, 'PortTestTab.vue'), 'utf8');

test('port test grid switches large scans to open-port cards', () => {
  assert.match(source, /buildOpenPortCards/);
  assert.match(source, /type OpenPortCard/);
  assert.match(source, /const LARGE_SCAN_GRID_THRESHOLD = 1024/);
  assert.match(source, /const isLargeScan = computed\(\(\) => totalPorts\.value > LARGE_SCAN_GRID_THRESHOLD\)/);
  assert.match(source, /const openPortCards = computed<OpenPortCard\[\]>\(\(\) => buildOpenPortCards\(resultRows\.value\)\)/);
  assert.match(source, /<template v-if="isLargeScan">/);
  assert.match(source, /v-if="openPortCards\.length > 0"/);
  assert.match(source, /v-for="card in openPortCards"/);
  assert.match(source, /networkTools\.port\.scanningNoOpenYet/);
  assert.match(source, /networkTools\.port\.completeNoOpen/);
});

test('port test keeps small scans as enlarged labelled overview cells', () => {
  assert.match(source, /minmax\(\$\{showCellLabels\.value \? '56px' : '10px'\}, 1fr\)/);
  assert.match(source, /gap: '6px'/);
  assert.match(source, /rounded-md flex aspect-square items-center justify-center text-xs font-mono font-medium/);
  assert.match(source, /<template v-else>/);
  assert.match(source, /v-for="cell in gridCells"/);
  assert.match(source, /<span v-if="showCellLabels">\{\{ cell\.port \}\}<\/span>/);
});
