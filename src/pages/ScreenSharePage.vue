<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  MonitorUp,
  Copy,
  QrCode,
  Users,
  Gauge,
  ArrowUpFromLine,
  Clock,
  Play,
  Power,
  ChevronDown,
  ChevronUp,
  ExternalLink,
  RefreshCw,
  MonitorOff,
  Eye,
  Eraser,
  Undo2,
  PencilRuler,
  MousePointer2,
  ShieldCheck,
  ShieldX,
  Layers3,
  Trash2,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  ArrowDown,
  X,
  Check,
} from 'lucide-vue-next';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import QRCode from 'qrcode';
import { LAN_SHARE_STATUS_REFRESH_INTERVAL_MS } from '../lib/lanShareStatus';
import {
  screenShareListMonitors,
  screenShareListInterfaces,
  screenShareStart,
  screenShareStop,
  screenShareGetStatus,
  screenShareClearAnnotations,
  screenShareGetAnnotationState,
  screenShareRemoveAnnotation,
  screenShareUpdateAnnotation,
  screenShareRespondControlRequest,
  screenShareRevokeControl,
  screenShareOpenLocalPreview,
  screenShareOpenDesktopOverlay,
  screenShareCloseDesktopOverlay,
  type MonitorInfo,
  type NetworkInterfaceInfo,
  type ScreenShareBackendMode,
  type ScreenShareMediaTransport,
  type ScreenShareConfig,
  type ScreenShareControlRequest,
  type ScreenShareAnnotationDocument,
  type ScreenShareAnnotationShape,
  type ScreenShareMediaMetrics,
  type ScreenShareStatus,
} from '../lib/tauri';
import { pushToast } from '../composables/useToast';

defineOptions({ name: 'ScreenSharePage' });

const { t } = useI18n();
const AUTO_REFRESH_SECONDS = Math.round(LAN_SHARE_STATUS_REFRESH_INTERVAL_MS / 1000);

const monitors = ref<MonitorInfo[]>([]);
const interfaces = ref<NetworkInterfaceInfo[]>([]);
const selectedMonitor = ref(0);
const selectedBindAddress = ref('0.0.0.0');
const port = ref(9870);
const usernameEnabled = ref(false);
const username = ref('');
const passwordEnabled = ref(false);
const password = ref('');
const quality = ref(70);
const fps = ref(15);
const showCursor = ref(true);
const backendMode = ref<ScreenShareBackendMode>('auto');
const mediaTransport = ref<ScreenShareMediaTransport>('auto');
const autoStart = ref(false);
const controlRequestsEnabled = ref(false);
const keyboardControlEnabled = ref(false);

const isActive = ref(false);
const isStarting = ref(false);
const serverUrl = ref('');
const qrForUrl = ref<string | null>(null);
const copiedUrl = ref<string | null>(null);
const showAllConnIps = ref(false);
const qrCanvases = ref<Record<string, HTMLCanvasElement | null>>({});
const setQrCanvas = (url: string, el: unknown) => {
  qrCanvases.value[url] = (el as HTMLCanvasElement | null) ?? null;
};
const errorMsg = ref('');
const isRefreshingStatus = ref(false);
const isOpeningPreview = ref(false);
const isTogglingDesktopOverlay = ref(false);
const isClearingAnnotations = ref(false);
const annotationActionId = ref<string | null>(null);
const isRespondingControl = ref(false);
const isRevokingControl = ref(false);

function emptyMediaMetrics(): ScreenShareMediaMetrics {
  return {
    encoded_frame_count: 0,
    jpeg_sample_count: 0,
    jpeg_size_avg_bytes: 0,
    jpeg_size_p50_bytes: 0,
    jpeg_size_p95_bytes: 0,
    first_frame_delay_ms: null,
    frame_age_ms: null,
    slow_client_dropped_frames: 0,
    stream_connection_count: 0,
    stream_first_frame_sample_count: 0,
    stream_first_frame_avg_ms: null,
    stream_first_frame_p95_ms: null,
    stream_reconnect_count: 0,
    stream_reconnect_sample_count: 0,
    stream_reconnect_avg_ms: null,
    stream_reconnect_p95_ms: null,
    fps_actual: 0,
    bitrate_kbps: 0,
  };
}

const status = ref<ScreenShareStatus>({
  is_active: false,
  viewer_count: 0,
  connection_count: 0,
  fps_actual: 0,
  bitrate_kbps: 0,
  uptime_secs: 0,
  server_url: '',
  all_urls: [],
  connected_ips: [],
  capture_paused: false,
  capture_issue: null,
  interaction_connected_count: 0,
  annotation_count: 0,
  view_mode: 'live',
  source_epoch: 0,
  latest_frame_id: null,
  frame_width: null,
  frame_height: null,
  transport: 'mjpeg',
  control_state: 'disabled',
  controller_ip: null,
  pending_control_request: null,
  desktop_overlay_active: false,
  media_metrics: emptyMediaMetrics(),
});

const annotationDocument = ref<ScreenShareAnnotationDocument>({
  session_id: 0,
  source_epoch: 0,
  revision: 0,
  mode: 'live',
  frozen_frame_id: null,
  shapes: [],
});
const hostAnnotationColors = ['#f59e0b', '#ef4444', '#22c55e', '#38bdf8', '#f8fafc'] as const;
const hostAnnotationWidths = [2, 4, 7] as const;
const persistentAnnotations = computed(() => annotationDocument.value.shapes.filter((shape) => shape.kind !== 'laser'));
const latestPersistentAnnotation = computed<ScreenShareAnnotationShape | null>(() => (
  persistentAnnotations.value[persistentAnnotations.value.length - 1] ?? null
));

const logs = ref<{ level: string; message: string; time: string }[]>([]);
let lastUptimeUpdate = 0;

const monitorOptions = computed(() =>
  monitors.value.map((monitor) => ({
    value: monitor.index,
    label: `${monitor.name} (${monitor.width}x${monitor.height}${monitor.is_primary ? `, ${t('tools.screenShare.monitorPrimary')}` : ''})`,
  })),
);

const formattedUptime = computed(() => {
  const seconds = status.value.uptime_secs;
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  return `${String(hours).padStart(2, '0')}:${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}`;
});

const formattedBitrate = computed(() => {
  const kbps = status.value.bitrate_kbps;
  if (kbps >= 1024) {
    return `${(kbps / 1024).toFixed(1)} Mbps`;
  }
  return `${kbps} Kbps`;
});

const allUrls = computed(() => {
  const seen = new Set<string>();
  const ordered: string[] = [];
  for (const url of [serverUrl.value, ...(status.value.all_urls || [])]) {
    if (!url || seen.has(url)) continue;
    seen.add(url);
    ordered.push(url);
  }
  return ordered;
});
const connectedIps = computed(() => status.value.connected_ips || []);
const connectionCount = computed(() => status.value.connection_count ?? connectedIps.value.length);
const privacyModeDetected = computed(() => status.value.capture_issue === 'privacy_mode_or_display_off');
const visibleConnIps = computed(() => showAllConnIps.value ? connectedIps.value : connectedIps.value.slice(0, 10));
const hiddenConnIpCount = computed(() => Math.max(0, connectedIps.value.length - 10));

async function loadMonitors() {
  try {
    monitors.value = await screenShareListMonitors();
    if (monitors.value.length > 0) {
      const primaryIndex = monitors.value.findIndex((monitor) => monitor.is_primary);
      selectedMonitor.value = primaryIndex >= 0 ? primaryIndex : 0;
    }
  } catch {
    errorMsg.value = t('tools.screenShare.errNoMonitor');
  }
}

async function loadInterfaces() {
  try {
    interfaces.value = await screenShareListInterfaces();
  } catch {
    /* Ignore optional interface listing failures. */
  }
}

