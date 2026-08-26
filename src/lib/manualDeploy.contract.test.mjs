import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const tauriSource = readFileSync(new URL('./tauri.ts', import.meta.url), 'utf8');
const editorSource = readFileSync(new URL('../components/sync/SyncConfigurationEditor.vue', import.meta.url), 'utf8');
const localeSource = readFileSync(new URL('../locales/messages.ts', import.meta.url), 'utf8');
const rustMainSource = readFileSync(new URL('../../src-tauri/src/main.rs', import.meta.url), 'utf8');
const rustDeploySource = readFileSync(new URL('../../src-tauri/src/deploy.rs', import.meta.url), 'utf8');

test('manual deployment strategy contract stays aligned across TypeScript and Rust', () => {
  for (const field of ['transfer_policy', 'extract_policy', 'extract_dir', 'extract_command_group_id']) {
    assert.match(tauriSource, new RegExp(field));
    assert.match(rustMainSource, new RegExp(field));
  }
  assert.match(tauriSource, /'smart' \| 'always' \| 'remote_only'/);
  assert.match(tauriSource, /'auto' \| 'force' \| 'skip'/);
  assert.match(rustDeploySource, /enum ManualDeployTransferPolicy/);
  assert.match(rustDeploySource, /enum ManualDeployExtractPolicy/);
  assert.match(rustDeploySource, /\.file-sync-deploy\.json/);
});

test('manual deployment preflight is registered and exposed to the UI', () => {
  assert.match(rustMainSource, /async fn preflight_manual_deploy/);
  assert.match(rustMainSource, /test_ssh_connection,\s+preflight_manual_deploy,\s+start_manual_copy_task,/);
  assert.match(tauriSource, /invoke\('preflight_manual_deploy', \{ request \}\)/);
  assert.match(editorSource, /handleManualPreflight/);
});

test('manual deployment policy labels exist in both locale blocks', () => {
  for (const key of [
    'manualTransferPolicy_smart',
    'manualTransferPolicy_remote_only',
    'manualExtractPolicy_auto',
    'manualExtractPolicy_force',
    'manualPreflightResults',
  ]) {
    assert.equal(localeSource.match(new RegExp(`${key}:`, 'g'))?.length, 2);
  }
});
