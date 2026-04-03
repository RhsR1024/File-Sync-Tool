<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { MonitorUp, Copy, QrCode, Users, Gauge, ArrowUpFromLine, Clock, Play, Square, ChevronDown, ChevronUp } from 'lucide-vue-next';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import QRCode from 'qrcode';
import {
  screenShareListMonitors,
  screenShareStart,
  screenShareStop,
  screenShareGetStatus,
  type MonitorInfo,
  type ScreenShareConfig,
  type ScreenShareStatus,
} from '../lib/tauri';

defineOptions({ name: 'ScreenSharePage' });

const { t } = useI18n();

// ─── State ───
const monitors = ref<MonitorInfo[]>([]);
const selectedMonitor = ref(0);
const port = ref(9870);
const passwordEnabled = ref(false);
const password = ref('');
const quality = ref(70);
const fps = ref(15);
const showCursor = ref(true);

const isActive = ref(false);
const isStarting = ref(false);
const serverUrl = ref('');
const showQr = ref(false);
const copiedUrl = ref(false);
const showAltUrls = ref(false);
const errorMsg = ref('');

const status = ref<ScreenShareStatus>({
  is_active: false,
  viewer_count: 0,
  fps_actual: 0,
  bitrate_kbps: 0,
  uptime_secs: 0,
  server_url: '',
  all_urls: [],
});

const logs = ref<{ level: string; message: string; time: string }[]>([]);
const qrCanvas = ref<HTMLCanvasElement | null>(null);

// ─── Computed ───
const monitorOptions = computed(() =>
  monitors.value.map((m) => ({
    value: m.index,
    label: `${m.name} (${m.width}x${m.height}${m.is_primary ? ', ' + t('tools.screenShare.monitorPrimary') : ''})`,
  })),
);

const formattedUptime = computed(() => {
  const s = status.value.uptime_secs;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
});

const formattedBitrate = computed(() => {
  const kbps = status.value.bitrate_kbps;
  if (kbps >= 1024) return `${(kbps / 1024).toFixed(1)} Mbps`;
  return `${kbps} Kbps`;
});

const altUrls = computed(() => (status.value.all_urls || []).filter((u) => u !== serverUrl.value));

// ─── Actions ───
async function loadMonitors() {
  try {
    monitors.value = await screenShareListMonitors();
    if (monitors.value.length > 0) {
      const primary = monitors.value.findIndex((m) => m.is_primary);
      selectedMonitor.value = primary >= 0 ? primary : 0;
    }
  } catch (e) {
    errorMsg.value = t('tools.screenShare.errNoMonitor');
  }
}

async function startShare() {
  errorMsg.value = '';
  isStarting.value = true;
  try {
    const config: ScreenShareConfig = {
      port: port.value,
      password: passwordEnabled.value && password.value ? password.value : null,
      monitor_index: selectedMonitor.value,
      quality: quality.value,
      fps: fps.value,
      show_cursor: showCursor.value,
    };
    const url = await screenShareStart(config);
    serverUrl.value = url;
    isActive.value = true;
  } catch (e: any) {
    errorMsg.value = t('tools.screenShare.errStartFailed', { error: String(e) });
  } finally {
    isStarting.value = false;
  }
}

async function stopShare() {
  try {
    await screenShareStop();
  } catch (_) {
    // ignore
  }
  isActive.value = false;
  serverUrl.value = '';
  showQr.value = false;
  status.value = { is_active: false, viewer_count: 0, fps_actual: 0, bitrate_kbps: 0, uptime_secs: 0, server_url: '', all_urls: [] };
}

async function copyUrl() {
  try {
    await navigator.clipboard.writeText(serverUrl.value);
    copiedUrl.value = true;
    setTimeout(() => (copiedUrl.value = false), 1800);
  } catch (_) {
    // fallback
  }
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch (_) {}
}

function addLog(level: string, message: string) {
  const now = new Date();
  const time = `${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}:${String(now.getSeconds()).padStart(2, '0')}`;
  logs.value.unshift({ level, message, time });
  if (logs.value.length > 50) logs.value.length = 50;
}