const KV_KEY = 'screen_share_settings';

interface SavedSettings {
  port: number;
  usernameEnabled: boolean;
  username: string;
  passwordEnabled: boolean;
  password: string;
  quality: number;
  fps: number;
  showCursor: boolean;
  backendMode: ScreenShareBackendMode;
  mediaTransport?: ScreenShareMediaTransport;
  selectedMonitor: number;
  selectedBindAddress: string;
  autoStart: boolean;
  controlRequestsEnabled?: boolean;
  keyboardControlEnabled?: boolean;
}

async function saveSettings() {
  try {
    await invoke('save_kv', {
      key: KV_KEY,
      value: {
        port: port.value,
        usernameEnabled: usernameEnabled.value,
        username: username.value,
        passwordEnabled: passwordEnabled.value,
        password: password.value,
        quality: quality.value,
        fps: fps.value,
        showCursor: showCursor.value,
        backendMode: backendMode.value,
        mediaTransport: mediaTransport.value,
        selectedMonitor: selectedMonitor.value,
        selectedBindAddress: selectedBindAddress.value,
        autoStart: autoStart.value,
        controlRequestsEnabled: controlRequestsEnabled.value,
        keyboardControlEnabled: keyboardControlEnabled.value,
      } satisfies SavedSettings,
    });
  } catch {
    /* Persisting settings is best-effort for this page. */
  }
}

async function loadSettings() {
  try {
    const saved = await invoke<SavedSettings | null>('load_kv', { key: KV_KEY });
    if (!saved) {
      return;
    }
    port.value = saved.port ?? 9870;
    usernameEnabled.value = saved.usernameEnabled ?? false;
    username.value = saved.username ?? '';
    passwordEnabled.value = saved.passwordEnabled ?? false;
    password.value = saved.password ?? '';
    quality.value = saved.quality ?? 70;
    fps.value = saved.fps ?? 15;
    showCursor.value = saved.showCursor ?? true;
    backendMode.value = saved.backendMode ?? 'auto';
    mediaTransport.value = saved.mediaTransport ?? 'auto';
    selectedMonitor.value = saved.selectedMonitor ?? 0;
    selectedBindAddress.value = saved.selectedBindAddress ?? '0.0.0.0';
    autoStart.value = saved.autoStart ?? false;
    controlRequestsEnabled.value = saved.controlRequestsEnabled ?? false;
    keyboardControlEnabled.value = Boolean(
      controlRequestsEnabled.value && saved.keyboardControlEnabled,
    );
  } catch {
    /* Ignore malformed legacy saved settings. */
  }
}

function buildScreenShareConfig(): ScreenShareConfig {
  return {
    port: port.value,
    username: usernameEnabled.value && username.value ? username.value : null,
    password: passwordEnabled.value && password.value ? password.value : null,
    monitor_index: selectedMonitor.value,
    quality: quality.value,
    fps: fps.value,
    show_cursor: showCursor.value,
    capture_backend_mode: backendMode.value,
    bind_address: selectedBindAddress.value || '0.0.0.0',
    control_requests_enabled: controlRequestsEnabled.value,
    keyboard_control_enabled: controlRequestsEnabled.value && keyboardControlEnabled.value,
    annotations_enabled: true,
    shared_freeze_enabled: true,
    transport: mediaTransport.value,
  };
}

const backendModeOptions = computed(() => [
  {
    value: 'auto' as const,
    label: t('tools.screenShare.backendModeAuto'),
    description: t('tools.screenShare.backendModeAutoDesc'),
  },
  {
    value: 'wgc' as const,
    label: t('tools.screenShare.backendModeWgc'),
    description: t('tools.screenShare.backendModeWgcDesc'),
  },
  {
    value: 'dxgi' as const,
    label: t('tools.screenShare.backendModeDxgi'),
    description: t('tools.screenShare.backendModeDxgiDesc'),
  },
]);

const mediaTransportOptions = computed(() => [
  {
    value: 'auto' as const,
    label: t('tools.screenShare.mediaTransportAuto'),
    description: t('tools.screenShare.mediaTransportAutoDesc'),
  },
  {
    value: 'mse_h264' as const,
    label: t('tools.screenShare.mediaTransportH264'),
    description: t('tools.screenShare.mediaTransportH264Desc'),
  },
  {
    value: 'mjpeg' as const,
    label: t('tools.screenShare.mediaTransportMjpeg'),
    description: t('tools.screenShare.mediaTransportMjpegDesc'),
  },
]);

const activeTransportLabel = computed(() => (
  status.value.transport === 'mse_h264'
    ? t('tools.screenShare.mediaTransportH264')
    : t('tools.screenShare.mediaTransportMjpeg')
));

watch(controlRequestsEnabled, (enabled) => {
  if (!enabled) keyboardControlEnabled.value = false;
});

async function applyStartedShare(url: string) {
  serverUrl.value = url;
  isActive.value = true;
  showAllConnIps.value = false;
  await refreshStatus(true);
  await saveSettings();
}

async function startShare() {
  errorMsg.value = '';
  isStarting.value = true;
  try {
    const config = buildScreenShareConfig();
    const url = await screenShareStart(config);
    await applyStartedShare(url);
  } catch (error) {
    errorMsg.value = t('tools.screenShare.errStartFailed', { error: String(error) });
  } finally {
    isStarting.value = false;
  }
}

async function stopShare() {
  try {
    await screenShareStop();
  } catch {
    /* Ignore stop errors while resetting local state. */
  }
  isActive.value = false;
  serverUrl.value = '';
  qrForUrl.value = null;
  showAllConnIps.value = false;
  status.value = {
    is_active: false,
    viewer_count: 0,
    connection_count: 0,
    fps_actual: 0,
    bitrate_kbps: 0,
    uptime_secs: 0,
    server_url: '',
    all_urls: [],
    connected_ips: [],
    capture_paused: false,
    capture_issue: null,
    interaction_connected_count: 0,
    annotation_count: 0,
    view_mode: 'live',
    source_epoch: 0,
    latest_frame_id: null,
    frame_width: null,
    frame_height: null,
    transport: 'mjpeg',
    control_state: 'disabled',
    controller_ip: null,
    pending_control_request: null,
    desktop_overlay_active: false,
    media_metrics: emptyMediaMetrics(),
  };
}

async function respondControlRequest(allow: boolean) {
  const request = status.value.pending_control_request;
  if (!request || isRespondingControl.value) return;
  isRespondingControl.value = true;
  errorMsg.value = '';
  try {
    await screenShareRespondControlRequest(request.request_id, allow);
    status.value.pending_control_request = null;
    status.value.control_state = allow ? 'granted' : 'available';
    await refreshStatus(true);
  } catch (error) {
    errorMsg.value = t('tools.screenShare.errControlResponseFailed', { error: String(error) });
  } finally {
    isRespondingControl.value = false;
  }
}

async function revokeControl() {
  if (isRevokingControl.value) return;
  isRevokingControl.value = true;
  errorMsg.value = '';
  try {
    await screenShareRevokeControl();
    await refreshStatus(true);
  } catch (error) {
    errorMsg.value = t('tools.screenShare.errRevokeControlFailed', { error: String(error) });
  } finally {
    isRevokingControl.value = false;
  }
}

async function openLocalPreview() {
  isOpeningPreview.value = true;
  errorMsg.value = '';
  try {
    await screenShareOpenLocalPreview();
  } catch (error) {
    errorMsg.value = t('tools.screenShare.errPreviewFailed', { error: String(error) });
  } finally {
    isOpeningPreview.value = false;
  }
}

