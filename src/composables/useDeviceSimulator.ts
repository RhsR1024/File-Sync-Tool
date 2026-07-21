import { computed, reactive, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import {
  DEVICE_SIMULATOR_EVENTS,
  deviceSimulatorApi,
  hasBlockingPreflightFailure,
  isDeviceSimulatorTopologyLocked,
  type AlarmJobRequest,
  type AlarmJobStats,
  type ProfileAlarmTypes,
  type AlarmTriggerResult,
  type AssetProgress,
  type AssetStatus,
  type CleanupProgress,
  type DeviceGroupDraft,
  type DevicePreview,
  type DeviceProfileSummary,
  type DeviceSimulatorSettings,
  type DeviceStatusBatch,
  type ImportedAlarmImage,
  type PreflightReport,
  type RtspStats,
  type SimulatorLogEvent,
  type SimulatorNetworkInterfaceInfo,
  type SimulatorStartRequest,
  type SimulatorStatus,
} from '@/lib/deviceSimulator';
import { recommendSimulatorInterface } from '@/lib/deviceSimulatorInterfaceSelection';

function newId(prefix: string) {
  const suffix = typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `${prefix}-${suffix}`;
}

const emptyStatus = (): SimulatorStatus => ({
  state: 'idle',
  session_id: null,
  started_at: null,
  phase_progress: null,
  metrics: {
    total_devices: 0,
    online_devices: 0,
    total_channels: 0,
    active_rtsp_clients: 0,
    outbound_bitrate_kbps: 0,
    active_alarm_jobs: 0,
  },
  cleanup_stage: null,
  recovery_session_id: null,
  last_error: null,
});

const defaultSettings = (): DeviceSimulatorSettings => ({
  asset_server_url_override: null,
  selected_interface_id: null,
  last_platform: 'ums',
  last_start_ip: '192.168.1.100',
  last_device_ips: [],
  last_subnet_prefix: 24,
  last_platform_servers: [],
  last_alarm_receiver_url: null,
  last_device_groups: [{
    id: newId('group'),
    profile_id: 'ipc-custom',
    count: 1,
    nvr_channel_count: null,
  }],
  last_http_port: 81,
  last_rtsp_ports: { main: 554, sub: 555, third: 556 },
  auto_check_asset_updates: true,
  manage_firewall: true,
});

function requestFromSettings(settings: DeviceSimulatorSettings): SimulatorStartRequest {
  return {
    platform: {
      kind: 'ums',
      servers: settings.last_platform_servers.length > 0
        ? settings.last_platform_servers.map((server) => ({ ...server }))
        : [{ id: newId('server'), host: '', port: 80 }],
      alarm_receiver_url: settings.last_alarm_receiver_url,
    },
    interface_id: settings.selected_interface_id ?? '',
    start_ip: settings.last_start_ip ?? '192.168.1.100',
    device_ips: [...settings.last_device_ips],
    subnet_prefix: settings.last_subnet_prefix,
    device_http_port: settings.last_http_port,
    rtsp_ports: { ...settings.last_rtsp_ports },
    groups: settings.last_device_groups.length > 0
      ? settings.last_device_groups.map((group) => ({ ...group }))
      : defaultSettings().last_device_groups,
    stream: {
      transport: 'tcp_interleaved',
      enabled_streams: ['main', 'sub', 'third'],
      audio_enabled: false,
    },
  };
}

function createDeviceSimulator() {
  const settings = ref(defaultSettings());
  const request = reactive<SimulatorStartRequest>(requestFromSettings(settings.value));
  const status = ref<SimulatorStatus>(emptyStatus());
  const interfaces = ref<SimulatorNetworkInterfaceInfo[]>([]);
  const profiles = ref<DeviceProfileSummary[]>([]);
  const alarmTypes = ref<ProfileAlarmTypes[]>([]);
  const assets = ref<AssetStatus | null>(null);
  const assetProgress = ref<AssetProgress | null>(null);
  const preview = ref<DevicePreview | null>(null);
  const preflight = ref<PreflightReport | null>(null);
  const deviceStatus = ref<DeviceStatusBatch | null>(null);
  const rtspStats = ref<RtspStats | null>(null);
  const alarmStats = ref<AlarmJobStats | null>(null);
  const cleanupProgress = ref<CleanupProgress | null>(null);
  const lastAlarmResult = ref<AlarmTriggerResult | null>(null);
  const importedAlarmImage = ref<ImportedAlarmImage | null>(null);
  const logs = ref<SimulatorLogEvent[]>([]);
  const busyAction = ref<string | null>(null);
  const errorMessage = ref('');
  const initialized = ref(false);
  const manualInterfaceSelection = ref(false);
  let unlisteners: UnlistenFn[] = [];

  const topologyLocked = computed(() => isDeviceSimulatorTopologyLocked(status.value.state));
  const selectedProfileIds = computed(() => [...new Set(request.groups.map((group) => group.profile_id))]);
  const blockingPreflight = computed(() => preflight.value
    ? hasBlockingPreflightFailure(preflight.value)
    : true);
  const recoverySessionId = computed(() => status.value.recovery_session_id
    ?? (status.value.state === 'recovery_required' ? status.value.session_id : null));
  const interfaceSelection = computed(() => recommendSimulatorInterface(
    interfaces.value,
    request.start_ip,
    request.device_ips,
    request.interface_id,
  ));
  const selectedInterface = computed(() => interfaces.value
    .find((item) => item.id === request.interface_id) ?? null);

  function selectAvailableInterface() {
    if (topologyLocked.value) return;
    const recommendation = interfaceSelection.value;
    if (!recommendation.recommended_interface_id) {
      request.interface_id = '';
      return;
    }
    const currentIsAvailable = interfaces.value.some((item) => item.id === request.interface_id);
    if (!manualInterfaceSelection.value || !currentIsAvailable) {
      request.interface_id = recommendation.recommended_interface_id;
      if (!currentIsAvailable) manualInterfaceSelection.value = false;
    }
  }

  function selectInterfaceManually(interfaceId: string) {
    if (topologyLocked.value) return;
    if (!interfaceId) {
      manualInterfaceSelection.value = false;
      selectAvailableInterface();
      return;
    }
    if (!interfaces.value.some((item) => item.id === interfaceId)) return;
    request.interface_id = interfaceId;
    manualInterfaceSelection.value = true;
  }

  function applyAutomaticInterfaceSelection() {
    if (topologyLocked.value) return;
    manualInterfaceSelection.value = false;
    request.interface_id = interfaceSelection.value.recommended_interface_id;
  }

  watch(
    () => [
      request.start_ip,
      request.device_ips.join('|'),
      request.subnet_prefix,
      interfaces.value.map((item) => `${item.id}:${item.ipv4_addresses.join(',')}`).join('|'),
    ],
    () => selectAvailableInterface(),
  );

  function replaceRequest(next: SimulatorStartRequest) {
    request.platform = next.platform;
    request.interface_id = next.interface_id;
    request.start_ip = next.start_ip;
    request.device_ips = next.device_ips;
    request.subnet_prefix = next.subnet_prefix;
    request.device_http_port = next.device_http_port;
    request.rtsp_ports = next.rtsp_ports;
    request.groups = next.groups;
    request.stream = next.stream;
  }

  function settingsFromRequest(): DeviceSimulatorSettings {
    return {
      ...settings.value,
      selected_interface_id: request.interface_id || null,
      last_platform: request.platform.kind,
      last_start_ip: request.start_ip || null,
      last_device_ips: [...request.device_ips],
      last_subnet_prefix: request.subnet_prefix,
      last_platform_servers: request.platform.servers.map((server) => ({ ...server })),
      last_alarm_receiver_url: request.platform.alarm_receiver_url,
      last_device_groups: request.groups.map((group) => ({ ...group })),
      last_http_port: request.device_http_port,
      last_rtsp_ports: { ...request.rtsp_ports },
    };
  }

  function errorText(error: unknown): string {
    const info = errorInfo(error);
    if (info) {
      const summary = info.message_key || info.code;
      const detail = [info.code, info.details].filter(Boolean).join(' | ');
      return detail ? `${summary}\n${detail}` : summary;
    }
    if (error instanceof Error) return error.message;
    try {
      return JSON.stringify(error);
    } catch {
      // Fall through to the final string conversion.
    }
    return String(error);
  }

  function errorInfo(error: unknown): { code: string; message_key: string; details: string } | null {
    if (!error || typeof error !== 'object') return null;
    const candidate = error as { code?: unknown; message_key?: unknown; details?: unknown };
    const code = typeof candidate.code === 'string' ? candidate.code : '';
    const message_key = typeof candidate.message_key === 'string' ? candidate.message_key : '';
    const details = typeof candidate.details === 'string' ? candidate.details : '';
    return code || message_key || details ? { code, message_key, details } : null;
  }

  function appendErrorLog(action: string, error: unknown) {
    const info = errorInfo(error);
    const message = info?.details || info?.message_key || (error instanceof Error ? error.message : String(error));
    const errorCode = info?.code || null;
    const previous = logs.value[logs.value.length - 1];
    if (previous?.level === 'error' && previous.error_code === errorCode && previous.message === message) return;
    logs.value.push({
      timestamp: new Date().toISOString(),
      level: 'error',
      session_id: status.value.session_id,
      component: `ui:${action}`,
      profile_id: null,
      device_id: null,
      device_ip: null,
      channel_id: null,
      alarm_job_id: null,
      rtsp_session_id: null,
      error_code: errorCode,
      message,
    });
    if (logs.value.length > 2_000) logs.value.splice(0, logs.value.length - 2_000);
  }

  async function run<T>(action: string, operation: () => Promise<T>): Promise<T | null> {
    busyAction.value = action;
    errorMessage.value = '';
    try {
      return await operation();
    } catch (error) {
      errorMessage.value = errorText(error);
      appendErrorLog(action, error);
      return null;
    } finally {
      busyAction.value = null;
    }
  }

  async function subscribeEvents() {
    if (unlisteners.length > 0) return;
    const listeners = await Promise.all([
      listen<SimulatorStatus>(DEVICE_SIMULATOR_EVENTS.status, ({ payload }) => {
        status.value = payload;
        if (payload.last_error) appendErrorLog('backend', payload.last_error);
      }),
      listen<AssetProgress>(DEVICE_SIMULATOR_EVENTS.assetProgress, ({ payload }) => {
        assetProgress.value = payload;
        if (payload.state === 'ready' || payload.state === 'failed') {
          void deviceSimulatorApi.getAssetStatus(selectedProfileIds.value)
            .then(async (status) => {
              assets.value = status;
              if (payload.state === 'ready') {
                await refreshAlarmTypes();
              } else {
                alarmTypes.value = [];
              }
            })
            .catch(() => undefined);
        }
      }),
      listen<DeviceStatusBatch>(DEVICE_SIMULATOR_EVENTS.deviceStatus, ({ payload }) => { deviceStatus.value = payload; }),
      listen<RtspStats>(DEVICE_SIMULATOR_EVENTS.rtspStats, ({ payload }) => { rtspStats.value = payload; }),
      listen<AlarmJobStats>(DEVICE_SIMULATOR_EVENTS.alarmStats, ({ payload }) => { alarmStats.value = payload; }),
      listen<CleanupProgress>(DEVICE_SIMULATOR_EVENTS.cleanupProgress, ({ payload }) => { cleanupProgress.value = payload; }),
      listen<SimulatorLogEvent>(DEVICE_SIMULATOR_EVENTS.log, ({ payload }) => {
        logs.value.push(payload);
        if (logs.value.length > 2_000) logs.value.splice(0, logs.value.length - 2_000);
      }),
    ]);
    unlisteners = listeners;
  }

  async function initialize() {
    if (initialized.value) return;
    await subscribeEvents();
    busyAction.value = 'initialize';
    errorMessage.value = '';
    const [settingsResult, interfaceResult, profileResult, statusResult] = await Promise.allSettled([
      deviceSimulatorApi.getSettings(),
      deviceSimulatorApi.listInterfaces(),
      deviceSimulatorApi.listProfiles(),
      deviceSimulatorApi.getStatus(),
    ]);
    if (settingsResult.status === 'fulfilled') {
      settings.value = settingsResult.value;
      replaceRequest(requestFromSettings(settingsResult.value));
    }
    if (interfaceResult.status === 'fulfilled') {
      interfaces.value = interfaceResult.value;
      selectAvailableInterface();
    }
    if (profileResult.status === 'fulfilled') profiles.value = profileResult.value;
    if (statusResult.status === 'fulfilled') status.value = statusResult.value;
    const failure = [settingsResult, interfaceResult, profileResult, statusResult]
      .find((result) => result.status === 'rejected');
    if (failure?.status === 'rejected') errorMessage.value = errorText(failure.reason);
    busyAction.value = null;
    initialized.value = true;
    if (selectedProfileIds.value.length > 0) await refreshAssets();
  }

  async function refreshEnvironment() {
    busyAction.value = 'refresh';
    errorMessage.value = '';
    const [interfaceResult, profileResult, statusResult] = await Promise.allSettled([
      deviceSimulatorApi.listInterfaces(),
      deviceSimulatorApi.listProfiles(),
      deviceSimulatorApi.getStatus(),
    ]);
    if (interfaceResult.status === 'fulfilled') {
      interfaces.value = interfaceResult.value;
      selectAvailableInterface();
    }
    if (profileResult.status === 'fulfilled') profiles.value = profileResult.value;
    if (statusResult.status === 'fulfilled') status.value = statusResult.value;
    const failure = [interfaceResult, profileResult, statusResult]
      .find((result) => result.status === 'rejected');
    if (failure?.status === 'rejected') errorMessage.value = errorText(failure.reason);
    busyAction.value = null;
    if (selectedProfileIds.value.length > 0) await refreshAssets();
  }

  function dispose() {
    for (const unlisten of unlisteners) unlisten();
    unlisteners = [];
    initialized.value = false;
  }

  async function saveSettings() {
    const next = settingsFromRequest();
    const saved = await run('save-settings', () => deviceSimulatorApi.saveSettings(next));
    if (saved) settings.value = saved;
  }

  async function refreshAssets() {
    const result = await run('check-assets', () => deviceSimulatorApi.getAssetStatus(selectedProfileIds.value));
    if (result) {
      assets.value = result;
      if (result.state === 'ready' || result.state === 'update_available') {
        try {
          alarmTypes.value = await deviceSimulatorApi.listAlarmTypes();
        } catch {
          // Alarm names are an optional convenience; sending all types still works.
          alarmTypes.value = [];
        }
      } else {
        alarmTypes.value = [];
      }
    }
  }

  async function prepareAssets() {
    const jobId = await run('prepare-assets', () => deviceSimulatorApi.prepareAssets(selectedProfileIds.value));
    if (jobId) {
      assetProgress.value = {
        job_id: jobId,
        state: 'checking',
        current_pack_id: null,
        downloaded: 0,
        total: null,
        speed_bps: 0,
        error: null,
      };
    }
  }

  async function refreshAlarmTypes() {
    try {
      alarmTypes.value = await deviceSimulatorApi.listAlarmTypes();
    } catch {
      alarmTypes.value = [];
    }
  }

  async function cancelAssetDownload() {
    if (!assetProgress.value?.job_id) return;
    await run('cancel-assets', () => deviceSimulatorApi.cancelAssetDownload(assetProgress.value!.job_id));
  }

  async function previewDevices() {
    const result = await run('preview', () => deviceSimulatorApi.previewDevices(request));
    if (result) preview.value = result;
  }

  async function runPreflight() {
    const result = await run('preflight', () => deviceSimulatorApi.preflight(request));
    if (result) {
      preflight.value = result;
      preview.value = result.device_preview;
    }
  }

  async function start() {
    const result = await run('start', async () => {
      const report = await deviceSimulatorApi.preflight(request);
      preflight.value = report;
      preview.value = report.device_preview;
      if (hasBlockingPreflightFailure(report)) throw new Error('deviceSimulator.errors.preflightBlocked');
      const saved = await deviceSimulatorApi.saveSettings(settingsFromRequest());
      settings.value = saved;
      return deviceSimulatorApi.start(request);
    });
    if (result) status.value = result;
  }

  async function stop() {
    const stopped = await run('stop', () => deviceSimulatorApi.stop());
    if (stopped === null) return;
    const next = await run('status', () => deviceSimulatorApi.getStatus());
    if (next) status.value = next;
  }

  async function recover() {
    if (!recoverySessionId.value) return;
    const recovered = await run('recover', async () => {
      const result = await deviceSimulatorApi.recover(recoverySessionId.value!);
      if (!result.recovered) {
        throw result.error ?? new Error('deviceSimulator.errors.recoveryFailed');
      }
      return result;
    });
    if (!recovered) return;
    const next = await run('status', () => deviceSimulatorApi.getStatus());
    if (next) status.value = next;
  }

  async function triggerAlarm(alarm: AlarmJobRequest) {
    const result = await run('trigger-alarm', () => deviceSimulatorApi.triggerAlarmOnce(alarm));
    if (result) lastAlarmResult.value = result;
  }

  async function importAlarmImage() {
    const result = await run('import-alarm-image', () => deviceSimulatorApi.importAlarmImage());
    if (result) importedAlarmImage.value = result;
    return result;
  }

  function clearAlarmImageSelection() {
    importedAlarmImage.value = null;
  }

  async function startAlarm(alarm: AlarmJobRequest) {
    await run('start-alarm', () => deviceSimulatorApi.startAlarm(alarm));
  }

  async function stopAlarm() {
    if (!alarmStats.value?.job_id) return;
    await run('stop-alarm', () => deviceSimulatorApi.stopAlarm(alarmStats.value!.job_id));
  }

  function addGroup(profileId = 'ipc-custom') {
    if (topologyLocked.value) return;
    request.groups.push({
      id: newId('group'),
      profile_id: profileId,
      count: 1,
      nvr_channel_count: profileId.startsWith('nvr-') ? 8 : null,
    });
  }

  function removeGroup(groupId: string) {
    if (topologyLocked.value || request.groups.length <= 1) return;
    request.groups = request.groups.filter((group) => group.id !== groupId);
  }

  function updateGroupProfile(group: DeviceGroupDraft, profileId: string) {
    group.profile_id = profileId;
    group.nvr_channel_count = profileId.startsWith('nvr-') ? (group.nvr_channel_count ?? 8) : null;
  }

  return {
    settings,
    request,
    status,
    interfaces,
    profiles,
    alarmTypes,
    assets,
    assetProgress,
    preview,
    preflight,
    deviceStatus,
    rtspStats,
    alarmStats,
    cleanupProgress,
    lastAlarmResult,
    importedAlarmImage,
    logs,
    busyAction,
    errorMessage,
    topologyLocked,
    blockingPreflight,
    recoverySessionId,
    selectedProfileIds,
    selectedInterface,
    interfaceSelection,
    manualInterfaceSelection,
    initialize,
    refreshEnvironment,
    dispose,
    saveSettings,
    refreshAssets,
    refreshAlarmTypes,
    prepareAssets,
    cancelAssetDownload,
    previewDevices,
    runPreflight,
    start,
    stop,
    recover,
    importAlarmImage,
    clearAlarmImageSelection,
    triggerAlarm,
    startAlarm,
    stopAlarm,
    addGroup,
    removeGroup,
    updateGroupProfile,
    selectInterfaceManually,
    applyAutomaticInterfaceSelection,
  };
}

let sharedDeviceSimulator: ReturnType<typeof createDeviceSimulator> | null = null;

/** Keep drafts, asset readiness, and background progress alive across route changes. */
export function useDeviceSimulator() {
  sharedDeviceSimulator ??= createDeviceSimulator();
  return sharedDeviceSimulator;
}
