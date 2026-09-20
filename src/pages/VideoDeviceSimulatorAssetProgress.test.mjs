import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const composable = readFileSync(new URL('../composables/useDeviceSimulator.ts', import.meta.url), 'utf8');
const prepareAssets = composable.match(
  /async function prepareAssets\(\)[\s\S]*?(?=\r?\n\s*async function refreshAlarmTypes)/,
)?.[0] ?? '';

assert.match(
  composable,
  /payload\.state === 'ready' \|\| payload\.state === 'failed'[\s\S]*?getAssetStatus\(selectedProfileIds\.value\)[\s\S]*?assets\.value = status/,
  'terminal progress must refresh the authoritative asset status',
);
assert.match(
  prepareAssets,
  /jobId && assetProgress\.value\?\.job_id !== jobId/,
  'an early progress event for the returned asset job must not be overwritten',
);

console.log('VideoDeviceSimulator asset progress regression tests PASSED');