async function toggleDesktopOverlay() {
  if (isTogglingDesktopOverlay.value) return;
  isTogglingDesktopOverlay.value = true;
  errorMsg.value = '';
  try {
    if (status.value.desktop_overlay_active) {
      await screenShareCloseDesktopOverlay();
    } else {
      await screenShareOpenDesktopOverlay();
    }
    await refreshStatus(true);
  } catch (error) {
    errorMsg.value = t('tools.screenShare.errDesktopOverlayFailed', { error: String(error) });
  } finally {
    isTogglingDesktopOverlay.value = false;
  }
}

async function clearAllAnnotations() {
  if (isClearingAnnotations.value || annotationActionId.value) return;
  isClearingAnnotations.value = true;
  errorMsg.value = '';
  try {
    await screenShareClearAnnotations();
    await Promise.all([refreshStatus(true), refreshAnnotationState()]);
    pushToast(t('tools.screenShare.annotationsCleared'), 'success', { ttlMs: 1600 });
  } catch (error) {
    errorMsg.value = t('tools.screenShare.errClearAnnotationsFailed', { error: String(error) });
  } finally {
    isClearingAnnotations.value = false;
  }
}

async function undoLatestHostAnnotation() {
  const latest = latestPersistentAnnotation.value;
  if (!latest || annotationActionId.value || isClearingAnnotations.value) return;
  await removeHostAnnotation(latest);
}

function applyAnnotationDocument(payload: ScreenShareAnnotationDocument) {
  const current = annotationDocument.value;
  if (payload.session_id === current.session_id
    && (payload.source_epoch < current.source_epoch
      || (payload.source_epoch === current.source_epoch && payload.revision < current.revision))) {
    return;
  }
  annotationDocument.value = payload;
}

async function refreshAnnotationState(silent = true) {
  if (!isActive.value) {
    annotationDocument.value = {
      session_id: 0,
      source_epoch: 0,
      revision: 0,
      mode: 'live',
      frozen_frame_id: null,
      shapes: [],
    };
    return;
  }
  try {
    applyAnnotationDocument(await screenShareGetAnnotationState());
  } catch (error) {
    if (!silent) errorMsg.value = String(error);
  }
}

function annotationTypeLabel(shape: ScreenShareAnnotationShape): string {
  if (shape.kind === 'arrow') return t('tools.screenShare.annotationArrow');
  if (shape.kind === 'rect') return t('tools.screenShare.annotationRectangle');
  return t('tools.screenShare.annotationLaser');
}

function annotationOwnerLabel(shape: ScreenShareAnnotationShape): string {
  return shape.owner_client_id.length > 10
    ? `${shape.owner_client_id.slice(0, 8)}…`
    : shape.owner_client_id;
}

async function updateHostAnnotation(
  shape: ScreenShareAnnotationShape,
  color = shape.color,
  width = shape.width,
  points = shape.points,
) {
  if (annotationActionId.value) return;
  annotationActionId.value = shape.id;
  errorMsg.value = '';
  try {
    await screenShareUpdateAnnotation(shape.id, points, color, width);
    await refreshAnnotationState();
  } catch (error) {
    errorMsg.value = t('tools.screenShare.errUpdateAnnotationFailed', { error: String(error) });
  } finally {
    annotationActionId.value = null;
  }
}

async function removeHostAnnotation(shape: ScreenShareAnnotationShape) {
  if (annotationActionId.value) return;
  annotationActionId.value = shape.id;
  errorMsg.value = '';
  try {
    await screenShareRemoveAnnotation(shape.id);
    await refreshAnnotationState();
    pushToast(t('tools.screenShare.annotationRemoved'), 'success', { ttlMs: 1400 });
  } catch (error) {
    errorMsg.value = t('tools.screenShare.errRemoveAnnotationFailed', { error: String(error) });
  } finally {
    annotationActionId.value = null;
  }
}

async function nudgeHostAnnotation(shape: ScreenShareAnnotationShape, deltaX: number, deltaY: number) {
  const minX = Math.min(...shape.points.map((point) => point.x));
  const maxX = Math.max(...shape.points.map((point) => point.x));
  const minY = Math.min(...shape.points.map((point) => point.y));
  const maxY = Math.max(...shape.points.map((point) => point.y));
  const boundedX = Math.min(1 - maxX, Math.max(-minX, deltaX));
  const boundedY = Math.min(1 - maxY, Math.max(-minY, deltaY));
  const points = shape.points.map((point) => ({
    x: Math.min(1, Math.max(0, point.x + boundedX)),
    y: Math.min(1, Math.max(0, point.y + boundedY)),
  }));
  await updateHostAnnotation(shape, shape.color, shape.width, points);
}

function changeHostAnnotationWidth(shape: ScreenShareAnnotationShape, event: Event) {
  const nextWidth = Number((event.target as HTMLSelectElement).value);
  if (Number.isFinite(nextWidth)) void updateHostAnnotation(shape, shape.color, nextWidth);
}

async function copyUrl(url: string) {
  try {
    await navigator.clipboard.writeText(url);
    copiedUrl.value = url;
    pushToast(t('tools.screenShare.copyUrl'), 'success', { ttlMs: 1600 });
    setTimeout(() => {
      if (copiedUrl.value === url) copiedUrl.value = null;
    }, 1800);
  } catch (error) {
    pushToast(String(error), 'error', { ttlMs: 3200 });
  }
}

async function openInBrowser(url?: string) {
  const target = url ?? serverUrl.value;
  if (!target) {
    return;
  }
  try {
    await invoke('open_url', { url: target });
  } catch {
    /* Opening the system browser is best-effort. */
  }
}

async function toggleQr(url: string) {
  qrForUrl.value = qrForUrl.value === url ? null : url;
  if (qrForUrl.value) {
    await nextTick();
    const canvas = qrCanvases.value[url];
    if (canvas) {
      await QRCode.toCanvas(canvas, url, {
        width: 128,
        margin: 1,
        color: { dark: '#1e293b', light: '#ffffff' },
      });
    }
  }
}

function addLog(level: string, message: string) {
  const now = new Date();
  const time = `${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}:${String(now.getSeconds()).padStart(2, '0')}`;
  logs.value.unshift({ level, message, time });
  if (logs.value.length > 50) {
    logs.value.length = 50;
  }
}

watch(serverUrl, () => {
  qrForUrl.value = null;
});

let unlistenStatus: UnlistenFn | null = null;
let unlistenLog: UnlistenFn | null = null;
let unlistenControlRequest: UnlistenFn | null = null;
let unlistenDesktopOverlayError: UnlistenFn | null = null;
let unlistenAnnotationState: UnlistenFn | null = null;
let statusRefreshTimer: ReturnType<typeof setInterval> | null = null;

function applyStatus(payload: ScreenShareStatus) {
  if (payload.is_active && payload.uptime_secs < lastUptimeUpdate && lastUptimeUpdate - payload.uptime_secs > 1) {
    return;
  }
  if (payload.is_active) {
    lastUptimeUpdate = payload.uptime_secs;
  }
  status.value = {
    ...payload,
    media_metrics: payload.media_metrics ?? emptyMediaMetrics(),
  };
  if (payload.is_active && !isActive.value) {
    isActive.value = true;
    serverUrl.value = payload.server_url;
  }
  if (!payload.is_active) {
    isActive.value = false;
    serverUrl.value = '';
    showAllConnIps.value = false;
    lastUptimeUpdate = 0;
    void refreshAnnotationState();
  }
}

async function refreshStatus(silent = false) {
  if (!silent) {
    isRefreshingStatus.value = true;
  }
  try {
    applyStatus(await screenShareGetStatus());
    await refreshAnnotationState();
  } catch (error) {
    if (!silent) {
      errorMsg.value = String(error);
    }
  } finally {
    if (!silent) {
      isRefreshingStatus.value = false;
    }
  }
}

