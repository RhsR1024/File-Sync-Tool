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
  Square,
  ChevronDown,
  ChevronUp,
  ExternalLink,
} from 'lucide-vue-next';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import QRCode from 'qrcode';
import {
  screenShareListMonitors,
  screenShareListInterfaces,
  screenShareStart,
  screenShareStop,
  screenShareGetStatus,
  type MonitorInfo,
  type NetworkInterfaceInfo,
  type ScreenShareConfig,
  type ScreenShareStatus,
} from '../lib/tauri';

defineOptions({ name: 'ScreenSharePage' });

const { t } = useI18n();

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
const autoStart = ref(false);

const isActive = ref(false);
const isStarting = ref(false);
const serverUrl = ref('');
const showQr = ref(false);
const copiedUrl = ref(false);
const showAltUrls = ref(false);
const showConnectionDetails = ref(false);
const errorMsg = ref('');

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
});

const logs = ref<{ level: string; message: string; time: string }[]>([]);
const qrCanvas = ref<HTMLCanvasElement | null>(null);

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

const altUrls = computed(() => (status.value.all_urls || []).filter((url) => url !== serverUrl.value));
const connectedIps = computed(() => status.value.connected_ips || []);
const connectionCount = computed(() => status.value.connection_count ?? connectedIps.value.length);

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
  } catch {}
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
  selectedMonitor: number;
  selectedBindAddress: string;
  autoStart: boolean;
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
        selectedMonitor: selectedMonitor.value,
        selectedBindAddress: selectedBindAddress.value,
        autoStart: autoStart.value,
      } satisfies SavedSettings,
    });
  } catch {}
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
    selectedMonitor.value = saved.selectedMonitor ?? 0;
    selectedBindAddress.value = saved.selectedBindAddress ?? '0.0.0.0';
    autoStart.value = saved.autoStart ?? false;
  } catch {}
}

async function startShare() {
  errorMsg.value = '';
  isStarting.value = true;
  try {
    const config: ScreenShareConfig = {
      port: port.value,
      username: usernameEnabled.value && username.value ? username.value : null,
      password: passwordEnabled.value && password.value ? password.value : null,
      monitor_index: selectedMonitor.value,
      quality: quality.value,
      fps: fps.value,
      show_cursor: showCursor.value,
      bind_address: selectedBindAddress.value || '0.0.0.0',
    };
    const url = await screenShareStart(config);
    serverUrl.value = url;
    isActive.value = true;
    showConnectionDetails.value = false;
    await saveSettings();
  } catch (error) {
    errorMsg.value = t('tools.screenShare.errStartFailed', { error: String(error) });
  } finally {
    isStarting.value = false;
  }
}

async function stopShare() {
  try {
    await screenShareStop();
  } catch {}
  isActive.value = false;
  serverUrl.value = '';
  showQr.value = false;
  showConnectionDetails.value = false;
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
  };
}

async function copyUrl() {
  try {
    await navigator.clipboard.writeText(serverUrl.value);
    copiedUrl.value = true;
    setTimeout(() => {
      copiedUrl.value = false;
    }, 1800);
  } catch {}
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {}
}

async function openInBrowser() {
  if (!serverUrl.value) {
    return;
  }
  try {
    await invoke('open_url', { url: serverUrl.value });
  } catch {}
}

function addLog(level: string, message: string) {
  const now = new Date();
  const time = `${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}:${String(now.getSeconds()).padStart(2, '0')}`;
  logs.value.unshift({ level, message, time });
  if (logs.value.length > 50) {
    logs.value.length = 50;
  }
}

watch([showQr, serverUrl], async ([show, url]) => {
  if (!show || !url) {
    return;
  }
  await nextTick();
  if (!qrCanvas.value) {
    return;
  }
  await QRCode.toCanvas(qrCanvas.value, url, {
    width: 128,
    margin: 1,
    color: { dark: '#1e293b', light: '#ffffff' },
  });
});

let unlistenStatus: UnlistenFn | null = null;
let unlistenLog: UnlistenFn | null = null;

onMounted(async () => {
  await loadSettings();
  await loadMonitors();
  await loadInterfaces();

  try {
    const currentStatus = await screenShareGetStatus();
    if (currentStatus.is_active) {
      isActive.value = true;
      serverUrl.value = currentStatus.server_url;
      status.value = currentStatus;
    }
  } catch {}

  if (autoStart.value && !isActive.value && monitors.value.length > 0) {
    await startShare();
  }

  unlistenStatus = await listen<ScreenShareStatus>('screen-share-status', (event) => {
    status.value = event.payload;
    if (event.payload.is_active && !isActive.value) {
      isActive.value = true;
      serverUrl.value = event.payload.server_url;
    }
    if (!event.payload.is_active) {
      showConnectionDetails.value = false;
    }
  });

  unlistenLog = await listen<{ level: string; message: string }>('screen-share-log', (event) => {
    addLog(event.payload.level, event.payload.message);
  });
});

