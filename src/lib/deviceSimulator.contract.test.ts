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
  local_materials_directory: 'D:\\VirtualDeviceMaterials',
  selected_interface_id: 'adapter-1',
  last_platform: 'ums',
  last_start_ip: '192.168.50.10',
  last_device_ips: [],
  last_subnet_prefix: 24,
  last_platform_servers: [{ id: 'server-1', host: '192.168.50.2', port: 80 }],
  last_platform_access_mode: 'configured_servers_only',
  last_alarm_receiver_url: 'http://192.168.50.2/alarm',
  last_alarm_receiver_port: 55025,
  last_device_groups: [{
    id: 'group-1',
    profile_id: 'ipc-structured',
    count: 10,
  }],
  last_http_port: 81,
  last_rtsp_ports: { main: 554, sub: 555, third: 556 },
  last_media_theme_id: 'fanren-xiuxian',
  last_time_watermark_enabled: true,
  auto_check_asset_updates: true,
  manage_firewall: true,
  platform_username: 'loadmin',
  platform_password: 'admin_123',
  platform_auto_add_devices: true,
  platform_replace_existing_devices: false,
};

export const simulatorStartRequestContract: SimulatorStartRequest = {
  platform: {
    kind: 'ums',
    servers: [{ id: 'server-1', host: '192.168.50.2', port: 80 }],
    access_mode: 'configured_servers_only',
    alarm_receiver_url: 'http://192.168.50.2/alarm',
    alarm_receiver_port: 55025,
  },
  interface_id: 'adapter-1',
  start_ip: '192.168.50.10',
  device_ips: [],
  subnet_prefix: 24,
  device_http_port: 81,
  rtsp_ports: { main: 554, sub: 555, third: 556 },
  media_theme_id: 'fanren-xiuxian',
  groups: [{
    id: 'group-1',
    profile_id: 'ipc-structured',
    count: 10,
  }],
  stream: {
    transport: 'tcp_interleaved',
    enabled_streams: ['main', 'sub', 'third'],
    audio_enabled: false,
    time_watermark_enabled: true,
  },
};

export const alarmJobRequestContract: AlarmJobRequest = {
  target_device_ids: ['device-1'],
  alarm_profile_id: 'ipc-structured',
  alarm_type_ids: ['car'],
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

export const alarmSubscriptionEventContract: DeviceSimulatorEventPayloads[
  typeof DEVICE_SIMULATOR_EVENTS.alarmSubscription
] = {
  destinations: ['http://192.115.1.55:22815/'],
  learned: true,
  host: '192.115.1.55',
  port: 22815,
  duration_secs: 600,
  learned_at_ms: 1_784_773_245_000,
  expires_at_ms: 1_784_773_845_000,
  overridden: false,
};