// ─── QR Code rendering ───
watch([showQr, serverUrl], async ([show, url]) => {
  if (!show || !url) return;
  await nextTick();
  if (qrCanvas.value) {
    await QRCode.toCanvas(qrCanvas.value, url, {
      width: 128,
      margin: 1,
      color: { dark: '#1e293b', light: '#ffffff' },
    });
  }
});

// ─── Event Listeners ───
let unlistenStatus: UnlistenFn | null = null;
let unlistenLog: UnlistenFn | null = null;

onMounted(async () => {
  await loadMonitors();

  // Check if already active
  try {
    const s = await screenShareGetStatus();
    if (s.is_active) {
      isActive.value = true;
      serverUrl.value = s.server_url;
      status.value = s;
    }
  } catch (_) {}

  unlistenStatus = await listen<ScreenShareStatus>('screen-share-status', (event) => {
    status.value = event.payload;
    if (event.payload.is_active && !isActive.value) {
      isActive.value = true;
      serverUrl.value = event.payload.server_url;
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
    <div class="mx-auto w-full max-w-6xl space-y-5 p-6 pb-10">
      <!-- Header -->
      <div class="flex items-center gap-3">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-br from-purple-500 to-indigo-600 shadow-sm">
          <MonitorUp class="h-5 w-5 text-white" />
        </div>
        <div>
          <h1 class="text-2xl font-bold text-slate-900">{{ t('sidebar.screenShare') }}</h1>
        </div>
      </div>

      <!-- Main Card -->
      <div class="overflow-hidden rounded-xl border border-slate-200/80 bg-white shadow-sm">
        <div class="grid grid-cols-1 divide-y lg:grid-cols-5 lg:divide-x lg:divide-y-0">
          <!-- Left: Config -->
          <div class="col-span-3 p-5 space-y-5">
            <!-- Monitor Select -->
            <div>
              <label class="mb-1.5 block text-sm font-medium text-slate-700">{{ t('tools.screenShare.monitorSelect') }}</label>
              <select
                v-model="selectedMonitor"
                :disabled="isActive"
                class="w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-800 transition focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500 disabled:opacity-50"
              >
                <option v-for="opt in monitorOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
              </select>
            </div>

            <!-- Port + Password row -->
            <div class="grid grid-cols-2 gap-4">
              <div>
                <label class="mb-1.5 block text-sm font-medium text-slate-700">{{ t('tools.screenShare.port') }}</label>
                <input
                  v-model.number="port"
                  type="number"
                  min="1024"
                  max="65535"
                  :disabled="isActive"
                  class="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm text-slate-800 transition focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500 disabled:opacity-50"
                >
                <p class="mt-1 text-xs text-slate-400">{{ t('tools.screenShare.portHint') }}</p>
              </div>
              <div>
                <label class="mb-1.5 flex items-center gap-2 text-sm font-medium text-slate-700">
                  <input
                    v-model="passwordEnabled"
                    type="checkbox"
                    :disabled="isActive"
                    class="h-4 w-4 rounded border-slate-300 text-blue-600 focus:ring-blue-500"
                  >
                  {{ t('tools.screenShare.passwordToggle') }}
                </label>
                <input
                  v-if="passwordEnabled"
                  v-model="password"
                  type="password"
                  :disabled="isActive"
                  :placeholder="t('tools.screenShare.passwordInput')"
                  class="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 text-sm text-slate-800 transition focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500 disabled:opacity-50"
                >
              </div>
            </div>

            <!-- Quality Slider -->
            <div>
              <div class="mb-1.5 flex items-center justify-between">
                <label class="text-sm font-medium text-slate-700">{{ t('tools.screenShare.quality') }}</label>
                <span class="text-sm font-semibold text-slate-600">{{ quality }}</span>
              </div>
              <div class="flex items-center gap-3">
                <span class="text-xs text-slate-400">{{ t('tools.screenShare.qualityLow') }}</span>
                <input
                  v-model.number="quality"
                  type="range"
                  min="10"
                  max="100"
                  step="5"
                  :disabled="isActive"
                  class="flex-1 accent-blue-600"
                >
                <span class="text-xs text-slate-400">{{ t('tools.screenShare.qualityHigh') }}</span>
              </div>
            </div>

            <!-- FPS Slider -->
            <div>
              <div class="mb-1.5 flex items-center justify-between">
                <label class="text-sm font-medium text-slate-700">{{ t('tools.screenShare.fps') }}</label>
                <span class="text-sm font-semibold text-slate-600">{{ fps }} {{ t('tools.screenShare.fpsUnit') }}</span>
              </div>
              <input
                v-model.number="fps"
                type="range"
                min="5"
                max="30"
                step="5"
                :disabled="isActive"
                class="w-full accent-blue-600"
              >
            </div>

            <!-- Show Cursor -->
            <label class="flex items-center gap-2 text-sm text-slate-700">
              <input
                v-model="showCursor"
                type="checkbox"
                :disabled="isActive"
                class="h-4 w-4 rounded border-slate-300 text-blue-600 focus:ring-blue-500"
              >
              {{ t('tools.screenShare.showCursor') }}
            </label>

            <!-- Error -->
            <div v-if="errorMsg" class="rounded-lg border border-red-200 bg-red-50 px-4 py-2 text-sm text-red-600">
              {{ errorMsg }}
            </div>

            <!-- Start / Stop Button -->
            <button
              v-if="!isActive"
              @click="startShare"
              :disabled="isStarting || monitors.length === 0"
              class="inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-purple-600 to-indigo-600 px-6 py-3 text-sm font-semibold text-white shadow-lg shadow-purple-500/25 transition hover:from-purple-700 hover:to-indigo-700 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <Play class="h-4 w-4" />
              {{ isStarting ? t('tools.screenShare.starting') : t('tools.screenShare.startShare') }}
            </button>
            <button
              v-else
              @click="stopShare"
              class="inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-red-500 to-rose-600 px-6 py-3 text-sm font-semibold text-white shadow-lg shadow-red-500/25 transition hover:from-red-600 hover:to-rose-700"
            >
              <Square class="h-4 w-4" />
              {{ t('tools.screenShare.stopShare') }}
            </button>
          </div>

          <!-- Right: Status Panel -->
          <div class="col-span-2 flex flex-col bg-slate-50/50 p-5">
            <!-- Status indicator -->
            <div class="mb-4 flex items-center gap-2">
              <div
                class="h-2.5 w-2.5 rounded-full"
                :class="isActive ? 'bg-green-500 shadow-[0_0_6px_rgba(34,197,94,0.5)] animate-pulse' : 'bg-slate-300'"
              ></div>
              <span class="text-sm font-semibold" :class="isActive ? 'text-green-600' : 'text-slate-400'">
                {{ isActive ? t('tools.screenShare.statusActive') : t('tools.screenShare.statusIdle') }}
              </span>
            </div>

            <!-- Active: URL + Stats -->
            <template v-if="isActive && serverUrl">
              <!-- Access URL -->
              <div class="mb-4">
                <label class="mb-1 block text-xs font-medium uppercase tracking-wider text-slate-400">{{ t('tools.screenShare.accessUrl') }}</label>
                <div class="flex items-center gap-2 rounded-lg border border-slate-200 bg-white px-3 py-2">
                  <code class="flex-1 truncate text-sm font-bold text-blue-600">{{ serverUrl }}</code>
                  <button @click="copyUrl" class="rounded p-1 text-slate-400 transition hover:bg-slate-100 hover:text-slate-600" :title="t('tools.screenShare.copyUrl')" aria-label="Copy URL">>
                    <Copy class="h-4 w-4" :class="copiedUrl ? 'text-blue-500' : ''" />
                  </button>
                  <button @click="showQr = !showQr" class="rounded p-1 text-slate-400 transition hover:bg-slate-100 hover:text-slate-600" :title="showQr ? t('tools.screenShare.hideQrCode') : t('tools.screenShare.showQrCode')">
                    <QrCode class="h-4 w-4" />
                  </button>
                </div>
                <p v-if="copiedUrl" class="mt-1 text-xs text-blue-500">{{ t('tools.screenShare.copied') }}</p>
                <p v-else class="mt-1 text-xs text-slate-400">{{ t('tools.screenShare.qrCodeHint') }}</p>
              </div>

              <!-- QR Code Canvas -->
              <div v-if="showQr" class="mb-4 flex justify-center">
                <div class="rounded-lg border border-slate-200 bg-white p-2">
                  <canvas ref="qrCanvas" width="128" height="128" />
                </div>
              </div>

              <!-- Alt URLs -->
              <div v-if="altUrls.length > 0" class="mb-4">
                <button @click="showAltUrls = !showAltUrls" class="inline-flex items-center gap-1 text-xs text-slate-400 hover:text-slate-600">
                  <component :is="showAltUrls ? ChevronUp : ChevronDown" class="h-3 w-3" />
                  {{ t('tools.screenShare.altUrls', { n: altUrls.length }) }}
                </button>
                <div v-if="showAltUrls" class="mt-1 space-y-1">
                  <div v-for="url in altUrls" :key="url" class="flex items-center gap-1 rounded bg-slate-100 px-2 py-1 text-xs text-slate-500">
                    <code class="flex-1 truncate">{{ url }}</code>
                    <button @click="copyText(url)" class="shrink-0 text-slate-400 hover:text-slate-600">
                      <Copy class="h-3 w-3" />
                    </button>
                  </div>
                </div>
              </div>

              <!-- Stats Grid -->
              <div class="grid grid-cols-2 gap-3">
                <div class="rounded-lg border border-slate-200 bg-white p-3">
                  <div class="flex items-center gap-2 text-slate-400">
                    <Users class="h-3.5 w-3.5" />
                    <span class="text-xs">{{ t('tools.screenShare.viewerCount') }}</span>
                  </div>
                  <div class="mt-1 text-xl font-bold text-slate-800">{{ status.viewer_count }}</div>
                </div>
                <div class="rounded-lg border border-slate-200 bg-white p-3">
                  <div class="flex items-center gap-2 text-slate-400">
                    <Gauge class="h-3.5 w-3.5" />
                    <span class="text-xs">{{ t('tools.screenShare.actualFps') }}</span>
                  </div>
                  <div class="mt-1 text-xl font-bold text-slate-800">{{ status.fps_actual.toFixed(1) }}</div>
                </div>
                <div class="rounded-lg border border-slate-200 bg-white p-3">
                  <div class="flex items-center gap-2 text-slate-400">
                    <ArrowUpFromLine class="h-3.5 w-3.5" />
                    <span class="text-xs">{{ t('tools.screenShare.bitrate') }}</span>
                  </div>
                  <div class="mt-1 text-xl font-bold text-slate-800">{{ formattedBitrate }}</div>
                </div>
                <div class="rounded-lg border border-slate-200 bg-white p-3">
                  <div class="flex items-center gap-2 text-slate-400">
                    <Clock class="h-3.5 w-3.5" />
                    <span class="text-xs">{{ t('tools.screenShare.uptime') }}</span>
                  </div>
                  <div class="mt-1 text-xl font-bold tabular-nums text-slate-800">{{ formattedUptime }}</div>
                </div>
              </div>
            </template>

            <!-- Idle placeholder -->
            <template v-else>
              <div class="flex flex-1 items-center justify-center">
                <div class="text-center text-slate-300">
                  <MonitorUp class="mx-auto h-12 w-12 mb-2 opacity-40" />
                  <p class="text-sm">{{ t('tools.screenShare.description') }}</p>
                </div>
              </div>
            </template>

            <!-- Logs -->
            <div v-if="logs.length > 0" class="mt-4 border-t border-slate-200 pt-3">
              <h3 class="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-400">{{ t('tools.screenShare.logTitle') }}</h3>
              <div class="max-h-36 space-y-1 overflow-y-auto">
                <div v-for="(log, i) in logs" :key="i" class="flex gap-2 text-xs">
                  <span class="shrink-0 font-mono text-slate-400">{{ log.time }}</span>
                  <span :class="log.level === 'error' ? 'text-red-500' : 'text-slate-600'">{{ log.message }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