onUnmounted(() => {
  unlistenStatus?.();
  unlistenLog?.();
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
          <span class="h-2 w-2 rounded-full" :class="isActive ? 'bg-emerald-500 animate-pulse' : 'bg-slate-300'"></span>
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
            <p class="ss-section-label">Performance</p>
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
          </div>

          <div v-if="errorMsg" class="rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-600 shadow-sm">
            {{ errorMsg }}
          </div>

          <button
            v-if="!isActive"
            type="button"
            @click="startShare"
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
              <Square class="h-4 w-4" />
              {{ t('tools.screenShare.stopShare') }}
            </span>
          </button>
        </div>

        <div class="space-y-4 lg:col-span-2">
          <template v-if="isActive && serverUrl">
            <div class="ss-card">
              <p class="ss-section-label">{{ t('tools.screenShare.accessUrl') }}</p>
              <div class="flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5">
                <code class="flex-1 truncate font-mono text-sm font-semibold text-violet-700">{{ serverUrl }}</code>
                <button
                  type="button"
                  @click="copyUrl"
                  class="ss-icon-button"
                  :title="t('tools.screenShare.copyUrl')"
                  aria-label="Copy URL"
                >
                  <Copy class="h-4 w-4" :class="copiedUrl ? 'text-violet-600' : ''" />
                </button>
                <button
                  type="button"
                  @click="showQr = !showQr"
                  class="ss-icon-button"
                  :title="showQr ? t('tools.screenShare.hideQrCode') : t('tools.screenShare.showQrCode')"
                >
                  <QrCode class="h-4 w-4" :class="showQr ? 'text-violet-600' : ''" />
                </button>
                <button
                  type="button"
                  @click="openInBrowser"
                  class="ss-icon-button"
                  :title="t('tools.screenShare.openInBrowser')"
                >
                  <ExternalLink class="h-4 w-4" />
                </button>
              </div>
              <p v-if="copiedUrl" class="mt-2 text-xs text-violet-600">{{ t('tools.screenShare.copied') }}</p>
              <p v-else class="mt-2 text-xs text-slate-500">{{ t('tools.screenShare.qrCodeHint') }}</p>

              <div v-if="showQr" class="mt-4 flex justify-center">
                <div class="rounded-xl border border-slate-200 bg-white p-3 shadow-sm">
                  <canvas ref="qrCanvas" width="128" height="128" />
                </div>
              </div>

              <div v-if="altUrls.length > 0" class="mt-4">
                <button type="button" @click="showAltUrls = !showAltUrls" class="ss-inline-button">
                  <component :is="showAltUrls ? ChevronUp : ChevronDown" class="h-3.5 w-3.5" />
                  {{ t('tools.screenShare.altUrls', { n: altUrls.length }) }}
                </button>
                <div v-if="showAltUrls" class="mt-3 space-y-2">
                  <div
                    v-for="url in altUrls"
                    :key="url"
                    class="flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2"
                  >
                    <code class="flex-1 truncate font-mono text-xs text-slate-600">{{ url }}</code>
                    <button type="button" @click="copyText(url)" class="text-slate-400 transition hover:text-violet-600">
                      <Copy class="h-3.5 w-3.5" />
                    </button>
                  </div>
                </div>
              </div>
            </div>

            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div class="ss-stat-card sm:col-span-2">
                <div class="mb-3 flex items-start justify-between gap-3">
                  <div class="flex items-center gap-2 text-slate-500">
                    <Users class="h-4 w-4 text-violet-500" />
                    <span class="text-[11px] font-semibold uppercase tracking-[0.14em]">
                      {{ t('tools.screenShare.connectionCount') }}
                    </span>
                  </div>
                  <button type="button" class="ss-detail-button" @click="showConnectionDetails = !showConnectionDetails">
                    {{ t('tools.screenShare.connectionDetails') }}
                    <component :is="showConnectionDetails ? ChevronUp : ChevronDown" class="h-3.5 w-3.5" />
                  </button>
                </div>
                <div class="font-mono text-3xl font-bold text-slate-900">{{ connectionCount }}</div>
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
              </div>

              <div class="ss-stat-card sm:col-span-2">
                <div class="mb-2 flex items-center gap-2 text-slate-500">
                  <Clock class="h-4 w-4 text-violet-500" />
                  <span class="text-[11px] font-semibold uppercase tracking-[0.14em]">{{ t('tools.screenShare.uptime') }}</span>
                </div>
                <div class="font-mono text-2xl font-bold text-slate-900">{{ formattedUptime }}</div>
              </div>
            </div>

            <div v-if="showConnectionDetails" class="ss-card">
              <div class="mb-3 flex items-center justify-between gap-3">
                <div>
                  <h3 class="text-sm font-semibold text-slate-900">{{ t('tools.screenShare.connectedIpList') }}</h3>
                  <p class="text-xs text-slate-500">{{ t('tools.screenShare.connectionCount') }}: {{ connectionCount }}</p>
                </div>
              </div>
              <div v-if="connectedIps.length > 0" class="space-y-2">
                <div
                  v-for="ip in connectedIps"
                  :key="ip"
                  class="rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 font-mono text-sm text-slate-700"
                >
                  {{ ip }}
                </div>
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
  border: 1px solid rgb(226 232 240);
  border-radius: 9999px;
  background: rgb(248 250 252);
  padding: 0.35rem 0.7rem;
  font-size: 0.75rem;
  font-weight: 600;
  color: rgb(71 85 105);
  transition: border-color 0.15s ease, background-color 0.15s ease, color 0.15s ease;
}

.ss-detail-button:hover {
  border-color: rgb(196 181 253);
  background: rgb(245 243 255);
  color: rgb(109 40 217);
}
</style>
