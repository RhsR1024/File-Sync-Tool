import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'SettingsPage.vue'), 'utf8');

test('settings page no longer includes the quick-jump anchor strip polish', () => {
  assert.doesNotMatch(pageSource, /const SETTINGS_SECTIONS:/);
  assert.doesNotMatch(pageSource, /function scrollToSection\(id: string\)/);
  assert.doesNotMatch(pageSource, /settings\.anchor\.label/);
  assert.doesNotMatch(pageSource, /settings\.anchor\.scrollHint/);
  assert.doesNotMatch(pageSource, /sticky top-0/);
  assert.doesNotMatch(pageSource, /scroll-mt-24/);
  assert.doesNotMatch(pageSource, /data-section-heading/);
  assert.doesNotMatch(pageSource, /settings\.section\./);
  assert.doesNotMatch(pageSource, /pb-28/);
});

test('settings page uses existing locale keys for task and timing section titles', () => {
  assert.match(pageSource, /settings\.scanTasks/);
  assert.match(pageSource, /settings\.scanTime/);
  assert.doesNotMatch(pageSource, /settings\.scanTaskManagement/);
  assert.doesNotMatch(pageSource, /settings\.scanTiming/);
});
