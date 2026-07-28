import { invoke } from '@tauri-apps/api/core';

export type DeviceSimulatorPlatform = 'ums';
export type DeviceSimulatorDeviceKind = 'ipc' | 'nvr';
export type DeviceSimulatorStreamKind = 'main' | 'sub' | 'third';

export type SimulatorSessionState =
  | 'idle'
  | 'validating'
  | 'assets_required'
  | 'downloading_assets'
  | 'preflighting'
  | 'starting_worker'
  | 'adding_ips'
  | 'starting_services'
  | 'running'
  | 'stopping_alarms'
  | 'stopping_services'
  | 'removing_firewall'
  | 'removing_ips'
  | 'stopped'
  | 'failed'
  | 'recovery_required'
  | 'recovering';

export type SimulatorAssetState =
  | 'unknown'
  | 'checking'
  | 'missing'
  | 'downloading'
  | 'verifying'
  | 'installing'
  | 'ready'
  | 'update_available'
  | 'failed';

export type SimulatorAlarmJobState =
  | 'idle'
  | 'starting'
  | 'running'
  | 'stopping'
  | 'completed'
  | 'failed';

/**
 * Which callers a running session answers. Virtual devices carry no protocol
 * credentials, so `open` lets any platform that can reach them add them;
 * `configured_servers_only` restricts discovery and HTTP to the addresses the
 * configured platform servers resolve to. RTSP is not gated by this setting.
 */
export type PlatformAccessMode = 'open' | 'configured_servers_only';

export type SimulatorCheckSeverity = 'info' | 'warning' | 'error';
export type SimulatorCheckStatus = 'passed' | 'warning' | 'failed' | 'skipped';
export type SimulatorLogLevel = 'trace' | 'debug' | 'info' | 'warning' | 'error';
export type AlarmDispatchMode = 'configured' | 'random' | 'sequential';

export interface RtspPorts {
  main: number;
  sub: number;
  third: number;
}

export interface DeviceGroupDraft {
  id: string;
  profile_id: string;
  count: number;
  nvr_channel_count: number | null;
}

/** Persisted application-domain settings. Runtime credentials never belong here. */
export interface DeviceSimulatorSettings {
  asset_server_url_override: string | null;
  selected_interface_id: string | null;
  last_platform: DeviceSimulatorPlatform | null;
  last_start_ip: string | null;
  last_device_ips: string[];
  last_subnet_prefix: number;
  last_platform_servers: TargetPlatformServer[];
  last_platform_access_mode: PlatformAccessMode;
  last_alarm_receiver_url: string | null;
  last_alarm_receiver_port: number | null;
  last_device_groups: DeviceGroupDraft[];
  last_http_port: number;
  last_rtsp_ports: RtspPorts;
  last_media_theme_id: string;
  auto_check_asset_updates: boolean;
  manage_firewall: boolean;
}

export interface TargetPlatformServer {
  id: string;
  host: string;
  port: number;
}

export interface TargetPlatformConfig {
  kind: DeviceSimulatorPlatform;
  servers: TargetPlatformServer[];
  access_mode: PlatformAccessMode;
  alarm_receiver_url: string | null;
  alarm_receiver_port: number | null;
}

export interface DeviceGroupConfig extends DeviceGroupDraft {}

/** The approved first-release media policy: TCP interleaving, three streams, no audio. */
export interface StreamRuntimeConfig {
  transport: 'tcp_interleaved';
  enabled_streams: DeviceSimulatorStreamKind[];
  audio_enabled: false;
}

export interface SimulatorStartRequest {
  platform: TargetPlatformConfig;
  interface_id: string;
  start_ip: string;
  device_ips: string[];
  subnet_prefix: number;
  device_http_port: number;
  rtsp_ports: RtspPorts;
  media_theme_id: string;
  groups: DeviceGroupConfig[];
  stream: StreamRuntimeConfig;
}

