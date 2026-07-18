import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  DEVICE_SIMULATOR_COMMANDS,
  DEVICE_SIMULATOR_EVENTS,
  createDeviceSimulatorApi,
  hasBlockingPreflightFailure,
  isDeviceSimulatorRuntimeActive,
  isDeviceSimulatorTopologyLocked,
} from './deviceSimulator.ts';

test('command and event names match the approved Tauri contract', () => {
  assert.deepEqual(Object.values(DEVICE_SIMULATOR_COMMANDS), [
    'device_simulator_get_settings',
    'device_simulator_save_settings',
    'device_simulator_list_interfaces',
    'device_simulator_list_profiles',
    'device_simulator_get_asset_status',
    'device_simulator_prepare_assets',
    'device_simulator_cancel_asset_download',
    'device_simulator_preview_devices',
    'device_simulator_preflight',
    'device_simulator_start',
    'device_simulator_stop',
    'device_simulator_get_status',
    'device_simulator_start_alarm',
    'device_simulator_trigger_alarm_once',
    'device_simulator_stop_alarm',
    'device_simulator_recover',
  ]);

  assert.deepEqual(Object.values(DEVICE_SIMULATOR_EVENTS), [
    'device-simulator-status',
    'device-simulator-log',
    'device-simulator-asset-progress',
    'device-simulator-device-status',
    'device-simulator-rtsp-stats',
    'device-simulator-alarm-stats',
    'device-simulator-cleanup-progress',
  ]);
});

test('API wrappers use camelCase Tauri arguments without leaking raw invoke calls to pages', async () => {
  const calls = [];
  const api = createDeviceSimulatorApi(async (command, args) => {
    calls.push([command, args]);
    return command.endsWith('_stop') ? undefined : 'result';
  });
  const request = { marker: 'start-request' };
  const alarmRequest = { marker: 'alarm-request' };

  await api.getSettings();
  await api.saveSettings({ marker: 'settings' });
  await api.listInterfaces();
  await api.listProfiles();
  await api.getAssetStatus(['ipc-custom']);
  await api.prepareAssets(['ipc-custom']);
  await api.cancelAssetDownload('download-1');
  await api.previewDevices(request);
  await api.preflight(request);
  await api.start(request);
  await api.stop();
  await api.getStatus();
  await api.startAlarm(alarmRequest);
  await api.triggerAlarmOnce(alarmRequest);
  await api.stopAlarm('alarm-1');
  await api.recover('session-1');

  assert.deepEqual(calls, [
    [DEVICE_SIMULATOR_COMMANDS.getSettings, undefined],
    [DEVICE_SIMULATOR_COMMANDS.saveSettings, { settings: { marker: 'settings' } }],
    [DEVICE_SIMULATOR_COMMANDS.listInterfaces, undefined],
    [DEVICE_SIMULATOR_COMMANDS.listProfiles, undefined],
    [DEVICE_SIMULATOR_COMMANDS.getAssetStatus, { profileIds: ['ipc-custom'] }],
    [DEVICE_SIMULATOR_COMMANDS.prepareAssets, { profileIds: ['ipc-custom'] }],
    [DEVICE_SIMULATOR_COMMANDS.cancelAssetDownload, { jobId: 'download-1' }],
    [DEVICE_SIMULATOR_COMMANDS.previewDevices, { request }],
    [DEVICE_SIMULATOR_COMMANDS.preflight, { request }],
    [DEVICE_SIMULATOR_COMMANDS.start, { request }],
    [DEVICE_SIMULATOR_COMMANDS.stop, undefined],
    [DEVICE_SIMULATOR_COMMANDS.getStatus, undefined],
    [DEVICE_SIMULATOR_COMMANDS.startAlarm, { request: alarmRequest }],
    [DEVICE_SIMULATOR_COMMANDS.triggerAlarmOnce, { request: alarmRequest }],
    [DEVICE_SIMULATOR_COMMANDS.stopAlarm, { jobId: 'alarm-1' }],
    [DEVICE_SIMULATOR_COMMANDS.recover, { sessionId: 'session-1' }],
  ]);
});

test('runtime activity includes cleanup and recovery ownership, but not validation or stopped states', () => {
  assert.equal(isDeviceSimulatorRuntimeActive('validating'), false);
  assert.equal(isDeviceSimulatorRuntimeActive('running'), true);
  assert.equal(isDeviceSimulatorRuntimeActive('removing_ips'), true);
  assert.equal(isDeviceSimulatorRuntimeActive('recovery_required'), true);
  assert.equal(isDeviceSimulatorRuntimeActive('stopped'), false);
});

test('topology stays locked for every non-terminal operation and residual recovery state', () => {
  assert.equal(isDeviceSimulatorTopologyLocked('idle'), false);
  assert.equal(isDeviceSimulatorTopologyLocked('failed'), false);
  assert.equal(isDeviceSimulatorTopologyLocked('downloading_assets'), true);
  assert.equal(isDeviceSimulatorTopologyLocked('running'), true);
  assert.equal(isDeviceSimulatorTopologyLocked('recovery_required'), true);
});

test('preflight warnings are non-blocking but failed errors remain blocking', () => {
  const warningOnly = {
    ok: true,
    checks: [{ severity: 'warning', status: 'warning' }],
  };
  const failed = {
    ok: false,
    checks: [{ severity: 'error', status: 'failed' }],
  };

  assert.equal(hasBlockingPreflightFailure(warningOnly), false);
  assert.equal(hasBlockingPreflightFailure(failed), true);
});
