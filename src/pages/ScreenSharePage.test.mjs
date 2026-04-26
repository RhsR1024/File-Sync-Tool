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
