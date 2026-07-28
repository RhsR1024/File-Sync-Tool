import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'ScreenSharePage.vue'), 'utf8');

test('screen share shows the connected IP list directly without a details toggle gate', () => {
  assert.match(pageSource, /tools\.screenShare\.connectedIpList/);
  assert.doesNotMatch(pageSource, /v-if="showConnectionDetails"/);
  assert.doesNotMatch(pageSource, /@click="showConnectionDetails = !showConnectionDetails"/);
  assert.doesNotMatch(pageSource, /tools\.screenShare\.connectionDetails/);
});

test('screen share still collapses connected IPs after the first 10 entries', () => {
  assert.match(pageSource, /connectedIps\.value\.slice\(0, 10\)/);
  assert.match(pageSource, /Math\.max\(0, connectedIps\.value\.length - 10\)/);
});

test('screen share places connection count and uptime side by side like file share', () => {
  assert.match(
    pageSource,
    /<div class="ss-stat-card">[\s\S]*?tools\.screenShare\.connectionCount[\s\S]*?{{ connectionCount }}[\s\S]*?<\/div>\s*<div class="ss-stat-card">[\s\S]*?tools\.screenShare\.uptime[\s\S]*?{{ formattedUptime }}/,
  );
});

test('screen share stop action executes directly without a confirmation prompt', () => {
  assert.doesNotMatch(pageSource, /window\.confirm\(t\('tools\.screenShare\.stopConfirm'\)\)/);
  assert.match(pageSource, /@click="stopShare"/);
  assert.doesNotMatch(pageSource, /@click="confirmStopShare"/);
});

test('screen share offers the 60 FPS experiment tier without changing the default', () => {
  assert.match(pageSource, /const EXPERIMENTAL_HIGH_FPS = 60;/);
  assert.match(pageSource, /const highFpsExperiment = ref\(false\);/);
  assert.match(
    pageSource,
    /const effectiveFps = computed\(\(\) => \(highFpsExperiment\.value \? EXPERIMENTAL_HIGH_FPS : fps\.value\)\);/,
  );
  assert.match(pageSource, /tools\.screenShare\.highFpsExperiment/);
  assert.match(pageSource, /tools\.screenShare\.highFpsExperimentDesc/);
  // 常规滑块保持 5-30，60 FPS 只能由实验开关选择。
  assert.match(pageSource, /v-model\.number="fps"[\s\S]*?min="5"[\s\S]*?max="30"/);
});

test('screen share starts and persists the experiment tier instead of the raw slider value', () => {
  assert.match(pageSource, /fps: effectiveFps\.value,/);
  assert.match(pageSource, /highFpsExperiment: highFpsExperiment\.value,/);
  assert.match(pageSource, /highFpsExperiment\.value = saved\.highFpsExperiment \?\? false;/);
});

test('screen share exposes explicit capture backend modes with explanatory helper copy', () => {
  assert.match(pageSource, /tools\.screenShare\.backendMode/);
  assert.match(pageSource, /tools\.screenShare\.backendModeHint/);
  assert.match(pageSource, /tools\.screenShare\.backendModeAuto/);
  assert.match(pageSource, /tools\.screenShare\.backendModeWgc/);
  assert.match(pageSource, /tools\.screenShare\.backendModeDxgi/);
  assert.match(pageSource, /tools\.screenShare\.backendModeAutoDesc/);
  assert.match(pageSource, /tools\.screenShare\.backendModeWgcDesc/);
  assert.match(pageSource, /tools\.screenShare\.backendModeDxgiDesc/);
});
