<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  Share2,
  Copy,
  QrCode,
  Plus,
  Trash2,
  Wifi,
  Clock,
  Play,
  Square,
  FolderOpen,
  ChevronDown,
  ChevronUp,
  ExternalLink,
} from 'lucide-vue-next';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import QRCode from 'qrcode';
import {
  fileSharePickDirectory,
  fileShareStart,
  fileShareStop,
  fileShareGetStatus,
  type SharedDir,
  type FileShareConfig,
  type FileShareStatus,
} from '../lib/tauri';

defineOptions({ name: 'FileSharePage' });

const { t } = useI18n();

const sharedDirs = ref<SharedDir[]>([]);
const port = ref(9800);
const passwordEnabled = ref(false);
const password = ref('');

const isActive = ref(false);
const isStarting = ref(false);
const serverUrl = ref('');
const errorMsg = ref('');
const copiedUrl = ref(false);
const showQr = ref(false);
const showAltUrls = ref(false);
const showConnectionDetails = ref(false);

const status = ref<FileShareStatus>({
  is_active: false,
  connection_count: 0,
  uptime_secs: 0,
  server_url: '',
  all_urls: [],
  shared_dirs: [],
  connected_ips: [],
});

const logs = ref<{ level: string; message: string; time: string }[]>([]);
const qrCanvas = ref<HTMLCanvasElement | null>(null);

const formattedUptime = computed(() => {
  const seconds = status.value.uptime_secs;
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  return `${String(hours).padStart(2, '0')}:${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}`;
});

const altUrls = computed(() => (status.value.all_urls || []).filter((url) => url !== serverUrl.value));
const connectedIps = computed(() => status.value.connected_ips || []);
const connectionCount = computed(() => status.value.connection_count ?? connectedIps.value.length);

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
    color: { dark: '#0f766e', light: '#ffffff' },
  });
});

async function pickDirectory() {
  try {
    const directory = await fileSharePickDirectory();
    if (!directory) {
      return;
    }
    if (sharedDirs.value.some((dir) => dir.path === directory.path)) {
      return;
    }
    let alias = directory.alias;
    let counter = 1;
    while (sharedDirs.value.some((dir) => dir.alias === alias)) {
      alias = `${directory.alias}-${counter++}`;
    }
    sharedDirs.value.push({ ...directory, alias });
  } catch (error) {
    errorMsg.value = String(error);
  }
}

function removeDir(index: number) {
  sharedDirs.value.splice(index, 1);
}

async function startShare() {
  errorMsg.value = '';
  isStarting.value = true;
  try {
    const config: FileShareConfig = {
      port: port.value,
      shared_dirs: sharedDirs.value,
      password: passwordEnabled.value && password.value ? password.value : null,
    };
    const url = await fileShareStart(config);
    serverUrl.value = url;
    isActive.value = true;
    showConnectionDetails.value = false;
  } catch (error) {
    errorMsg.value = t('tools.fileShare.errStartFailed', { error: String(error) });
  } finally {
    isStarting.value = false;
  }
}