export interface SimulatorNetworkInterfaceInfo {
  id: string;
  name: string;
  description: string;
  is_enabled: boolean;
  is_up: boolean;
  ipv4_addresses: string[];
}

export type DeviceProfileAvailability =
  | 'local'
  | 'remote'
  | 'update_available'
  | 'unavailable';

export interface DeviceProfileSummary {
  id: string;
  display_name_key: string;
  device_kind: DeviceSimulatorDeviceKind;
  supported_platforms: DeviceSimulatorPlatform[];
  availability: DeviceProfileAvailability;
  installed_version: string | null;
  available_version: string | null;
  verified_platforms: DeviceSimulatorPlatform[];
}

export interface AlarmTypeSummary {
  id: string;
  display_name: string;
  supports_pictures: boolean;
}

export interface ProfileAlarmTypes {
  profile_id: string;
  alarm_types: AlarmTypeSummary[];
}

export interface MediaThemeSummary {
  id: string;
  display_name_key: string;
  is_default: boolean;
}

export interface AssetPackStatus {
  id: string;
  required_version: string;
  installed_version: string | null;
  size: number;
  state: SimulatorAssetState;
  error_code: string | null;
}

export interface AssetStatus {
  state: SimulatorAssetState;
  profile_ids: string[];
  packs: AssetPackStatus[];
  update_available: boolean;
  error_code: string | null;
}

export interface DeviceStreamAddress {
  device_id: string;
  channel_id: string | null;
  stream: DeviceSimulatorStreamKind;
  url: string;
}

export interface DeviceIdentityPreview {
  device_id: string;
  group_id: string;
  profile_id: string;
  device_kind: DeviceSimulatorDeviceKind;
  ip: string;
  mac: string;
  serial_number: string;
  hardware_id: string;
  channel_count: number | null;
  streams: DeviceStreamAddress[];
}

export interface DevicePreview {
  total_devices: number;
  total_channels: number;
  devices: DeviceIdentityPreview[];
}

export interface PreflightCheck {
  id: string;
  severity: SimulatorCheckSeverity;
  status: SimulatorCheckStatus;
  message_key: string;
  details: string | null;
}

export type ConflictEvidenceKind = 'local' | 'neighbor' | 'probe' | 'unknown';
export type ConflictObservationResult = 'occupied' | 'available' | 'inconclusive';
export type ConflictVerdict = 'conflict' | 'clear' | 'unknown';

export interface ConflictEvidence {
  address: string;
  kind: ConflictEvidenceKind;
  result: ConflictObservationResult;
  details: string | null;
}

export interface AddressConflictAssessment {
  address: string;
  verdict: ConflictVerdict;
  strongest_evidence: ConflictEvidenceKind;
  evidence: ConflictEvidence[];
}

export interface PreflightReport {
  ok: boolean;
  checks: PreflightCheck[];
  device_preview: DevicePreview;
  address_assessments: AddressConflictAssessment[];
}

export interface SimulatorErrorInfo {
  code: string;
  message_key: string;
  details: string | null;
}

export interface SimulatorMetrics {
  total_devices: number;
  online_devices: number;
  total_channels: number;
  active_rtsp_clients: number;
  outbound_bitrate_kbps: number;
  active_alarm_jobs: number;
}

export interface SimulatorStatus {
  state: SimulatorSessionState;
  session_id: string | null;
  started_at: string | null;
  phase_progress: number | null;
  metrics: SimulatorMetrics;
  cleanup_stage: string | null;
  recovery_session_id: string | null;
  last_error: SimulatorErrorInfo | null;
}

export function shouldReleaseActiveAlarmJob(
  status: SimulatorStatus,
  alarmStartPending: boolean,
): boolean {
  return status.state !== 'running'
    || (status.metrics.active_alarm_jobs === 0 && !alarmStartPending);
}

export interface ImportedAlarmImage {
  image_id: string;
  file_name: string;
  extension: 'jpg' | 'jpeg' | 'png';
  size: number;
}

