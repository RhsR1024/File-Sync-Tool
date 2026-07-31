import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'FileSharePage.vue'), 'utf8');
const tauriSource = readFileSync(join(__dirname, '..', 'lib', 'tauri.ts'), 'utf8');

test('file share stop action executes directly without a confirmation prompt', () => {
  assert.match(pageSource, /const stopShare = async \(\) =>/);
  assert.doesNotMatch(pageSource, /window\.confirm\(t\('tools\.fileShare\.stopConfirm'\)\)/);
  assert.match(pageSource, /@click="stopShare"/);
  assert.doesNotMatch(pageSource, /@click="confirmStopShare"/);
});

test('file share shared roots do not expose an editable alias field', () => {
  assert.doesNotMatch(pageSource, /tools\.fileShare\.aliasLabel/);
  assert.doesNotMatch(pageSource, /<input v-model="root\.alias"/);
});

test('file share shared roots render the path as the primary single-line label', () => {
  assert.match(pageSource, /class="fs-root-main"/);
  assert.match(pageSource, /:title="root\.path"/);
  assert.match(pageSource, /\{\{ root\.path \}\}/);
  assert.doesNotMatch(pageSource, /displayRootName\(root\)/);
  assert.doesNotMatch(pageSource, /class="fs-root-path"/);
});

test('new shared roots grant read-only access to every account by default', () => {
  assert.match(pageSource, /const readOnlyRootPermission = \(rootId: string\): FileShareUserRootPermissions =>/);
  assert.match(pageSource, /const grantReadOnlyRootToAllUsers = \(rootId: string\) =>/);
  assert.match(pageSource, /for \(const user of \[draft\.value\.guest_account, \.\.\.draft\.value\.accounts\]\)/);
  assert.match(pageSource, /grantReadOnlyRootToAllUsers\(root\.id\)/);
});

test('file share account password fields preload persisted plaintext passwords', () => {
  assert.match(tauriSource, /password_plain:\s*string \| null/);
  assert.match(pageSource, /password_plain:\s*null/);
  assert.match(pageSource, /new_password:\s*a\.password_plain \?\? ''/);
  assert.doesNotMatch(pageSource, /v-model="(?:guest|account)\.new_password"\s+type="password"/);
});

test('file share save action labels restart when sharing is active', () => {
  assert.match(pageSource, /isActive \? startShare\(true, true\) : saveSettings\(\)/);
  assert.match(pageSource, /isActive \? t\('tools\.fileShare\.saveAndRestart'\) : t\('tools\.fileShare\.saveSettings'\)/);
});

test('file share header keeps the product description while sharing is active', () => {
  assert.match(pageSource, /\{\{ t\('tools\.fileShare\.consoleDescription'\) \}\}/);
  assert.doesNotMatch(pageSource, /isActive \? serverUrl : t\('tools\.fileShare\.consoleDescription'\)/);
});
