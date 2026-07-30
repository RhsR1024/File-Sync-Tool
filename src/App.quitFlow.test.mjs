import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const appSource = readFileSync(new URL('./App.vue', import.meta.url), 'utf8');
const dialogSource = readFileSync(new URL('./components/QuitConfirmDialog.vue', import.meta.url), 'utf8');
const mainSource = readFileSync(new URL('../src-tauri/src/main.rs', import.meta.url), 'utf8');
const simulatorCommandsSource = readFileSync(
  new URL('../src-tauri/src/device_simulator_commands.rs', import.meta.url),
  'utf8',
);

test('quit blockers use an application-owned modal instead of browser dialogs', () => {
  assert.doesNotMatch(appSource, /window\.(?:alert|confirm|prompt)\s*\(/);
  assert.match(appSource, /deviceSimulatorApi\.getStatus\(\)/);
  assert.match(appSource, /:simulator-cleanup-required="quitConfirmSimulatorCleanup"/);
  assert.match(dialogSource, /<Teleport to="body">/);
  assert.match(dialogSource, /role="alertdialog"/);
  assert.match(dialogSource, /@keydown\.tab\.stop="keepFocusInside"/);
  assert.match(dialogSource, /retryCleanupAndExit/);
});

test('tray exit foregrounds the app before emitting the quit request', () => {
  const trayQuit = mainSource.slice(
    mainSource.indexOf('TRAY_QUIT_ID =>'),
    mainSource.indexOf('_ => {}', mainSource.indexOf('TRAY_QUIT_ID =>')),
  );
  assert.ok(trayQuit.indexOf('show_main_window') < trayQuit.indexOf('emit("before-quit"'));
});

test('confirmed exit recovers residual simulator sessions before exiting', () => {
  const shutdown = simulatorCommandsSource.slice(
    simulatorCommandsSource.indexOf('pub async fn shutdown_for_exit'),
    simulatorCommandsSource.indexOf('async fn pin_runtime_assets'),
  );
  assert.match(shutdown, /SessionState::RecoveryRequired \| SessionState::Recovering/);
  assert.match(shutdown, /recover_session\(app_handle, state, session_id\)\.await/);
  assert.doesNotMatch(shutdown, /Let exit proceed/);
});