export interface AlarmJobRequest {
  target_device_ids: string[];
  alarm_profile_id: string;
  alarm_type_ids: string[];
  mode: AlarmDispatchMode;
  interval_ms: number;
  /** Null means that the job continues until explicitly stopped. */
  send_count: number | null;
  recovery_delay_secs: number | null;
  image_variant: string | null;
  user_image_id: string | null;
}

export interface AlarmTriggerResult {
  attempted: number;
  succeeded: number;
  failed: number;
  unverified: number;
  duration_ms: number;
  errors: SimulatorErrorInfo[];
}

export interface AlarmJobStats {
  job_id: string;
  state: SimulatorAlarmJobState;
  attempted: number;
  succeeded: number;
  failed: number;
  unverified: number;
  in_flight: number;
  last_http_status: number | null;
  average_duration_ms: number;
  last_error: SimulatorErrorInfo | null;
}

/**
 * Where alarms are currently delivered, and whether that came from a platform
 * subscription or from the configured fallback port.
 */
export interface AlarmSubscription {
  destinations: string[];
  learned: boolean;
  host: string | null;
  port: number | null;
  duration_secs: number | null;
  learned_at_ms: number | null;
  expires_at_ms: number | null;
  overridden: boolean;
}

export interface RecoveryResult {
  session_id: string;
  recovered: boolean;
  remaining_resources: string[];
  error: SimulatorErrorInfo | null;
}

export interface AssetProgress {
  job_id: string;
  state: SimulatorAssetState;
  current_pack_id: string | null;
  downloaded: number;
  total: number | null;
  speed_bps: number;
  error: SimulatorErrorInfo | null;
}

export interface DeviceRuntimeStatus {
  device_id: string;
  online: boolean;
  active_http_connections: number;
  active_rtsp_clients: number;
  last_error_code: string | null;
}

export interface DeviceStatusBatch {
  session_id: string;
  sequence: number;
  devices: DeviceRuntimeStatus[];
}

export interface RtspStats {
  session_id: string;
  active_clients: number;
  bitrate_kbps: number;
  bytes_sent: number;
  disconnected_clients: number;
}

export interface CleanupProgress {
  session_id: string;
  stage: string;
  completed: number;
  total: number;
  message_key: string;
  error: SimulatorErrorInfo | null;
}

export interface SimulatorLogEvent {
  timestamp: string;
  level: SimulatorLogLevel;
  session_id: string | null;
  component: string;
  profile_id: string | null;
  device_id: string | null;
  device_ip: string | null;
  channel_id: string | null;
  alarm_job_id: string | null;
  rtsp_session_id: string | null;
  error_code: string | null;
  message: string;
}

export const DEVICE_SIMULATOR_COMMANDS = {
  getSettings: 'device_simulator_get_settings',
  saveSettings: 'device_simulator_save_settings',
  listInterfaces: 'device_simulator_list_interfaces',
  listProfiles: 'device_simulator_list_profiles',
  listAlarmTypes: 'device_simulator_list_alarm_types',
  listMediaThemes: 'device_simulator_list_media_themes',
  getAssetStatus: 'device_simulator_get_asset_status',
  prepareAssets: 'device_simulator_prepare_assets',
  cancelAssetDownload: 'device_simulator_cancel_asset_download',
  previewDevices: 'device_simulator_preview_devices',
  preflight: 'device_simulator_preflight',
  start: 'device_simulator_start',
  stop: 'device_simulator_stop',
  getStatus: 'device_simulator_get_status',
  importAlarmImage: 'device_simulator_import_alarm_image',
  startAlarm: 'device_simulator_start_alarm',
  triggerAlarmOnce: 'device_simulator_trigger_alarm_once',
  stopAlarm: 'device_simulator_stop_alarm',
  recover: 'device_simulator_recover',
} as const;

