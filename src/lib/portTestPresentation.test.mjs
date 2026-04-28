import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  buildPortGridCells,
  filterPortRows,
  parsePorts,
} from './portTestPresentation.ts';

test('parsePorts accepts the full TCP port range without a 1000-port cap', () => {
  const ports = parsePorts('1-1001');

  assert.equal(ports.length, 1001);
  assert.equal(ports[0], 1);
  assert.equal(ports.at(-1), 1001);
});

test('parsePorts supports an all shortcut for every TCP port', () => {
  const ports = parsePorts('all');

  assert.equal(ports.length, 65535);
  assert.equal(ports[0], 1);
  assert.equal(ports.at(-1), 65535);
});

test('parsePorts sorts and de-duplicates mixed single ports and ranges', () => {
  assert.deepEqual(parsePorts('443, 22, 80, 22, 100-102'), [22, 80, 100, 101, 102, 443]);
});

test('buildPortGridCells marks requested ports as open, closed, scanning, or waiting', () => {
  const rows = new Map([
    [22, { port: 22, open: true, latencyMs: 4.2, name: 'SSH' }],
    [23, { port: 23, open: false, latencyMs: null, name: '' }],
  ]);

  assert.deepEqual(buildPortGridCells([22, 23, 24], rows, true), [
    { port: 22, state: 'open', latencyMs: 4.2, name: 'SSH' },
    { port: 23, state: 'closed', latencyMs: null, name: '' },
    { port: 24, state: 'scanning', latencyMs: null, name: '' },
  ]);

  assert.equal(buildPortGridCells([24], new Map(), false)[0].state, 'waiting');
});

test('filterPortRows sorts scanned rows and filters by open or closed status', () => {
  const rows = new Map([
    [443, { port: 443, open: true, latencyMs: 5, name: 'HTTPS' }],
    [22, { port: 22, open: false, latencyMs: null, name: 'SSH' }],
    [80, { port: 80, open: true, latencyMs: 3, name: 'HTTP' }],
  ]);

  assert.deepEqual(filterPortRows(rows, 'open').map(row => row.port), [80, 443]);
  assert.deepEqual(filterPortRows(rows, 'closed').map(row => row.port), [22]);
  assert.deepEqual(filterPortRows(rows, 'all').map(row => row.port), [22, 80, 443]);
});
