import type {
  AlarmJobRequest,
  DeviceSimulatorEventPayloads,
  DeviceSimulatorSettings,
  ImportedAlarmImage,
  SimulatorStartRequest,
  SimulatorStatus,
} from './deviceSimulator';
import { DEVICE_SIMULATOR_EVENTS } from './deviceSimulator';

export const deviceSimulatorSettingsContract: DeviceSimulatorSettings = {
  asset_server_url_override: null,
  selected_interface_id: 'adapter-1',
  last_platform: 'vms',
  last_start_ip: '192.168.50.10',
  last_device_groups: [{
    id: 'group-1',
    profile_id: 'ipc-custom',
    count: 10,
    nvr_channel_count: null,
  }],
  last_http_port: 81,
  last_rtsp_ports: { main: 554, sub: 555, third: 556 },
  auto_check_asset_updates: true,
  manage_firewall: true,
};

export const simulatorStartRequestContract: SimulatorStartRequest = {
  platform: {
    kind: 'vms',
    servers: [{ id: 'server-1', host: '192.168.50.2', port: 80 }],
    alarm_receiver_url: 'http://192.168.50.2/alarm',
  },
  interface_id: 'adapter-1',
  start_ip: '192.168.50.10',
  subnet_prefix: 24,
  device_http_port: 81,
  rtsp_ports: { main: 554, sub: 555, third: 556 },
  groups: [{
    id: 'group-1',
    profile_id: 'ipc-custom',
    count: 10,
    nvr_channel_count: null,
  }],
  stream: {
    transport: 'tcp_interleaved',
    enabled_streams: ['main', 'sub', 'third'],
    audio_enabled: false,
  },
};

export const alarmJobRequestContract: AlarmJobRequest = {
  target_device_ids: ['device-1'],
  alarm_profile_id: 'ipc-custom',
  alarm_type_ids: ['custom-alarm'],
  mode: 'configured',
  interval_ms: 1000,
  send_count: null,
  recovery_delay_secs: 5,
  image_variant: 'normal',
  user_image_id: null,
};

export const importedAlarmImageContract: ImportedAlarmImage = {
  image_id: 'a'.repeat(64),
  file_name: 'alarm.png',
  extension: 'png',
  size: 1024,
};

export const simulatorStatusContract: SimulatorStatus = {
  state: 'running',
  session_id: 'session-1',
  started_at: '2026-07-18T12:00:00+08:00',
  phase_progress: 1,
  metrics: {
    total_devices: 10,
    online_devices: 10,
    total_channels: 10,
    active_rtsp_clients: 1,
    outbound_bitrate_kbps: 2048,
    active_alarm_jobs: 1,
  },
  cleanup_stage: null,
  recovery_session_id: null,
  last_error: null,
};

export const statusEventContract: DeviceSimulatorEventPayloads[
  typeof DEVICE_SIMULATOR_EVENTS.status
] = simulatorStatusContract;