function startStatusPolling() {
  if (statusRefreshTimer) {
    clearInterval(statusRefreshTimer);
  }
  statusRefreshTimer = setInterval(() => {
    if (isActive.value) {
      void refreshStatus(true);
    }
  }, LAN_SHARE_STATUS_REFRESH_INTERVAL_MS);
}

onMounted(async () => {
  await loadSettings();
  await loadMonitors();
  await loadInterfaces();

  try {
    await refreshStatus(true);
  } catch {
    /* Ignore status probe failures during page bootstrap. */
  }

  if (autoStart.value && !isActive.value && monitors.value.length > 0) {
    await startShare();
  }

  unlistenStatus = await listen<ScreenShareStatus>('screen-share-status', (event) => {
    applyStatus(event.payload);
  });

  unlistenLog = await listen<{ level: string; message: string }>('screen-share-log', (event) => {
    addLog(event.payload.level, event.payload.message);
  });
  unlistenControlRequest = await listen<ScreenShareControlRequest>('screen-share-control-request', (event) => {
    if (!isActive.value) return;
    status.value.pending_control_request = event.payload;
    status.value.control_state = 'requested';
  });
  unlistenDesktopOverlayError = await listen<string>('screen-share-desktop-overlay-error', (event) => {
    errorMsg.value = t('tools.screenShare.errDesktopOverlayFailed', { error: event.payload });
    void refreshStatus(true);
  });
  unlistenAnnotationState = await listen<ScreenShareAnnotationDocument>('screen-share-annotation-state', (event) => {
    applyAnnotationDocument(event.payload);
  });
  await refreshAnnotationState();
  startStatusPolling();
});

onUnmounted(() => {
  unlistenStatus?.();
  unlistenLog?.();
  unlistenControlRequest?.();
  unlistenDesktopOverlayError?.();
  unlistenAnnotationState?.();
  if (statusRefreshTimer) {
    clearInterval(statusRefreshTimer);
    statusRefreshTimer = null;
  }
});
</script>

