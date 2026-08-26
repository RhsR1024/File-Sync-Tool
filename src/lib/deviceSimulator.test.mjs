import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  DEVICE_SIMULATOR_COMMANDS,
  DEVICE_SIMULATOR_EVENTS,
  createDeviceSimulatorApi,
  describeSimulatorError,
  hasBlockingPreflightFailure,
  isDeviceSimulatorRuntimeActive,
  isDeviceSimulatorTopologyLocked,
} from './deviceSimulator.ts';

test('command and event names match the approved Tauri contract', () => {
  assert.deepEqual(Object.values(DEVICE_SIMULATOR_COMMANDS), [
    'device_simulator_get_settings',
    'device_simulator_save_settings',
    'device_simulator_update_platform_servers',
    'device_simulator_migrate_local_materials',
    'device_simulator_list_interfaces',
    'device_simulator_list_profiles',
    'device_simulator_list_alarm_types',
    'device_simulator_list_media_themes',
    'device_simulator_get_local_materials_path',
    'device_simulator_refresh_local_materials',
    'device_simulator_sync_remote_materials',
    'device_simulator_reset_and_sync_remote_materials',
    'device_simulator_get_asset_status',
    'device_simulator_prepare_assets',
    'device_simulator_cancel_asset_download',
    'device_simulator_preview_devices',
    'device_simulator_preflight',
    'device_simulator_start',
    'device_simulator_stop',
    'device_simulator_get_status',
    'device_simulator_import_alarm_image',
    'device_simulator_start_alarm',
    'device_simulator_trigger_alarm_once',
    'device_simulator_stop_alarm',
    'device_simulator_recover',
    'device_simulator_add_devices_to_platform',
  ]);

  assert.deepEqual(Object.values(DEVICE_SIMULATOR_EVENTS), [
    'device-simulator-status',
    'device-simulator-log',
    'device-simulator-asset-progress',
    'device-simulator-device-status',
    'device-simulator-rtsp-stats',
    'device-simulator-alarm-stats',
    'device-simulator-alarm-subscription',
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
  const servers = [{ id: 'server-1', host: '192.115.1.38', port: 80 }];
  await api.updatePlatformServers(servers);
  await api.listInterfaces();
  await api.listProfiles();
  await api.listAlarmTypes();
  await api.listMediaThemes();
  await api.getAssetStatus(['ipc-structured']);
  await api.prepareAssets(['ipc-structured']);
  await api.cancelAssetDownload('download-1');
  await api.previewDevices(request);
  await api.preflight(request);
  await api.start(request);
  await api.stop();
  await api.getStatus();
  await api.importAlarmImage();
  await api.startAlarm(alarmRequest);
  await api.triggerAlarmOnce(alarmRequest);
  await api.stopAlarm('alarm-1');
  await api.recover('session-1');
  const devices = [{ address: '192.115.1.69', port: 80 }];
  const platformRequest = {
    devices,
    serverIds: ['server-1'],
    automaticOnly: false,
    replaceExisting: true,
  };
  await api.addDevicesToPlatform(platformRequest);

  assert.deepEqual(calls, [
    [DEVICE_SIMULATOR_COMMANDS.getSettings, undefined],
    [DEVICE_SIMULATOR_COMMANDS.saveSettings, { settings: { marker: 'settings' } }],
    [DEVICE_SIMULATOR_COMMANDS.updatePlatformServers, { servers }],
    [DEVICE_SIMULATOR_COMMANDS.listInterfaces, undefined],
    [DEVICE_SIMULATOR_COMMANDS.listProfiles, undefined],
    [DEVICE_SIMULATOR_COMMANDS.listAlarmTypes, undefined],
    [DEVICE_SIMULATOR_COMMANDS.listMediaThemes, undefined],
    [DEVICE_SIMULATOR_COMMANDS.getAssetStatus, { profileIds: ['ipc-structured'] }],
    [DEVICE_SIMULATOR_COMMANDS.prepareAssets, { profileIds: ['ipc-structured'] }],
    [DEVICE_SIMULATOR_COMMANDS.cancelAssetDownload, { jobId: 'download-1' }],
    [DEVICE_SIMULATOR_COMMANDS.previewDevices, { request }],
    [DEVICE_SIMULATOR_COMMANDS.preflight, { request }],
    [DEVICE_SIMULATOR_COMMANDS.start, { request }],
    [DEVICE_SIMULATOR_COMMANDS.stop, undefined],
    [DEVICE_SIMULATOR_COMMANDS.getStatus, undefined],
    [DEVICE_SIMULATOR_COMMANDS.importAlarmImage, undefined],
    [DEVICE_SIMULATOR_COMMANDS.startAlarm, { request: alarmRequest }],
    [DEVICE_SIMULATOR_COMMANDS.triggerAlarmOnce, { request: alarmRequest }],
    [DEVICE_SIMULATOR_COMMANDS.stopAlarm, { jobId: 'alarm-1' }],
    [DEVICE_SIMULATOR_COMMANDS.recover, { sessionId: 'session-1' }],
    [DEVICE_SIMULATOR_COMMANDS.addDevicesToPlatform, { request: platformRequest }],
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

test('rejected simulator commands keep their code and details in one log line', () => {
  assert.equal(
    describeSimulatorError({
      code: 'device_simulator.recovery.journal_invalid',
      message_key: 'deviceSimulator.errors.sessionJournalInvalid',
      details: 'decode session journal',
      retryable: false,
    }),
    'device_simulator.recovery.journal_invalid | deviceSimulator.errors.sessionJournalInvalid | decode session journal',
  );
  assert.equal(describeSimulatorError({ unexpected: 1 }), '{"unexpected":1}');
  assert.equal(describeSimulatorError(new Error('boom')), 'Error: boom');
  assert.equal(describeSimulatorError('command not found'), 'command not found');
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
