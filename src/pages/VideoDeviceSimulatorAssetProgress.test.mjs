import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const page = readFileSync(new URL('./VideoDeviceSimulatorPage.vue', import.meta.url), 'utf8');
const composable = readFileSync(new URL('../composables/useDeviceSimulator.ts', import.meta.url), 'utf8');

const activeProgress = page.match(
  /const assetDownloadActive = computed\(\(\) => \{[\s\S]*?\n\}\);/,
)?.[0] ?? '';
const prepareAssets = composable.match(
  /async function prepareAssets\(\)[\s\S]*?(?=\r?\n\s*async function refreshAlarmTypes)/,
)?.[0] ?? '';

assert.match(
  activeProgress,
  /simulator\.assets\.value\?\.state === 'ready'[\s\S]*return false/,
  'authoritative asset readiness must suppress stale active progress',
);
assert.match(
  prepareAssets,
  /jobId && assetProgress\.value\?\.job_id !== jobId/,
  'an early progress event for the returned asset job must not be overwritten',
);

console.log('VideoDeviceSimulator asset progress regression tests PASSED');