async function stopShare() {
  try {
    await fileShareStop();
  } catch {}
  isActive.value = false;
  serverUrl.value = '';
  showQr.value = false;
  showConnectionDetails.value = false;
  status.value = {
    is_active: false,
    connection_count: 0,
    uptime_secs: 0,
    server_url: '',
    all_urls: [],
    shared_dirs: [],
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

let unlistenStatus: UnlistenFn | null = null;
let unlistenLog: UnlistenFn | null = null;

onMounted(async () => {
  try {
    const currentStatus = await fileShareGetStatus();
    if (currentStatus.is_active) {
      isActive.value = true;
      serverUrl.value = currentStatus.server_url;
      sharedDirs.value = currentStatus.shared_dirs;
      status.value = currentStatus;
    }
  } catch {}

  unlistenStatus = await listen<FileShareStatus>('file-share-status', (event) => {
    status.value = event.payload;
    if (event.payload.is_active && !isActive.value) {
      isActive.value = true;
      serverUrl.value = event.payload.server_url;
    }
    if (!event.payload.is_active) {
      isActive.value = false;
      serverUrl.value = '';
      showQr.value = false;
      showConnectionDetails.value = false;
    }
  });

  unlistenLog = await listen<{ level: string; message: string }>('file-share-log', (event) => {
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
          <div class="relative flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-teal-500 to-cyan-600 shadow-sm">
            <Share2 class="h-5 w-5 text-white" />
            <span
              v-if="isActive"
              class="absolute -right-1 -top-1 flex h-3.5 w-3.5 items-center justify-center rounded-full border-2 border-white bg-emerald-500"
            >
              <span class="h-1.5 w-1.5 rounded-full bg-white"></span>
            </span>
          </div>
          <div>
            <h1 class="text-2xl font-bold text-slate-900">{{ t('sidebar.fileShare') }}</h1>
            <p class="mt-1 text-sm text-slate-500">
              {{ isActive ? serverUrl : t('tools.fileShare.emptyPlaceholder') }}
            </p>
          </div>
        </div>
        <div
          class="inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-xs font-semibold shadow-sm"
          :class="isActive ? 'border-emerald-200 bg-emerald-50 text-emerald-700' : 'border-slate-200 bg-white text-slate-500'"
        >
          <span class="h-2 w-2 rounded-full" :class="isActive ? 'bg-emerald-500 animate-pulse' : 'bg-slate-300'"></span>
          {{ isActive ? t('tools.fileShare.statusActive') : t('tools.fileShare.statusIdle') }}
        </div>
      </div>

      <div class="grid grid-cols-1 gap-5 lg:grid-cols-5">
        <div class="space-y-4 lg:col-span-3">
          <div class="fs-card">
            <div class="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <p class="fs-section-label !mb-0">{{ t('tools.fileShare.sharedDirs') }}</p>
              <button
                type="button"
                :disabled="isActive"
                @click="pickDirectory"
                class="inline-flex items-center justify-center gap-1.5 rounded-lg border border-teal-200 bg-teal-50 px-3 py-2 text-sm font-semibold text-teal-700 shadow-sm transition hover:border-teal-300 hover:bg-teal-100 disabled:cursor-not-allowed disabled:opacity-40"
              >
                <Plus class="h-4 w-4" />
                {{ t('tools.fileShare.addDir') }}
              </button>
            </div>

            <div
              v-if="sharedDirs.length === 0"
              class="flex flex-col items-center justify-center rounded-xl border border-dashed border-slate-200 bg-slate-50 px-4 py-10 text-center"
            >
              <FolderOpen class="mb-3 h-8 w-8 text-slate-300" />
              <p class="text-sm text-slate-500">{{ t('tools.fileShare.noDirs') }}</p>
            </div>

            <div v-else class="space-y-2">
              <div
                v-for="(dir, index) in sharedDirs"
                :key="dir.path"
                class="group flex items-start gap-3 rounded-xl border border-slate-200 bg-slate-50 px-3.5 py-3 transition hover:border-teal-200 hover:bg-white"
              >
                <div class="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-teal-50 text-teal-600">
                  <FolderOpen class="h-4 w-4" />
                </div>
                <div class="min-w-0 flex-1">
                  <div class="mb-1 flex items-center gap-2">
                    <span class="rounded-md bg-teal-100 px-2 py-0.5 font-mono text-xs font-semibold text-teal-700">
                      {{ dir.alias }}
                    </span>
                  </div>
                  <div class="truncate text-xs text-slate-500" :title="dir.path">{{ dir.path }}</div>
                </div>
                <button
                  v-if="!isActive"
                  type="button"
                  @click="removeDir(index)"
                  class="rounded-lg p-2 text-slate-400 opacity-0 transition hover:bg-red-50 hover:text-red-600 group-hover:opacity-100"
                  :title="t('tools.fileShare.removeDir')"
                >
                  <Trash2 class="h-4 w-4" />
                </button>
              </div>
            </div>
          </div>

          <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
            <div class="fs-card">
              <p class="fs-section-label">{{ t('tools.fileShare.port') }}</p>
              <input
                v-model.number="port"
                type="number"
                min="1024"
                max="65535"
                :disabled="isActive"
                class="fs-input w-full"
              >
              <p class="mt-2 text-xs text-slate-500">{{ t('tools.fileShare.portHint') }}</p>
            </div>

            <div class="fs-card">
              <div class="mb-3 flex items-center justify-between gap-3">
                <p class="fs-section-label !mb-0">{{ t('tools.fileShare.passwordToggle') }}</p>
                <label class="fs-toggle">
                  <input v-model="passwordEnabled" type="checkbox" :disabled="isActive" class="sr-only">
                  <span class="fs-toggle-track" :class="passwordEnabled ? 'bg-teal-600' : 'bg-slate-300'">
                    <span class="fs-toggle-thumb" :class="passwordEnabled ? 'translate-x-4' : 'translate-x-0'"></span>
                  </span>
                </label>
              </div>
              <input
                v-if="passwordEnabled"
                v-model="password"
                type="password"
                :disabled="isActive"
                :placeholder="t('tools.fileShare.passwordInput')"
                class="fs-input w-full"
              >
              <div v-else class="flex h-10 items-center rounded-lg border border-dashed border-slate-200 px-3 text-sm text-slate-400">
                --
              </div>
            </div>
          </div>

          <div v-if="errorMsg" class="rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-600 shadow-sm">
            {{ errorMsg }}
          </div>

          <button
            v-if="!isActive"
            type="button"
            @click="startShare"
            :disabled="isStarting || sharedDirs.length === 0"
            class="w-full rounded-xl bg-gradient-to-r from-teal-600 to-cyan-600 px-6 py-3.5 text-sm font-semibold text-white shadow-sm transition hover:from-teal-700 hover:to-cyan-700 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <span class="flex items-center justify-center gap-2">
              <Play class="h-4 w-4" />
              {{ isStarting ? t('tools.fileShare.starting') : t('tools.fileShare.startShare') }}
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
              {{ t('tools.fileShare.stopShare') }}
            </span>
          </button>
        </div>

        <div class="space-y-4 lg:col-span-2">
          <template v-if="isActive && serverUrl">
            <div class="fs-card">
              <p class="fs-section-label">{{ t('tools.fileShare.accessUrl') }}</p>
              <div class="flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5">
                <code class="flex-1 truncate font-mono text-sm font-semibold text-teal-700">{{ serverUrl }}</code>
                <button
                  type="button"
                  @click="copyUrl"
                  class="fs-icon-button"
                  :title="t('tools.fileShare.copyUrl')"
                  aria-label="Copy URL"
                >
                  <Copy class="h-4 w-4" :class="copiedUrl ? 'text-teal-600' : ''" />
                </button>
                <button
                  type="button"
                  @click="showQr = !showQr"
                  class="fs-icon-button"
                  :title="showQr ? t('tools.fileShare.hideQrCode') : t('tools.fileShare.showQrCode')"
                  aria-label="Toggle QR Code"
                >
                  <QrCode class="h-4 w-4" :class="showQr ? 'text-teal-600' : ''" />
                </button>
                <button type="button" @click="openInBrowser" class="fs-icon-button" title="Open in browser">
                  <ExternalLink class="h-4 w-4" />
                </button>
              </div>
              <p v-if="copiedUrl" class="mt-2 text-xs text-teal-600">{{ t('tools.fileShare.copied') }}</p>
              <p v-else class="mt-2 text-xs text-slate-500">{{ t('tools.fileShare.qrCodeHint') }}</p>

              <div v-if="showQr" class="mt-4 flex justify-center">
                <div class="rounded-xl border border-slate-200 bg-white p-3 shadow-sm">
                  <canvas ref="qrCanvas" width="128" height="128" />
                </div>
              </div>

              <div v-if="altUrls.length > 0" class="mt-4">
                <button type="button" @click="showAltUrls = !showAltUrls" class="fs-inline-button">
                  <component :is="showAltUrls ? ChevronUp : ChevronDown" class="h-3.5 w-3.5" />
                  {{ t('tools.fileShare.altUrls', { n: altUrls.length }) }}
                </button>
                <div v-if="showAltUrls" class="mt-3 space-y-2">
                  <div
                    v-for="url in altUrls"
                    :key="url"
                    class="flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2"
                  >
                    <code class="flex-1 truncate font-mono text-xs text-slate-600">{{ url }}</code>
                    <button type="button" @click="copyText(url)" class="text-slate-400 transition hover:text-teal-600">
                      <Copy class="h-3.5 w-3.5" />
                    </button>
                  </div>
                </div>
              </div>
            </div>

            <div class="grid grid-cols-1 gap-3">
              <div class="fs-stat-card">
                <div class="mb-3 flex items-start justify-between gap-3">
                  <div class="flex items-center gap-2 text-slate-500">
                    <Wifi class="h-4 w-4 text-teal-600" />
                    <span class="text-[11px] font-semibold uppercase tracking-[0.14em]">
                      {{ t('tools.fileShare.connectionCount') }}
                    </span>
                  </div>
                  <button type="button" class="fs-detail-button" @click="showConnectionDetails = !showConnectionDetails">
                    {{ t('tools.fileShare.connectionDetails') }}
                    <component :is="showConnectionDetails ? ChevronUp : ChevronDown" class="h-3.5 w-3.5" />
                  </button>
                </div>
                <div class="font-mono text-3xl font-bold text-slate-900">{{ connectionCount }}</div>
              </div>

              <div class="fs-stat-card">
                <div class="mb-2 flex items-center gap-2 text-slate-500">
                  <Clock class="h-4 w-4 text-teal-600" />
                  <span class="text-[11px] font-semibold uppercase tracking-[0.14em]">{{ t('tools.fileShare.uptime') }}</span>
                </div>
                <div class="font-mono text-2xl font-bold text-slate-900">{{ formattedUptime }}</div>
              </div>
            </div>

            <div v-if="showConnectionDetails" class="fs-card">
              <div class="mb-3">
                <h3 class="text-sm font-semibold text-slate-900">{{ t('tools.fileShare.connectedIpList') }}</h3>
                <p class="text-xs text-slate-500">{{ t('tools.fileShare.connectionCount') }}: {{ connectionCount }}</p>
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
                {{ t('tools.fileShare.noConnections') }}
              </div>
            </div>
          </template>

          <template v-else>
            <div class="flex min-h-56 flex-col items-center justify-center rounded-xl border border-slate-200/80 bg-white p-8 text-center shadow-sm">
              <div class="mb-4 flex h-16 w-16 items-center justify-center rounded-2xl bg-teal-50 text-teal-600">
                <Share2 class="h-7 w-7" />
              </div>
              <p class="text-sm text-slate-500">{{ t('tools.fileShare.emptyPlaceholder') }}</p>
            </div>
          </template>

          <div v-if="logs.length > 0" class="fs-card">
            <h3 class="fs-section-label">{{ t('tools.fileShare.logTitle') }}</h3>
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
.fs-card {
  border: 1px solid rgb(226 232 240 / 0.8);
  border-radius: 0.75rem;
  background: white;
  padding: 1.25rem;
  box-shadow: 0 1px 2px rgb(15 23 42 / 0.06);
}

.fs-stat-card {
  border: 1px solid rgb(226 232 240 / 0.8);
  border-radius: 0.75rem;
  background: white;
  padding: 1rem;
  box-shadow: 0 1px 2px rgb(15 23 42 / 0.06);
}

.fs-section-label {
  margin-bottom: 1rem;
  font-size: 0.7rem;
  font-weight: 700;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: rgb(100 116 139);
}

.fs-input {
  border: 1px solid rgb(203 213 225);
  border-radius: 0.75rem;
  background: white;
  padding: 0.65rem 0.9rem;
  font-size: 0.875rem;
  color: rgb(15 23 42);
  outline: none;
  transition: border-color 0.15s ease, box-shadow 0.15s ease;
}

.fs-input:focus {
  border-color: rgb(13 148 136);
  box-shadow: 0 0 0 3px rgb(13 148 136 / 0.12);
}

.fs-input:disabled {
  cursor: not-allowed;
  background-color: rgb(248 250 252);
  color: rgb(148 163 184);
}

.fs-toggle {
  position: relative;
  display: inline-flex;
}

.fs-toggle-track {
  display: block;
  height: 20px;
  width: 36px;
  flex-shrink: 0;
  border-radius: 9999px;
  transition: background-color 0.2s ease;
}

.fs-toggle-thumb {
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

.fs-icon-button {
  border: 1px solid rgb(226 232 240);
  border-radius: 0.65rem;
  background: white;
  padding: 0.45rem;
  color: rgb(100 116 139);
  transition: border-color 0.15s ease, color 0.15s ease, background-color 0.15s ease;
}

.fs-icon-button:hover {
  border-color: rgb(153 246 228);
  background: rgb(240 253 250);
  color: rgb(13 148 136);
}

.fs-inline-button {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.75rem;
  font-weight: 600;
  color: rgb(100 116 139);
  transition: color 0.15s ease;
}

.fs-inline-button:hover {
  color: rgb(13 148 136);
}

.fs-detail-button {
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

.fs-detail-button:hover {
  border-color: rgb(153 246 228);
  background: rgb(240 253 250);
  color: rgb(13 148 136);
}
</style>