<template>
  <div class="flex flex-1 flex-col overflow-y-auto bg-gradient-to-br from-slate-50 to-slate-100">
    <div class="mx-auto flex w-full max-w-6xl flex-col gap-5 p-6 pb-10">
      <div class="flex flex-col gap-4 md:flex-row md:items-start md:justify-between">
        <div class="flex items-start gap-3">
          <div class="relative flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-violet-500 to-indigo-600 shadow-sm">
            <MonitorUp class="h-5 w-5 text-white" />
            <span
              v-if="isActive"
              class="absolute -right-1 -top-1 flex h-3.5 w-3.5 items-center justify-center rounded-full border-2 border-white bg-emerald-500"
            >
              <span class="h-1.5 w-1.5 rounded-full bg-white"></span>
            </span>
          </div>
          <div>
            <h1 class="text-2xl font-bold text-slate-900">{{ t('sidebar.screenShare') }}</h1>
            <p class="mt-1 text-sm text-slate-500">
              {{ isActive ? serverUrl : t('tools.screenShare.description') }}
            </p>
          </div>
        </div>
        <div
          class="inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-xs font-semibold shadow-sm"
          :class="isActive ? 'border-emerald-200 bg-emerald-50 text-emerald-700' : 'border-slate-200 bg-white text-slate-500'"
        >
          <span class="h-2 w-2 rounded-full" :class="isActive ? 'bg-emerald-500 motion-safe:animate-pulse motion-reduce:animate-none' : 'bg-slate-300'"></span>
          {{ isActive ? t('tools.screenShare.statusActive') : t('tools.screenShare.statusIdle') }}
        </div>
      </div>

      <div class="grid grid-cols-1 gap-5 lg:grid-cols-5">
        <div class="space-y-4 lg:col-span-3">
          <div class="ss-card">
            <p class="ss-section-label">{{ t('tools.screenShare.monitorSelect') }}</p>
            <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div>
                <label class="ss-label">{{ t('tools.screenShare.monitorSelect') }}</label>
                <select v-model="selectedMonitor" :disabled="isActive" class="ss-select w-full">
                  <option v-for="option in monitorOptions" :key="option.value" :value="option.value">
                    {{ option.label }}
                  </option>
                </select>
              </div>
              <div>
                <label class="ss-label">{{ t('tools.screenShare.bindAddress') }}</label>
                <select v-model="selectedBindAddress" :disabled="isActive" class="ss-select w-full">
                  <option value="0.0.0.0">{{ t('tools.screenShare.allInterfaces') }} (0.0.0.0)</option>
                  <option v-for="iface in interfaces" :key="iface.ip" :value="iface.ip">
                    {{ iface.name }} ({{ iface.ip }})
                  </option>
                </select>
              </div>
            </div>
          </div>

          <div class="grid grid-cols-1 gap-4 xl:grid-cols-3">
            <div class="ss-card">
              <p class="ss-section-label">{{ t('tools.screenShare.port') }}</p>
              <input
                v-model.number="port"
                type="number"
                min="1024"
                max="65535"
                :disabled="isActive"
                class="ss-input w-full"
              >
              <p class="mt-2 text-xs text-slate-500">{{ t('tools.screenShare.portHint') }}</p>
            </div>

            <div class="ss-card">
              <div class="mb-3 flex items-center justify-between gap-3">
                <p class="ss-section-label !mb-0">{{ t('tools.screenShare.usernameToggle') }}</p>
                <label class="ss-toggle">
                  <input v-model="usernameEnabled" type="checkbox" :disabled="isActive" class="sr-only">
                  <span class="ss-toggle-track" :class="usernameEnabled ? 'bg-violet-600' : 'bg-slate-300'">
                    <span class="ss-toggle-thumb" :class="usernameEnabled ? 'translate-x-4' : 'translate-x-0'"></span>
                  </span>
                </label>
              </div>
              <input
                v-if="usernameEnabled"
                v-model="username"
                type="text"
                :disabled="isActive"
                :placeholder="t('tools.screenShare.usernameInput')"
                class="ss-input w-full"
              >
              <div v-else class="flex h-10 items-center rounded-lg border border-dashed border-slate-200 px-3 text-sm text-slate-400">
                --
              </div>
            </div>

            <div class="ss-card">
              <div class="mb-3 flex items-center justify-between gap-3">
                <p class="ss-section-label !mb-0">{{ t('tools.screenShare.passwordToggle') }}</p>
                <label class="ss-toggle">
                  <input v-model="passwordEnabled" type="checkbox" :disabled="isActive" class="sr-only">
                  <span class="ss-toggle-track" :class="passwordEnabled ? 'bg-violet-600' : 'bg-slate-300'">
                    <span class="ss-toggle-thumb" :class="passwordEnabled ? 'translate-x-4' : 'translate-x-0'"></span>
                  </span>
                </label>
              </div>
              <input
                v-if="passwordEnabled"
                v-model="password"
                type="password"
                :disabled="isActive"
                :placeholder="t('tools.screenShare.passwordInput')"
                class="ss-input w-full"
              >
              <div v-else class="flex h-10 items-center rounded-lg border border-dashed border-slate-200 px-3 text-sm text-slate-400">
                --
              </div>
            </div>
          </div>

          <div class="ss-card">
            <p class="ss-section-label">{{ t('tools.screenShare.performance') }}</p>
            <div class="space-y-5">
              <div>
                <div class="mb-2 flex items-center justify-between">
                  <label class="ss-label">{{ t('tools.screenShare.quality') }}</label>
                  <span class="font-mono text-sm font-semibold text-violet-600">{{ quality }}</span>
                </div>
                <div class="flex items-center gap-3">
                  <span class="text-[11px] text-slate-400">{{ t('tools.screenShare.qualityLow') }}</span>
                  <input
                    v-model.number="quality"
                    type="range"
                    min="10"
                    max="100"
                    step="5"
                    :disabled="isActive"
                    class="ss-range flex-1"
                  >
                  <span class="text-[11px] text-slate-400">{{ t('tools.screenShare.qualityHigh') }}</span>
                </div>
              </div>
              <div>
                <div class="mb-2 flex items-center justify-between">
                  <label class="ss-label">{{ t('tools.screenShare.fps') }}</label>
                  <span class="font-mono text-sm font-semibold text-violet-600">
                    {{ fps }} <span class="text-xs font-normal text-slate-400">{{ t('tools.screenShare.fpsUnit') }}</span>
                  </span>
                </div>
                <input
                  v-model.number="fps"
                  type="range"
                  min="5"
                  max="30"
                  step="5"
                  :disabled="isActive"
                  class="ss-range w-full"
                >
              </div>
              <div>
                <div class="mb-2 flex items-center justify-between gap-3">
                  <label class="ss-label">{{ t('tools.screenShare.mediaTransport') }}</label>
                  <span class="text-[11px] text-slate-400">{{ t('tools.screenShare.mediaTransportHint') }}</span>
                </div>
                <div class="grid grid-cols-3 gap-1 rounded-xl border border-slate-200 bg-slate-100 p-1">
                  <button
                    v-for="option in mediaTransportOptions"
                    :key="option.value"
                    type="button"
                    class="rounded-lg px-2 py-2 text-xs font-medium transition"
                    :class="mediaTransport === option.value ? 'bg-white text-violet-700 shadow-sm' : 'text-slate-500 hover:text-slate-700'"
                    :disabled="isActive"
                    :title="option.description"
                    @click="mediaTransport = option.value"
                  >
                    {{ option.label }}
                  </button>
                </div>
                <p class="mt-2 text-xs leading-5 text-slate-500">
                  {{ mediaTransportOptions.find((option) => option.value === mediaTransport)?.description }}
                </p>
              </div>
              <div>
                <div class="mb-2 flex items-center justify-between gap-3">
                  <label class="ss-label">{{ t('tools.screenShare.backendMode') }}</label>
                  <span class="text-[11px] text-slate-400">{{ t('tools.screenShare.backendModeHint') }}</span>
                </div>
                <div class="space-y-2">
                  <label
                    v-for="option in backendModeOptions"
                    :key="option.value"
                    class="flex cursor-pointer items-start gap-3 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5 transition hover:border-violet-200 hover:bg-violet-50/40"
                    :class="backendMode === option.value ? 'border-violet-300 bg-violet-50' : ''"
                  >
                    <input
                      v-model="backendMode"
                      type="radio"
                      name="screen-share-backend-mode"
                      :value="option.value"
                      :disabled="isActive"
                      class="mt-0.5 h-4 w-4 accent-violet-600"
                    >
                    <span class="min-w-0">
                      <span class="block text-sm font-medium text-slate-800">{{ option.label }}</span>
                      <span class="mt-1 block text-xs leading-5 text-slate-500">{{ option.description }}</span>
                    </span>
                  </label>
                </div>
              </div>
            </div>
          </div>

          <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <label class="ss-toggle-card">
              <span class="flex items-center gap-3">
                <span class="ss-toggle">
                  <input v-model="showCursor" type="checkbox" :disabled="isActive" class="sr-only">
                  <span class="ss-toggle-track" :class="showCursor ? 'bg-violet-600' : 'bg-slate-300'">
                    <span class="ss-toggle-thumb" :class="showCursor ? 'translate-x-4' : 'translate-x-0'"></span>
                  </span>
                </span>
                <span class="text-sm font-medium text-slate-700">{{ t('tools.screenShare.showCursor') }}</span>
              </span>
            </label>
            <label class="ss-toggle-card">
              <span class="flex items-center gap-3">
                <span class="ss-toggle">
                  <input
                    v-model="autoStart"
                    type="checkbox"
                    :disabled="isActive"
                    class="sr-only"
                    @change="saveSettings"
                  >
                  <span class="ss-toggle-track" :class="autoStart ? 'bg-violet-600' : 'bg-slate-300'">
                    <span class="ss-toggle-thumb" :class="autoStart ? 'translate-x-4' : 'translate-x-0'"></span>
                  </span>
                </span>
                <span class="text-sm font-medium text-slate-700">{{ t('tools.screenShare.autoStart') }}</span>
              </span>
            </label>
            <label class="ss-toggle-card">
              <span class="flex items-center gap-3">
                <span class="ss-toggle">
                  <input
                    v-model="controlRequestsEnabled"
                    type="checkbox"
                    :disabled="isActive"
                    class="sr-only"
                  >
                  <span class="ss-toggle-track" :class="controlRequestsEnabled ? 'bg-emerald-600' : 'bg-slate-300'">
                    <span class="ss-toggle-thumb" :class="controlRequestsEnabled ? 'translate-x-4' : 'translate-x-0'"></span>
                  </span>
                </span>
                <span class="text-sm font-medium text-slate-700">{{ t('tools.screenShare.allowControlRequests') }}</span>
              </span>
            </label>
            <label
              class="ss-toggle-card"
              :class="isActive || !controlRequestsEnabled ? 'cursor-not-allowed opacity-60' : 'cursor-pointer'"
              :title="!controlRequestsEnabled ? t('tools.screenShare.keyboardControlRequiresRemote') : undefined"
            >
              <span class="flex items-center gap-3">
                <span class="ss-toggle">
                  <input
                    v-model="keyboardControlEnabled"
                    type="checkbox"
                    :disabled="isActive || !controlRequestsEnabled"
                    class="sr-only"
                  >
                  <span class="ss-toggle-track" :class="keyboardControlEnabled && controlRequestsEnabled ? 'bg-emerald-600' : 'bg-slate-300'">
                    <span class="ss-toggle-thumb" :class="keyboardControlEnabled && controlRequestsEnabled ? 'translate-x-4' : 'translate-x-0'"></span>
                  </span>
                </span>
                <span class="text-sm font-medium text-slate-700">{{ t('tools.screenShare.allowKeyboardControl') }}</span>
              </span>
            </label>
          </div>

          <div v-if="errorMsg" class="rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-600 shadow-sm">
            {{ errorMsg }}
          </div>

          <button
            v-if="!isActive"
            type="button"
            @click="startShare()"
            :disabled="isStarting || monitors.length === 0"
            class="w-full rounded-xl bg-gradient-to-r from-violet-600 to-indigo-600 px-6 py-3.5 text-sm font-semibold text-white shadow-sm transition hover:from-violet-700 hover:to-indigo-700 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <span class="flex items-center justify-center gap-2">
              <Play class="h-4 w-4" />
              {{ isStarting ? t('tools.screenShare.starting') : t('tools.screenShare.startShare') }}
            </span>
          </button>
          <button
            v-else
            type="button"
            @click="stopShare"
            class="w-full rounded-xl border border-red-200 bg-red-50 px-6 py-3.5 text-sm font-semibold text-red-600 shadow-sm transition hover:border-red-300 hover:bg-red-100"
          >
            <span class="flex items-center justify-center gap-2">
              <Power class="h-4 w-4" />
              {{ t('tools.screenShare.stopShare') }}
            </span>
          </button>
        </div>

        <div class="space-y-4 lg:col-span-2">
          <template v-if="isActive && serverUrl">
            <div
              v-if="status.capture_paused"
              role="status"
              aria-live="polite"
              class="flex items-start gap-3 rounded-xl border px-4 py-3 text-sm shadow-sm"
              :class="privacyModeDetected
                ? 'border-red-200 bg-red-50 text-red-700'
                : 'border-amber-200 bg-amber-50 text-amber-700'"
            >
              <MonitorOff v-if="privacyModeDetected" class="mt-0.5 h-5 w-5 shrink-0" aria-hidden="true" />
              <span v-else class="mt-1.5 h-2 w-2 shrink-0 animate-pulse rounded-full bg-amber-500"></span>
              <span>
                <strong class="block font-semibold">
                  {{ privacyModeDetected ? t('tools.screenShare.privacyModeDetected') : t('tools.screenShare.capturePaused') }}
                </strong>
                <span v-if="privacyModeDetected" class="mt-1 block leading-5 text-red-600">
                  {{ t('tools.screenShare.privacyModeHint') }}
                </span>
              </span>
            </div>
            <div class="ss-card">
              <p class="ss-section-label">{{ t('tools.screenShare.accessUrl') }}</p>
              <div class="mb-3 flex flex-wrap gap-2">
                <button type="button" class="ss-detail-button" :disabled="isOpeningPreview" @click="openLocalPreview">
                  <Eye class="h-3.5 w-3.5" />
                  {{ isOpeningPreview ? t('tools.screenShare.openingPreview') : t('tools.screenShare.openLocalPreview') }}
                </button>
                <button
                  type="button"
                  class="ss-detail-button"
                  :class="status.desktop_overlay_active ? 'border-violet-300 bg-violet-50 text-violet-700' : ''"
                  :disabled="isTogglingDesktopOverlay"
                  :aria-pressed="status.desktop_overlay_active"
                  @click="toggleDesktopOverlay"
                >
                  <Layers3 class="h-3.5 w-3.5" aria-hidden="true" />
                  {{ isTogglingDesktopOverlay
                    ? t('tools.screenShare.desktopOverlayUpdating')
                    : status.desktop_overlay_active
                      ? t('tools.screenShare.hideDesktopAnnotations')
                      : t('tools.screenShare.showDesktopAnnotations') }}
                </button>
                <button type="button" class="ss-detail-button" :disabled="!latestPersistentAnnotation || isClearingAnnotations || annotationActionId !== null" @click="undoLatestHostAnnotation">
                  <Undo2 class="h-3.5 w-3.5" />
                  {{ t('tools.screenShare.undoLatestAnnotation') }}
                </button>
                <button type="button" class="ss-detail-button" :disabled="isClearingAnnotations || annotationActionId !== null || status.annotation_count === 0" @click="clearAllAnnotations">
                  <Eraser class="h-3.5 w-3.5" />
                  {{ isClearingAnnotations ? t('tools.screenShare.clearingAnnotations') : t('tools.screenShare.clearAllAnnotations') }}
                </button>
              </div>
              <div v-if="persistentAnnotations.length" class="mb-3 rounded-xl border border-slate-200 bg-slate-50/80 p-3">
                <div class="mb-2 flex items-center justify-between gap-3">
                  <div>
                    <p class="text-sm font-semibold text-slate-800">{{ t('tools.screenShare.annotationManagement') }}</p>
                    <p class="mt-0.5 text-xs text-slate-500">{{ t('tools.screenShare.annotationManagementHint') }}</p>
                  </div>
                  <span class="rounded-full bg-white px-2 py-1 text-[11px] font-semibold text-slate-500">
                    {{ persistentAnnotations.length }}
                  </span>
                </div>
                <div class="max-h-72 divide-y divide-slate-200 overflow-y-auto rounded-lg border border-slate-200 bg-white">
                  <div v-for="shape in persistentAnnotations" :key="shape.id" class="flex flex-wrap items-center gap-2 px-3 py-2.5">
                    <div class="mr-auto min-w-[130px]">
                      <div class="flex items-center gap-2 text-xs font-semibold text-slate-800">
                        <span class="h-3 w-3 rounded-full border border-slate-300" :style="{ backgroundColor: shape.color }" aria-hidden="true" />
                        {{ annotationTypeLabel(shape) }}
                      </div>
                      <div class="mt-0.5 text-[11px] text-slate-500" :title="shape.owner_client_id">
                        {{ t('tools.screenShare.annotationOwner') }}: {{ annotationOwnerLabel(shape) }}
                      </div>
                    </div>
                    <div class="flex items-center gap-1" :aria-label="t('tools.screenShare.annotationColor')">
                      <button
                        v-for="swatch in hostAnnotationColors"
                        :key="swatch"
                        type="button"
                        class="h-11 w-11 cursor-pointer rounded-full border-2 border-white shadow-sm ring-1 ring-slate-300 transition-colors disabled:cursor-not-allowed disabled:opacity-40"
                        :class="shape.color === swatch ? 'ring-2 ring-violet-500' : ''"
                        :style="{ backgroundColor: swatch }"
                        :aria-label="`${t('tools.screenShare.annotationSetColor')}: ${swatch}`"
                        :aria-pressed="shape.color === swatch"
                        :disabled="annotationActionId !== null"
                        @click="updateHostAnnotation(shape, swatch, shape.width)"
                      />
                    </div>
                    <label class="sr-only" :for="`annotation-width-${shape.id}`">{{ t('tools.screenShare.annotationLineWidth') }}</label>
                    <select
                      :id="`annotation-width-${shape.id}`"
                      class="h-11 rounded-lg border border-slate-200 bg-white px-2 text-xs font-semibold text-slate-700 outline-none focus:border-violet-400 focus:ring-2 focus:ring-violet-100"
                      :value="shape.width"
                      :disabled="annotationActionId !== null"
                      @change="changeHostAnnotationWidth(shape, $event)"
                    >
                      <option v-for="widthOption in hostAnnotationWidths" :key="widthOption" :value="widthOption">{{ widthOption }}px</option>
                    </select>
                    <div class="flex items-center gap-1" :aria-label="t('tools.screenShare.annotationMove')">
                      <button type="button" class="ss-icon-button inline-flex h-11 w-11 items-center justify-center" :title="t('tools.screenShare.annotationMoveLeft')" :aria-label="t('tools.screenShare.annotationMoveLeft')" :disabled="annotationActionId !== null" @click="nudgeHostAnnotation(shape, -0.01, 0)"><ArrowLeft class="h-4 w-4" /></button>
                      <button type="button" class="ss-icon-button inline-flex h-11 w-11 items-center justify-center" :title="t('tools.screenShare.annotationMoveRight')" :aria-label="t('tools.screenShare.annotationMoveRight')" :disabled="annotationActionId !== null" @click="nudgeHostAnnotation(shape, 0.01, 0)"><ArrowRight class="h-4 w-4" /></button>
                      <button type="button" class="ss-icon-button inline-flex h-11 w-11 items-center justify-center" :title="t('tools.screenShare.annotationMoveUp')" :aria-label="t('tools.screenShare.annotationMoveUp')" :disabled="annotationActionId !== null" @click="nudgeHostAnnotation(shape, 0, -0.01)"><ArrowUp class="h-4 w-4" /></button>
                      <button type="button" class="ss-icon-button inline-flex h-11 w-11 items-center justify-center" :title="t('tools.screenShare.annotationMoveDown')" :aria-label="t('tools.screenShare.annotationMoveDown')" :disabled="annotationActionId !== null" @click="nudgeHostAnnotation(shape, 0, 0.01)"><ArrowDown class="h-4 w-4" /></button>
                    </div>
                    <button type="button" class="ss-icon-button inline-flex h-11 w-11 items-center justify-center text-red-600 hover:bg-red-50" :title="t('tools.screenShare.removeAnnotation')" :aria-label="t('tools.screenShare.removeAnnotation')" :disabled="annotationActionId !== null" @click="removeHostAnnotation(shape)"><Trash2 class="h-4 w-4" /></button>
                  </div>
                </div>
              </div>
              <div
                v-if="status.pending_control_request"
                class="mb-3 rounded-xl border border-amber-200 bg-amber-50 px-3 py-3 text-sm text-amber-900"
                role="alert"
              >
                <div class="flex items-start gap-3">
                  <MousePointer2 class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                  <div class="min-w-0 flex-1">
                    <p class="font-semibold">{{ t('tools.screenShare.controlRequestTitle') }}</p>
                    <p class="mt-1 truncate text-xs text-amber-800">
                      {{ status.pending_control_request.ip }} · {{ status.pending_control_request.user_agent }}
                    </p>
                    <div class="mt-3 flex flex-wrap gap-2">
                      <button
                        type="button"
                        class="ss-detail-button border-emerald-200 bg-emerald-50 text-emerald-700"
                        :disabled="isRespondingControl"
                        @click="respondControlRequest(true)"
                      >
                        <Check class="h-3.5 w-3.5" />
                        {{ t('tools.screenShare.allowControl') }}
                      </button>
                      <button
                        type="button"
                        class="ss-detail-button border-red-200 bg-red-50 text-red-700"
                        :disabled="isRespondingControl"
                        @click="respondControlRequest(false)"
                      >
                        <X class="h-3.5 w-3.5" />
                        {{ t('tools.screenShare.denyControl') }}
                      </button>
                    </div>
                  </div>
                </div>
              </div>
              <div
                v-else-if="status.control_state === 'granted'"
                class="mb-3 flex items-center gap-3 rounded-xl border border-emerald-200 bg-emerald-50 px-3 py-3 text-sm text-emerald-800"
                role="status"
              >
                <ShieldCheck class="h-4 w-4 shrink-0" aria-hidden="true" />
                <span class="min-w-0 flex-1 truncate">
                  {{ t('tools.screenShare.controlGranted', { ip: status.controller_ip || t('tools.screenShare.unknownViewer') }) }}
                </span>
                <button
                  type="button"
                  class="ss-detail-button border-red-200 bg-white text-red-700"
                  :disabled="isRevokingControl"
                  @click="revokeControl"
                >
                  <ShieldX class="h-3.5 w-3.5" />
                  {{ t('tools.screenShare.revokeControl') }}
                </button>
              </div>
              <div class="space-y-2">
                <div v-for="url in allUrls" :key="url" class="rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5">
                  <div class="flex items-center gap-2">
                    <code class="flex-1 truncate font-mono text-sm font-semibold text-violet-700">{{ url }}</code>
                    <button
                      type="button"
                      @click="copyUrl(url)"
                      class="ss-icon-button"
                      :title="t('tools.screenShare.copyUrl')"
                      :aria-label="t('tools.screenShare.copyUrl')"
                    >
                      <Copy class="h-4 w-4" :class="copiedUrl === url ? 'text-violet-600' : ''" />
                    </button>
                    <button
                      type="button"
                      @click="toggleQr(url)"
                      class="ss-icon-button"
                      :title="qrForUrl === url ? t('tools.screenShare.hideQrCode') : t('tools.screenShare.showQrCode')"
                      :aria-label="qrForUrl === url ? t('tools.screenShare.hideQrCode') : t('tools.screenShare.showQrCode')"
                    >
                      <QrCode class="h-4 w-4" :class="qrForUrl === url ? 'text-violet-600' : ''" />
                    </button>
                    <button
                      type="button"
                      @click="openInBrowser(url)"
                      class="ss-icon-button"
                      :title="t('tools.screenShare.openInBrowser')"
                      :aria-label="t('tools.screenShare.openInBrowser')"
                    >
                      <ExternalLink class="h-4 w-4" />
                    </button>
                  </div>
                  <div v-if="qrForUrl === url" class="mt-3 flex justify-center">
                    <div class="rounded-xl border border-slate-200 bg-white p-3 shadow-sm">
                      <canvas :ref="(el) => setQrCanvas(url, el)" width="128" height="128" />
                    </div>
                  </div>
                </div>
              </div>
              <p class="mt-2 text-xs text-slate-500">{{ t('tools.screenShare.qrCodeHint') }}</p>
            </div>

            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div class="ss-stat-card">
                <div class="mb-3 flex items-center gap-2 text-slate-500">
                  <Users class="h-4 w-4 text-violet-500" />
                  <span class="text-[11px] font-semibold uppercase tracking-[0.14em]">
                    {{ t('tools.screenShare.connectionCount') }}
                  </span>
                </div>
                <div class="font-mono text-2xl font-bold text-slate-900">{{ connectionCount }}</div>
              </div>

              <div class="ss-stat-card">
                <div class="mb-2 flex items-center gap-2 text-slate-500">
                  <MousePointer2 class="h-4 w-4 text-violet-500" />
                  <span class="text-[11px] font-semibold uppercase tracking-[0.14em]">{{ t('tools.screenShare.controlStatus') }}</span>
                </div>
                <div class="flex items-center gap-2 text-sm font-semibold text-slate-900">
                  <ShieldCheck v-if="status.control_state === 'granted'" class="h-4 w-4 text-emerald-600" />
                  <ShieldX v-else class="h-4 w-4 text-slate-400" />
                  {{ t(`tools.screenShare.controlState.${status.control_state}`) }}
                </div>
              </div>
              <div class="ss-stat-card">
                <div class="mb-2 flex items-center gap-2 text-slate-500">
                  <PencilRuler class="h-4 w-4 text-violet-500" />
                  <span class="text-[11px] font-semibold uppercase tracking-[0.14em]">{{ t('tools.screenShare.annotationCount') }}</span>
                </div>
                <div class="font-mono text-2xl font-bold text-slate-900">{{ status.annotation_count }}</div>
                <div class="mt-1 text-xs text-slate-500">
                  {{ status.view_mode === 'frozen' ? t('tools.screenShare.viewModeFrozen') : t('tools.screenShare.viewModeLive') }}
                </div>
              </div>

              <div class="ss-stat-card">
                <div class="mb-2 flex items-center gap-2 text-slate-500">
                  <Clock class="h-4 w-4 text-violet-500" />
                  <span class="text-[11px] font-semibold uppercase tracking-[0.14em]">{{ t('tools.screenShare.uptime') }}</span>
                </div>
                <div class="font-mono text-2xl font-bold text-slate-900">{{ formattedUptime }}</div>
              </div>

              <div class="ss-stat-card">
                <div class="mb-2 flex items-center gap-2 text-slate-500">
                  <Gauge class="h-4 w-4 text-violet-500" />
                  <span class="text-[11px] font-semibold uppercase tracking-[0.14em]">{{ t('tools.screenShare.actualFps') }}</span>
                </div>
                <div class="font-mono text-2xl font-bold text-slate-900">{{ status.fps_actual.toFixed(1) }}</div>
              </div>

              <div class="ss-stat-card">
                <div class="mb-2 flex items-center gap-2 text-slate-500">
                  <ArrowUpFromLine class="h-4 w-4 text-violet-500" />
                  <span class="text-[11px] font-semibold uppercase tracking-[0.14em]">{{ t('tools.screenShare.bitrate') }}</span>
                </div>
                <div class="font-mono text-2xl font-bold text-slate-900">{{ formattedBitrate }}</div>
                <div class="mt-1 text-xs text-slate-500">
                  {{ t('tools.screenShare.activeTransport') }}: {{ activeTransportLabel }}
                </div>
              </div>
            </div>

            <div class="ss-card">
              <div class="mb-3 flex items-center justify-between gap-3">
                <div>
                  <h3 class="text-sm font-semibold text-slate-900">{{ t('tools.screenShare.connectedIpList') }}</h3>
                  <p class="text-xs text-slate-500">{{ t('tools.screenShare.connectionCount') }}: {{ connectionCount }}</p>
                  <p class="text-xs text-slate-400">{{ t('tools.screenShare.autoRefreshHint', { seconds: AUTO_REFRESH_SECONDS }) }}</p>
                </div>
                <button
                  type="button"
                  class="ss-detail-button"
                  :disabled="isRefreshingStatus"
                  :title="t('tools.screenShare.refreshStatus')"
                  @click="refreshStatus()"
                >
                  <RefreshCw class="h-3.5 w-3.5" :class="{ 'animate-spin': isRefreshingStatus }" />
                  {{ t('tools.screenShare.refreshStatus') }}
                </button>
              </div>
              <div v-if="connectedIps.length > 0" class="space-y-2">
                <div
                  v-for="ip in visibleConnIps"
                  :key="ip"
                  class="rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 font-mono text-sm text-slate-700"
                >
                  {{ ip }}
                </div>
                <button v-if="hiddenConnIpCount > 0" type="button" class="ss-detail-button" @click="showAllConnIps = true">{{ t('tools.screenShare.showMoreIps', { n: hiddenConnIpCount }) }}<ChevronDown class="h-3.5 w-3.5" /></button>
                <button v-else-if="connectedIps.length > 10" type="button" class="ss-detail-button" @click="showAllConnIps = false">{{ t('tools.screenShare.collapseIps') }}<ChevronUp class="h-3.5 w-3.5" /></button>
              </div>
              <div v-else class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-4 py-6 text-center text-sm text-slate-400">
                {{ t('tools.screenShare.noConnections') }}
              </div>
            </div>
          </template>

          <template v-else>
            <div class="flex min-h-56 flex-col items-center justify-center rounded-xl border border-slate-200/80 bg-white p-8 text-center shadow-sm">
              <div class="mb-4 flex h-16 w-16 items-center justify-center rounded-2xl bg-violet-50 text-violet-600">
                <MonitorUp class="h-7 w-7" />
              </div>
              <p class="text-sm text-slate-500">{{ t('tools.screenShare.description') }}</p>
            </div>
          </template>

          <div v-if="logs.length > 0" class="ss-card">
            <h3 class="ss-section-label">{{ t('tools.screenShare.logTitle') }}</h3>
            <div class="max-h-48 space-y-2 overflow-y-auto">
              <div v-for="(log, index) in logs" :key="index" class="flex gap-3 text-xs">
                <span class="shrink-0 font-mono text-slate-400">{{ log.time }}</span>
                <span :class="log.level === 'error' ? 'text-red-600' : 'text-slate-600'">{{ log.message }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.ss-card {
  border: 1px solid rgb(226 232 240 / 0.8);
  border-radius: 0.75rem;
  background: white;
  padding: 1.25rem;
  box-shadow: 0 1px 2px rgb(15 23 42 / 0.06);
}

.ss-stat-card {
  border: 1px solid rgb(226 232 240 / 0.8);
  border-radius: 0.75rem;
  background: white;
  padding: 1rem;
  box-shadow: 0 1px 2px rgb(15 23 42 / 0.06);
}

.ss-section-label {
  margin-bottom: 1rem;
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: rgb(100 116 139);
}

.ss-label {
  display: block;
  margin-bottom: 0.4rem;
  font-size: 0.75rem;
  font-weight: 600;
  color: rgb(71 85 105);
}

.ss-select {
  appearance: none;
  border: 1px solid rgb(203 213 225);
  border-radius: 0.75rem;
  background-color: white;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%2364758b' stroke-width='2'%3E%3Cpolyline points='6 9 12 15 18 9'%3E%3C/polyline%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 12px center;
  padding: 0.65rem 2.25rem 0.65rem 0.9rem;
  font-size: 0.875rem;
  color: rgb(15 23 42);
  outline: none;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

.ss-select:focus {
  border-color: rgb(139 92 246);
  box-shadow: 0 0 0 3px rgb(139 92 246 / 0.12);
}

.ss-select:disabled {
  cursor: not-allowed;
  background-color: rgb(248 250 252);
  color: rgb(148 163 184);
}

.ss-input {
  border: 1px solid rgb(203 213 225);
  border-radius: 0.75rem;
  background: white;
  padding: 0.65rem 0.9rem;
  font-size: 0.875rem;
  color: rgb(15 23 42);
  outline: none;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

.ss-input:focus {
  border-color: rgb(139 92 246);
  box-shadow: 0 0 0 3px rgb(139 92 246 / 0.12);
}

.ss-input:disabled {
  cursor: not-allowed;
  background-color: rgb(248 250 252);
  color: rgb(148 163 184);
}

.ss-toggle {
  position: relative;
  display: inline-flex;
}

.ss-toggle-track {
  display: block;
  height: 20px;
  width: 36px;
  flex-shrink: 0;
  border-radius: 9999px;
  transition: background-color 0.2s ease;
}

.ss-toggle-thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  height: 16px;
  width: 16px;
  border-radius: 9999px;
  background: white;
  box-shadow: 0 1px 3px rgb(15 23 42 / 0.2);
  transition: transform 0.2s ease;
}

.ss-range {
  appearance: none;
  height: 6px;
  border-radius: 9999px;
  background: rgb(226 232 240);
  outline: none;
}

.ss-range:disabled {
  cursor: not-allowed;
  opacity: 0.5;
}

.ss-range::-webkit-slider-thumb {
  appearance: none;
  width: 16px;
  height: 16px;
  border-radius: 9999px;
  background: rgb(124 58 237);
  box-shadow: 0 0 0 4px rgb(124 58 237 / 0.18);
  cursor: pointer;
}

.ss-toggle-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  border: 1px solid rgb(226 232 240 / 0.8);
  border-radius: 0.75rem;
  background: white;
  padding: 0.9rem 1rem;
  box-shadow: 0 1px 2px rgb(15 23 42 / 0.06);
}

.ss-icon-button {
  border: 1px solid rgb(226 232 240);
  border-radius: 0.65rem;
  background: white;
  padding: 0.45rem;
  color: rgb(100 116 139);
  cursor: pointer;
  transition: border-color 0.15s ease, color 0.15s ease, background-color 0.15s ease;
}

.ss-icon-button:hover {
  border-color: rgb(196 181 253);
  background: rgb(245 243 255);
  color: rgb(109 40 217);
}

.ss-inline-button {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.75rem;
  font-weight: 600;
  color: rgb(100 116 139);
  transition: color 0.15s ease;
}

.ss-inline-button:hover {
  color: rgb(109 40 217);
}

.ss-detail-button {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  min-height: 2.75rem;
  border: 1px solid rgb(226 232 240);
  border-radius: 9999px;
  background: rgb(248 250 252);
  padding: 0.5rem 0.85rem;
  font-size: 0.75rem;
  font-weight: 600;
  color: rgb(71 85 105);
  cursor: pointer;
  transition: border-color 0.15s ease, background-color 0.15s ease, color 0.15s ease;
}

.ss-icon-button:disabled {
  cursor: not-allowed;
  opacity: 0.45;
}

.ss-detail-button:hover {
  border-color: rgb(196 181 253);
  background: rgb(245 243 255);
  color: rgb(109 40 217);
}

.ss-detail-button:disabled {
  cursor: not-allowed;
  opacity: 0.45;
}

.ss-detail-button:focus-visible {
  outline: 2px solid rgb(139 92 246 / 0.7);
  outline-offset: 2px;
}

@media (prefers-reduced-motion: reduce) {
  .ss-detail-button {
    transition: none;
  }
}
</style>