export const DEVICE_SIMULATOR_EVENTS = {
  status: 'device-simulator-status',
  log: 'device-simulator-log',
  assetProgress: 'device-simulator-asset-progress',
  deviceStatus: 'device-simulator-device-status',
  rtspStats: 'device-simulator-rtsp-stats',
  alarmStats: 'device-simulator-alarm-stats',
  alarmSubscription: 'device-simulator-alarm-subscription',
  cleanupProgress: 'device-simulator-cleanup-progress',
} as const;

export interface DeviceSimulatorEventPayloads {
  [DEVICE_SIMULATOR_EVENTS.status]: SimulatorStatus;
  [DEVICE_SIMULATOR_EVENTS.log]: SimulatorLogEvent;
  [DEVICE_SIMULATOR_EVENTS.assetProgress]: AssetProgress;
  [DEVICE_SIMULATOR_EVENTS.deviceStatus]: DeviceStatusBatch;
  [DEVICE_SIMULATOR_EVENTS.rtspStats]: RtspStats;
  [DEVICE_SIMULATOR_EVENTS.alarmStats]: AlarmJobStats;
  [DEVICE_SIMULATOR_EVENTS.alarmSubscription]: AlarmSubscription;
  [DEVICE_SIMULATOR_EVENTS.cleanupProgress]: CleanupProgress;
}

export type DeviceSimulatorInvoke = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export interface DeviceSimulatorApi {
  getSettings(): Promise<DeviceSimulatorSettings>;
  saveSettings(settings: DeviceSimulatorSettings): Promise<DeviceSimulatorSettings>;
  listInterfaces(): Promise<SimulatorNetworkInterfaceInfo[]>;
  listProfiles(): Promise<DeviceProfileSummary[]>;
  listAlarmTypes(): Promise<ProfileAlarmTypes[]>;
  listMediaThemes(): Promise<MediaThemeSummary[]>;
  getAssetStatus(profileIds: string[]): Promise<AssetStatus>;
  prepareAssets(profileIds: string[]): Promise<string>;
  cancelAssetDownload(jobId: string): Promise<void>;
  previewDevices(request: SimulatorStartRequest): Promise<DevicePreview>;
  preflight(request: SimulatorStartRequest): Promise<PreflightReport>;
  start(request: SimulatorStartRequest): Promise<SimulatorStatus>;
  stop(): Promise<void>;
  getStatus(): Promise<SimulatorStatus>;
  importAlarmImage(): Promise<ImportedAlarmImage | null>;
  startAlarm(request: AlarmJobRequest): Promise<string>;
  triggerAlarmOnce(request: AlarmJobRequest): Promise<AlarmTriggerResult>;
  stopAlarm(jobId: string): Promise<void>;
  recover(sessionId: string): Promise<RecoveryResult>;
}

export function createDeviceSimulatorApi(invokeCommand: DeviceSimulatorInvoke): DeviceSimulatorApi {
  return {
    getSettings: () => invokeCommand(DEVICE_SIMULATOR_COMMANDS.getSettings),
    saveSettings: (settings) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.saveSettings, { settings }),
    listInterfaces: () => invokeCommand(DEVICE_SIMULATOR_COMMANDS.listInterfaces),
    listProfiles: () => invokeCommand(DEVICE_SIMULATOR_COMMANDS.listProfiles),
    listAlarmTypes: () => invokeCommand(DEVICE_SIMULATOR_COMMANDS.listAlarmTypes),
    listMediaThemes: () => invokeCommand(DEVICE_SIMULATOR_COMMANDS.listMediaThemes),
    getAssetStatus: (profileIds) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.getAssetStatus, { profileIds }),
    prepareAssets: (profileIds) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.prepareAssets, { profileIds }),
    cancelAssetDownload: (jobId) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.cancelAssetDownload, { jobId }),
    previewDevices: (request) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.previewDevices, { request }),
    preflight: (request) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.preflight, { request }),
    start: (request) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.start, { request }),
    stop: () => invokeCommand(DEVICE_SIMULATOR_COMMANDS.stop),
    getStatus: () => invokeCommand(DEVICE_SIMULATOR_COMMANDS.getStatus),
    importAlarmImage: () => invokeCommand(DEVICE_SIMULATOR_COMMANDS.importAlarmImage),
    startAlarm: (request) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.startAlarm, { request }),
    triggerAlarmOnce: (request) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.triggerAlarmOnce, { request }),
    stopAlarm: (jobId) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.stopAlarm, { jobId }),
    recover: (sessionId) => invokeCommand(DEVICE_SIMULATOR_COMMANDS.recover, { sessionId }),
  };
}

const tauriInvoke: DeviceSimulatorInvoke = (command, args) => invoke(command, args);

export const deviceSimulatorApi = createDeviceSimulatorApi(tauriInvoke);

const RUNTIME_ACTIVE_STATES = new Set<SimulatorSessionState>([
  'starting_worker',
  'adding_ips',
  'starting_services',
  'running',
  'stopping_alarms',
  'stopping_services',
  'removing_firewall',
  'removing_ips',
  'recovery_required',
  'recovering',
]);

const TOPOLOGY_EDITABLE_STATES = new Set<SimulatorSessionState>([
  'idle',
  'stopped',
  'failed',
]);

/** True while a Worker may be active or session-owned OS resources may remain. */
export function isDeviceSimulatorRuntimeActive(state: SimulatorSessionState): boolean {
  return RUNTIME_ACTIVE_STATES.has(state);
}

/** Topology edits are unsafe during validation, downloads, startup, cleanup, or recovery. */
export function isDeviceSimulatorTopologyLocked(state: SimulatorSessionState): boolean {
  return !TOPOLOGY_EDITABLE_STATES.has(state);
}

export function hasBlockingPreflightFailure(report: PreflightReport): boolean {
  return !report.ok || report.checks.some((check) => (
    check.severity === 'error' && check.status === 'failed'
  ));
}

/**
 * Backend alarm error codes that have a specific explanation, keyed by the
 * suffix under `deviceSimulator.alarmErrors`. Codes absent here still surface
 * verbatim in the UI next to the generic message — the raw code must never be
 * swallowed, because it is the only thing that identifies the fault.
 */
const ALARM_ERROR_MESSAGE_KEYS: Record<string, string> = {
  'device_simulator.alarm.transport_connect_failed': 'transportConnectFailed',
  'device_simulator.alarm.transport_timeout': 'transportTimeout',
  'device_simulator.alarm.transport_failed': 'transportFailed',
  'device_simulator.alarm.request_timeout': 'requestTimeout',
  'device_simulator.alarm.destination_unknown': 'destinationUnknown',
  'device_simulator.alarm.destination_url_invalid': 'destinationUrlInvalid',
  'device_simulator.alarm.destination_missing': 'destinationUnknown',
  'device_simulator.alarm.header_invalid': 'headerInvalid',
  'device_simulator.alarm.http_client_failed': 'httpClientFailed',
  'device_simulator.alarm.client_cache_poisoned': 'httpClientFailed',
  'device_simulator.alarm.cancelled': 'cancelled',
};

/** i18n key explaining a specific alarm failure, or null when only the generic message applies. */
export function alarmErrorMessageKey(code: string): string | null {
  if (code.startsWith('device_simulator.alarm.http_status.')) {
    return 'deviceSimulator.alarmErrors.httpStatus';
  }
  const suffix = ALARM_ERROR_MESSAGE_KEYS[code];
  return suffix ? `deviceSimulator.alarmErrors.${suffix}` : null;
}

/** HTTP status carried inside a `device_simulator.alarm.http_status.<n>` code. */
export function alarmErrorHttpStatus(code: string): string | null {
  const prefix = 'device_simulator.alarm.http_status.';
  return code.startsWith(prefix) ? code.slice(prefix.length) : null;
}

/** True once a learned subscription has passed the lifetime the platform declared. */
export function isAlarmSubscriptionExpired(
  subscription: AlarmSubscription,
  now = Date.now(),
): boolean {
  return subscription.expires_at_ms !== null && subscription.expires_at_ms <= now;
}
