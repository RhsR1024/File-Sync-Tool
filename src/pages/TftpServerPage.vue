<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  Check,
  Clipboard,
  FileBox,
  FileUp,
  FolderOpen,
  LoaderCircle,
  Play,
  RadioTower,
  RefreshCw,
  Server,
  ShieldAlert,
  Square,
} from 'lucide-vue-next';

import { pushToast } from '@/composables/useToast';
import { setToolRuntime } from '@/lib/store';
import { buildTftpCommand, type TftpCommandMode } from '@/lib/tftpCommands';
import {
  screenShareListInterfaces,
  tftpServerGetStatus,
  tftpServerListFiles,
  tftpServerPickDirectory,
  tftpServerPickFile,
  tftpServerStart,
  tftpServerStop,
  type NetworkInterfaceInfo,
  type TftpEvent,
  type TftpServerConfig,
  type TftpServerStatus,
  type TftpSharedFile,
} from '@/lib/tauri';

defineOptions({ name: 'TftpServerPage' });

const STORAGE_KEY = 'tftp_server_config_v2';
const LEGACY_STORAGE_KEY = 'tftp_server_config_v1';
const { t } = useI18n();

const defaultConfig = (): TftpServerConfig => ({
  root_dir: '',
  bind_address: '0.0.0.0',
  port: 69,
  allow_upload: true,
  allow_overwrite: true,
  block_size_limit: 8192,
  window_size_limit: 8,
});

const defaultStatus = (): TftpServerStatus => ({
  ...defaultConfig(),
  allow_upload: false,
  allow_overwrite: false,
  is_active: false,
  uptime_secs: 0,
  active_transfers: [],
  events: [],
  stats: {
    completed_downloads: 0,
    completed_uploads: 0,
    bytes_sent: 0,
    bytes_received: 0,
  },
  last_error: null,
});

function loadStoredConfig(): TftpServerConfig {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      const parsed = { ...defaultConfig(), ...JSON.parse(stored) };
      if (parsed.block_size_limit === 1468) parsed.block_size_limit = 8192;
      return parsed;
    }
    const legacy = localStorage.getItem(LEGACY_STORAGE_KEY);
    if (legacy) {
      // v1 kept -pl receive and overwrite opt-in. Carry the directory and network
      // settings over, but adopt the new defaults for those two switches.
      const parsed = {
        ...defaultConfig(),
        ...JSON.parse(legacy),
        allow_upload: true,
        allow_overwrite: true,
      };
      if (parsed.block_size_limit === 1468) parsed.block_size_limit = 8192;
      localStorage.removeItem(LEGACY_STORAGE_KEY);
      return parsed;
    }
  } catch {
    // Ignore invalid local settings and keep safe defaults.
  }
  return defaultConfig();
}

const config = ref<TftpServerConfig>(loadStoredConfig());
const status = ref<TftpServerStatus>(defaultStatus());
const interfaces = ref<NetworkInterfaceInfo[]>([]);
const files = ref<TftpSharedFile[]>([]);
const fileName = ref('firmware.bin');
const commandMode = ref<TftpCommandMode>('download');
const commandIp = ref('');
const isLoading = ref(true);
const isStarting = ref(false);
const isStopping = ref(false);
const isPickingDirectory = ref(false);
const isPickingFile = ref(false);
const isRefreshingFiles = ref(false);
const copiedCommand = ref<string | null>(null);
let pollTimer: ReturnType<typeof setInterval> | null = null;

const isBusy = computed(() => isStarting.value || isStopping.value);
const ipv4Interfaces = computed(() => interfaces.value.filter((item) => item.ip.includes('.')));
const endpointIps = computed(() => {
  if (config.value.bind_address !== '0.0.0.0') return [config.value.bind_address];
  const addresses = ipv4Interfaces.value
    .map((item) => item.ip)
    .filter((ip) => ip !== '127.0.0.1');
  return addresses.length > 0 ? [...new Set(addresses)] : ['127.0.0.1'];
});
const effectiveCommandIp = computed(() => commandIp.value || endpointIps.value[0] || '<PC_IP>');
const activePort = computed(() => status.value.is_active ? status.value.port : config.value.port);
const visibleEvents = computed(() => [...status.value.events].reverse());
const uptime = computed(() => formatDuration(status.value.uptime_secs));
const generatedCommand = computed(() => buildTftpCommand({
  mode: commandMode.value,
  fileName: fileName.value,
  serverIp: effectiveCommandIp.value,
  blockSize: config.value.block_size_limit,
}));

watch(config, (value) => {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
  if (!value.allow_upload) value.allow_overwrite = false;
}, { deep: true });

watch(endpointIps, (addresses) => {
  if (!addresses.includes(commandIp.value)) commandIp.value = addresses[0] ?? '';
}, { immediate: true });

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value.toFixed(index === 0 || value >= 100 ? 0 : 1)} ${units[index]}`;
}

function formatDuration(seconds: number): string {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remaining = seconds % 60;
  return [hours, minutes, remaining].map((value) => String(value).padStart(2, '0')).join(':');
}

function formatTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  // The log survives restarts, so date-stamp anything that is not from today.
  const isToday = date.toDateString() === new Date().toDateString();
  if (isToday) return date.toLocaleTimeString();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  const hours = String(date.getHours()).padStart(2, '0');
  const minutes = String(date.getMinutes()).padStart(2, '0');
  return `${month}-${day} ${hours}:${minutes}`;
}

function eventLabel(event: TftpEvent): string {
  const key = `tools.tftpServer.events.${event.action}`;
  return t(key, {
    file: event.file_name ?? '',
    client: event.client ?? '',
    bytes: formatBytes(event.bytes),
    message: event.message,
  });
}

async function refreshStatus(showError = false) {
  try {
    const next = await tftpServerGetStatus();
    status.value = next;
    setToolRuntime('tftpServer', next.is_active);
    if (next.is_active) {
      config.value = {
        root_dir: next.root_dir,
        bind_address: next.bind_address,
        port: next.port,
        allow_upload: next.allow_upload,
        allow_overwrite: next.allow_overwrite,
        block_size_limit: next.block_size_limit,
        window_size_limit: next.window_size_limit,
      };
    }
  } catch (error) {
    if (showError) pushToast(errorMessage(error), 'error');
  }
}

async function refreshFiles(showError = false) {
  if (!config.value.root_dir.trim()) {
    files.value = [];
    return;
  }
  isRefreshingFiles.value = true;
  try {
    files.value = await tftpServerListFiles(config.value.root_dir);
  } catch (error) {
    files.value = [];
    if (showError) pushToast(errorMessage(error), 'error');
  } finally {
    isRefreshingFiles.value = false;
  }
}

async function pickDirectory() {
  isPickingDirectory.value = true;
  try {
    const selected = await tftpServerPickDirectory();
    if (selected) {
      config.value.root_dir = selected;
      await refreshFiles(true);
    }
  } catch (error) {
    pushToast(errorMessage(error), 'error');
  } finally {
    isPickingDirectory.value = false;
  }
}

async function pickLocalFile() {
  isPickingFile.value = true;
  try {
    const selected = await tftpServerPickFile();
    if (selected) {
      config.value.root_dir = selected.rootDir;
      fileName.value = selected.fileName;
      await refreshFiles(true);
    }
  } catch (error) {
    pushToast(errorMessage(error), 'error');
  } finally {
    isPickingFile.value = false;
  }
}

async function startServer() {
  if (!config.value.root_dir.trim()) {
    pushToast(t('tools.tftpServer.validation.rootRequired'), 'error');
    return;
  }
  if (config.value.port < 1 || config.value.port > 65535) {
    pushToast(t('tools.tftpServer.validation.portRange'), 'error');
    return;
  }
  isStarting.value = true;
  try {
    status.value = await tftpServerStart({ ...config.value });
    config.value.port = status.value.port;
    setToolRuntime('tftpServer', true);
    pushToast(t('tools.tftpServer.started'), 'success');
  } catch (error) {
    pushToast(t('tools.tftpServer.startFailed', { error: errorMessage(error) }), 'error');
  } finally {
    isStarting.value = false;
  }
}

async function stopServer() {
  isStopping.value = true;
  try {
    status.value = await tftpServerStop();
    setToolRuntime('tftpServer', false);
    pushToast(t('tools.tftpServer.stopped'), 'success');
  } catch (error) {
    pushToast(t('tools.tftpServer.stopFailed', { error: errorMessage(error) }), 'error');
  } finally {
    isStopping.value = false;
  }
}

async function copyCommand(command: string) {
  try {
    await navigator.clipboard.writeText(command);
    copiedCommand.value = command;
    window.setTimeout(() => {
      if (copiedCommand.value === command) copiedCommand.value = null;
    }, 1500);
  } catch (error) {
    pushToast(errorMessage(error), 'error');
  }
}

function useSharedFile(file: TftpSharedFile) {
  fileName.value = file.relative_path;
}

function selectCommandMode(mode: TftpCommandMode) {
  if (mode === 'upload' && !status.value.is_active) {
    config.value.allow_upload = true;
  }
  commandMode.value = mode;
}

onMounted(async () => {
  const interfacesPromise = screenShareListInterfaces()
    .then((items) => {
      interfaces.value = items;
    })
    .catch((error) => {
      pushToast(errorMessage(error), 'error');
    });
  await Promise.all([interfacesPromise, refreshStatus(true)]);
  await refreshFiles(false);
  isLoading.value = false;
  pollTimer = setInterval(() => void refreshStatus(false), 1000);
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
});
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-slate-50">
    <div class="mx-auto flex w-full max-w-7xl flex-col gap-5 px-5 py-5 pb-10 lg:px-7">
      <header class="flex flex-col gap-4 border-b border-slate-200 pb-5 sm:flex-row sm:items-center sm:justify-between">
        <div class="flex min-w-0 items-center gap-3">
          <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-cyan-500 to-teal-600 shadow-sm">
            <RadioTower class="h-5 w-5 text-white" aria-hidden="true" />
          </div>
          <div class="min-w-0">
            <div class="flex flex-wrap items-center gap-2">
              <h1 class="text-2xl font-bold text-slate-900">{{ t('tools.tftpServer.title') }}</h1>
              <span
                class="inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-xs font-semibold"
                :class="status.is_active ? 'border-emerald-200 bg-emerald-50 text-emerald-700' : 'border-slate-200 bg-white text-slate-500'"
              >
                <span class="h-1.5 w-1.5 rounded-full" :class="status.is_active ? 'bg-emerald-500' : 'bg-slate-400'"></span>
                {{ status.is_active ? t('tools.tftpServer.active') : t('tools.tftpServer.idle') }}
              </span>
            </div>
            <p class="mt-1 text-sm text-slate-500">{{ t('tools.tftpServer.description') }}</p>
          </div>
        </div>

        <div class="flex shrink-0 items-center gap-2">
          <button
            type="button"
            class="inline-flex h-10 w-10 cursor-pointer items-center justify-center rounded-md border border-slate-200 bg-white text-slate-600 transition-colors hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-500 disabled:cursor-not-allowed disabled:opacity-50"
            :aria-label="t('tools.tftpServer.refreshStatus')"
            :title="t('tools.tftpServer.refreshStatus')"
            :disabled="isLoading"
            @click="refreshStatus(true)"
          >
            <RefreshCw class="h-4 w-4" :class="isLoading ? 'animate-spin motion-reduce:animate-none' : ''" aria-hidden="true" />
          </button>
          <button
            v-if="!status.is_active"
            type="button"
            class="inline-flex h-10 cursor-pointer items-center gap-2 rounded-md bg-cyan-700 px-4 text-sm font-semibold text-white transition-colors hover:bg-cyan-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-500 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="isBusy || isLoading"
            @click="startServer"
          >
            <LoaderCircle v-if="isStarting" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
            <Play v-else class="h-4 w-4" aria-hidden="true" />
            {{ isStarting ? t('tools.tftpServer.starting') : t('tools.tftpServer.start') }}
          </button>
          <button
            v-else
            type="button"
            class="inline-flex h-10 cursor-pointer items-center gap-2 rounded-md bg-rose-600 px-4 text-sm font-semibold text-white transition-colors hover:bg-rose-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-rose-500 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="isBusy"
            @click="stopServer"
          >
            <LoaderCircle v-if="isStopping" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
            <Square v-else class="h-4 w-4" aria-hidden="true" />
            {{ isStopping ? t('tools.tftpServer.stopping') : t('tools.tftpServer.stop') }}
          </button>
        </div>
      </header>

      <section class="grid grid-cols-2 gap-3 lg:grid-cols-4" :aria-label="t('tools.tftpServer.runtime')">
        <div class="rounded-xl border border-slate-200 bg-white p-4 transition-colors hover:border-cyan-200">
          <div class="flex items-center gap-2 text-xs font-semibold text-slate-500"><ArrowDownToLine class="h-4 w-4 text-cyan-600" />{{ t('tools.tftpServer.downloads') }}</div>
          <div class="mt-2 text-2xl font-bold tabular-nums text-slate-950">{{ status.stats.completed_downloads }}</div>
          <div class="mt-1 text-xs text-slate-500">{{ formatBytes(status.stats.bytes_sent) }}</div>
        </div>
        <div class="rounded-xl border border-slate-200 bg-white p-4 transition-colors hover:border-cyan-200">
          <div class="flex items-center gap-2 text-xs font-semibold text-slate-500"><ArrowUpFromLine class="h-4 w-4 text-blue-600" />{{ t('tools.tftpServer.uploads') }}</div>
          <div class="mt-2 text-2xl font-bold tabular-nums text-slate-950">{{ status.stats.completed_uploads }}</div>
          <div class="mt-1 text-xs text-slate-500">{{ formatBytes(status.stats.bytes_received) }}</div>
        </div>
        <div class="rounded-xl border border-slate-200 bg-white p-4 transition-colors hover:border-cyan-200">
          <div class="flex items-center gap-2 text-xs font-semibold text-slate-500"><Server class="h-4 w-4 text-emerald-600" />{{ t('tools.tftpServer.activeTransfers') }}</div>
          <div class="mt-2 text-2xl font-bold tabular-nums text-slate-950">{{ status.active_transfers.length }}</div>
          <div class="mt-1 text-xs text-slate-500">UDP {{ activePort }}</div>
        </div>
        <div class="rounded-xl border border-slate-200 bg-white p-4 transition-colors hover:border-cyan-200">
          <div class="flex items-center gap-2 text-xs font-semibold text-slate-500"><RadioTower class="h-4 w-4 text-violet-600" />{{ t('tools.tftpServer.uptime') }}</div>
          <div class="mt-2 font-mono text-2xl font-bold text-slate-950">{{ uptime }}</div>
          <div class="mt-1 truncate text-xs text-slate-500">{{ status.is_active ? `${status.bind_address}:${status.port}` : t('tools.tftpServer.notListening') }}</div>
        </div>
      </section>

      <div v-if="status.last_error" class="flex items-start gap-3 rounded-lg border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800" role="alert">
        <ShieldAlert class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
        <span>{{ status.last_error }}</span>
      </div>

      <div class="grid gap-5 xl:grid-cols-[minmax(0,1.05fr)_minmax(380px,0.95fr)]">
        <section class="min-w-0 rounded-2xl border border-slate-200 bg-white p-5" :aria-labelledby="'tftp-config-title'">
          <div class="mb-5 flex items-center justify-between gap-3">
            <div>
              <h2 id="tftp-config-title" class="text-base font-bold text-slate-950">{{ t('tools.tftpServer.configuration') }}</h2>
              <p class="mt-1 text-xs text-slate-500">{{ t('tools.tftpServer.configurationHint') }}</p>
            </div>
          </div>

          <div class="space-y-4">
            <div>
              <label for="tftp-root" class="mb-1.5 block text-sm font-semibold text-slate-700">{{ t('tools.tftpServer.rootDir') }}</label>
              <div class="flex gap-2">
                <input
                  id="tftp-root"
                  v-model="config.root_dir"
                  type="text"
                  class="h-10 min-w-0 flex-1 rounded-md border border-slate-300 bg-white px-3 text-sm text-slate-900 outline-none transition focus:border-cyan-500 focus:ring-2 focus:ring-cyan-500/20 disabled:bg-slate-100 disabled:text-slate-500"
                  :placeholder="t('tools.tftpServer.rootPlaceholder')"
                  :disabled="status.is_active"
                  @change="refreshFiles(false)"
                />
                <button
                  type="button"
                  class="inline-flex h-10 w-10 shrink-0 cursor-pointer items-center justify-center rounded-md border border-slate-300 bg-white text-slate-700 transition-colors hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-500 disabled:cursor-not-allowed disabled:opacity-50"
                  :disabled="status.is_active || isPickingDirectory"
                  :aria-label="t('tools.tftpServer.pickDirectory')"
                  :title="t('tools.tftpServer.pickDirectory')"
                  @click="pickDirectory"
                >
                  <LoaderCircle v-if="isPickingDirectory" class="h-4 w-4 animate-spin motion-reduce:animate-none" />
                  <FolderOpen v-else class="h-4 w-4" />
                </button>
              </div>
            </div>

            <div class="grid gap-4 sm:grid-cols-[minmax(0,1fr)_140px]">
              <div>
                <label for="tftp-bind" class="mb-1.5 block text-sm font-semibold text-slate-700">{{ t('tools.tftpServer.bindAddress') }}</label>
                <select id="tftp-bind" v-model="config.bind_address" class="h-10 w-full cursor-pointer rounded-md border border-slate-300 bg-white px-3 text-sm text-slate-900 outline-none focus:border-cyan-500 focus:ring-2 focus:ring-cyan-500/20 disabled:cursor-not-allowed disabled:bg-slate-100" :disabled="status.is_active">
                  <option value="0.0.0.0">{{ t('tools.tftpServer.allInterfaces') }}</option>
                  <option v-for="item in ipv4Interfaces" :key="`${item.name}-${item.ip}`" :value="item.ip">{{ item.name }} · {{ item.ip }}</option>
                </select>
              </div>
              <div>
                <label for="tftp-port" class="mb-1.5 block text-sm font-semibold text-slate-700">{{ t('tools.tftpServer.port') }}</label>
                <input id="tftp-port" v-model.number="config.port" type="number" min="1" max="65535" class="h-10 w-full rounded-md border border-slate-300 bg-white px-3 text-sm text-slate-900 outline-none focus:border-cyan-500 focus:ring-2 focus:ring-cyan-500/20 disabled:bg-slate-100" :disabled="status.is_active" />
              </div>
            </div>

            <div class="grid gap-3 sm:grid-cols-2">
              <label class="flex min-h-16 cursor-pointer items-center justify-between gap-3 rounded-lg border border-slate-200 px-3.5 py-3 transition-colors hover:bg-slate-50" :class="status.is_active ? 'cursor-not-allowed opacity-60' : ''">
                <span>
                  <span class="block text-sm font-semibold text-slate-800">{{ t('tools.tftpServer.allowUpload') }}</span>
                  <span class="mt-0.5 block text-xs text-slate-500">WRQ</span>
                </span>
                <input v-model="config.allow_upload" type="checkbox" class="peer sr-only" :disabled="status.is_active" />
                <span class="relative h-6 w-11 shrink-0 rounded-full bg-slate-300 transition-colors peer-checked:bg-cyan-600 peer-focus-visible:ring-2 peer-focus-visible:ring-cyan-500 peer-focus-visible:ring-offset-2 after:absolute after:left-1 after:top-1 after:h-4 after:w-4 after:rounded-full after:bg-white after:transition-transform peer-checked:after:translate-x-5"></span>
              </label>
              <label class="flex min-h-16 cursor-pointer items-center gap-3 rounded-lg border border-slate-200 px-3.5 py-3 transition-colors hover:bg-slate-50" :class="status.is_active || !config.allow_upload ? 'cursor-not-allowed opacity-60' : ''">
                <input v-model="config.allow_overwrite" type="checkbox" class="h-4 w-4 cursor-pointer rounded border-slate-300 text-cyan-600 focus:ring-cyan-500 disabled:cursor-not-allowed" :disabled="status.is_active || !config.allow_upload" />
                <span>
                  <span class="block text-sm font-semibold text-slate-800">{{ t('tools.tftpServer.allowOverwrite') }}</span>
                  <span class="mt-0.5 block text-xs text-slate-500">{{ t('tools.tftpServer.allowOverwriteHint') }}</span>
                </span>
              </label>
            </div>

          </div>
        </section>

        <section class="min-w-0 rounded-2xl border border-slate-200 bg-white p-5" :aria-labelledby="'tftp-command-title'">
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h2 id="tftp-command-title" class="text-base font-bold text-slate-950">{{ t('tools.tftpServer.deviceCommands') }}</h2>
              <p class="mt-1 text-xs text-slate-500">{{ t('tools.tftpServer.deviceCommandsHint') }}</p>
            </div>
            <div class="inline-flex rounded-md border border-slate-200 bg-slate-100 p-1" role="group" :aria-label="t('tools.tftpServer.commandMode')">
              <button type="button" class="inline-flex h-8 cursor-pointer items-center gap-1.5 rounded px-3 text-xs font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-500" :class="commandMode === 'download' ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-500 hover:text-slate-800'" @click="selectCommandMode('download')">
                <ArrowDownToLine class="h-3.5 w-3.5" />{{ t('tools.tftpServer.download') }}
              </button>
              <button type="button" class="inline-flex h-8 cursor-pointer items-center gap-1.5 rounded px-3 text-xs font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-500 disabled:cursor-not-allowed disabled:opacity-50" :class="commandMode === 'upload' ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-500 hover:text-slate-800'" :disabled="status.is_active && !config.allow_upload" @click="selectCommandMode('upload')">
                <ArrowUpFromLine class="h-3.5 w-3.5" />{{ t('tools.tftpServer.upload') }}
              </button>
            </div>
          </div>

          <div class="mt-5 grid gap-3" :class="commandMode === 'download' ? 'sm:grid-cols-3' : 'sm:grid-cols-2'">
            <div>
              <label for="tftp-command-ip" class="mb-1.5 block text-xs font-semibold text-slate-600">{{ t('tools.tftpServer.pcIp') }}</label>
              <select id="tftp-command-ip" v-model="commandIp" class="h-10 w-full cursor-pointer rounded-md border border-slate-300 bg-white px-3 text-sm outline-none focus:border-cyan-500 focus:ring-2 focus:ring-cyan-500/20">
                <option v-for="ip in endpointIps" :key="ip" :value="ip">{{ ip }}</option>
              </select>
            </div>
            <div>
              <label for="tftp-command-file" class="mb-1.5 block text-xs font-semibold text-slate-600">{{ t('tools.tftpServer.fileName') }}</label>
              <div class="flex min-w-0 gap-2">
                <input id="tftp-command-file" v-model="fileName" type="text" class="h-10 min-w-0 flex-1 rounded-md border border-slate-300 px-3 font-mono text-sm outline-none focus:border-cyan-500 focus:ring-2 focus:ring-cyan-500/20" />
                <button
                  v-if="commandMode === 'download'"
                  type="button"
                  class="inline-flex h-10 w-10 shrink-0 cursor-pointer items-center justify-center rounded-md border border-slate-300 bg-white text-slate-700 transition-colors hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-500 disabled:cursor-not-allowed disabled:opacity-50"
                  :disabled="status.is_active || isPickingFile"
                  :aria-label="t('tools.tftpServer.pickLocalFile')"
                  :title="t('tools.tftpServer.pickLocalFile')"
                  @click="pickLocalFile"
                >
                  <LoaderCircle v-if="isPickingFile" class="h-4 w-4 animate-spin motion-reduce:animate-none" />
                  <FileUp v-else class="h-4 w-4" />
                </button>
              </div>
              <p class="mt-1 text-[11px] leading-4 text-slate-500">
                {{ commandMode === 'download' ? t('tools.tftpServer.localFileHint') : t('tools.tftpServer.deviceFileHint') }}
              </p>
            </div>
            <div v-if="commandMode === 'download'">
              <label for="tftp-command-block-size" class="mb-1.5 block text-xs font-semibold text-slate-600">{{ t('tools.tftpServer.blockSize') }}</label>
              <input id="tftp-command-block-size" v-model.number="config.block_size_limit" type="number" min="512" max="65464" class="h-10 w-full rounded-md border border-slate-300 px-3 font-mono text-sm outline-none focus:border-cyan-500 focus:ring-2 focus:ring-cyan-500/20 disabled:bg-slate-100" :disabled="status.is_active" />
            </div>
          </div>

          <div class="mt-4">
            <div class="group flex min-w-0 items-start gap-3 rounded-lg border border-slate-200 bg-slate-950 px-3 py-3 text-slate-100">
              <span class="shrink-0 pt-1.5 text-xs font-semibold text-slate-400">{{ t('tools.tftpServer.runOnDevice') }}</span>
              <code class="min-w-0 flex-1 whitespace-pre-wrap break-all py-1 font-mono text-xs leading-5">{{ generatedCommand }}</code>
              <button type="button" class="inline-flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded text-slate-400 transition-colors hover:bg-slate-800 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-400" :aria-label="t('tools.tftpServer.copyCommand')" :title="t('tools.tftpServer.copyCommand')" @click="copyCommand(generatedCommand)">
                <Check v-if="copiedCommand === generatedCommand" class="h-4 w-4 text-emerald-400" />
                <Clipboard v-else class="h-4 w-4" />
              </button>
            </div>
          </div>

          <div class="mt-4 flex items-start gap-2.5 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2.5 text-xs leading-5 text-amber-900">
            <ShieldAlert class="mt-0.5 h-4 w-4 shrink-0" />
            <span>{{ t('tools.tftpServer.firewallHint', { port: activePort }) }}</span>
          </div>
        </section>
      </div>

      <div class="grid gap-5 xl:grid-cols-[minmax(340px,0.8fr)_minmax(0,1.2fr)]">
        <section class="min-w-0 rounded-2xl border border-slate-200 bg-white" :aria-labelledby="'tftp-files-title'">
          <div class="flex items-center justify-between gap-3 border-b border-slate-200 px-4 py-3.5">
            <div class="min-w-0">
              <h2 id="tftp-files-title" class="text-sm font-bold text-slate-900">{{ t('tools.tftpServer.sharedFiles') }}</h2>
              <p class="mt-0.5 truncate text-xs text-slate-500">{{ config.root_dir || t('tools.tftpServer.noRoot') }}</p>
            </div>
            <button type="button" class="inline-flex h-9 w-9 shrink-0 cursor-pointer items-center justify-center rounded-md border border-slate-200 text-slate-600 hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-500 disabled:cursor-not-allowed disabled:opacity-50" :disabled="!config.root_dir || isRefreshingFiles" :aria-label="t('tools.tftpServer.refreshFiles')" :title="t('tools.tftpServer.refreshFiles')" @click="refreshFiles(true)">
              <RefreshCw class="h-4 w-4" :class="isRefreshingFiles ? 'animate-spin motion-reduce:animate-none' : ''" />
            </button>
          </div>
          <div class="max-h-80 overflow-auto">
            <button v-for="file in files" :key="file.relative_path" type="button" class="flex min-h-12 w-full cursor-pointer items-center gap-3 border-b border-slate-100 px-4 py-2 text-left transition-colors last:border-b-0 hover:bg-cyan-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-cyan-500" @click="useSharedFile(file)">
              <FileBox class="h-4 w-4 shrink-0 text-cyan-600" />
              <span class="min-w-0 flex-1 truncate font-mono text-xs text-slate-800" :title="file.relative_path">{{ file.relative_path }}</span>
              <span class="shrink-0 text-xs tabular-nums text-slate-500">{{ formatBytes(file.size) }}</span>
            </button>
            <div v-if="files.length === 0" class="flex min-h-32 flex-col items-center justify-center px-4 text-center text-sm text-slate-500">
              <FileBox class="mb-2 h-6 w-6 text-slate-300" />
              {{ config.root_dir ? t('tools.tftpServer.noFiles') : t('tools.tftpServer.selectRootFirst') }}
            </div>
          </div>
        </section>

        <section class="min-w-0 rounded-2xl border border-slate-200 bg-white" :aria-labelledby="'tftp-events-title'">
          <div class="flex items-center justify-between gap-3 border-b border-slate-200 px-4 py-3.5">
            <div>
              <h2 id="tftp-events-title" class="text-sm font-bold text-slate-900">{{ t('tools.tftpServer.transferLog') }}</h2>
              <p class="mt-0.5 text-xs text-slate-500">{{ t('tools.tftpServer.transferLogHint') }}</p>
            </div>
            <span class="rounded-full bg-slate-100 px-2 py-0.5 text-xs font-semibold text-slate-600">{{ status.events.length }}</span>
          </div>
          <div class="max-h-80 overflow-auto">
            <div v-for="event in visibleEvents" :key="event.id" class="grid min-h-12 grid-cols-[84px_18px_minmax(0,1fr)_auto] items-center gap-2 border-b border-slate-100 px-4 py-2 text-xs last:border-b-0">
              <span class="font-mono tabular-nums text-slate-400">{{ formatTime(event.timestamp) }}</span>
              <span class="h-2 w-2 rounded-full" :class="event.level === 'success' ? 'bg-emerald-500' : event.level === 'error' ? 'bg-rose-500' : 'bg-cyan-500'"></span>
              <span class="min-w-0 truncate text-slate-700" :title="event.message || eventLabel(event)">{{ eventLabel(event) }}</span>
              <span class="max-w-36 truncate font-mono text-slate-400" :title="event.client ?? ''">{{ event.client ?? '' }}</span>
            </div>
            <div v-if="visibleEvents.length === 0" class="flex min-h-32 items-center justify-center px-4 text-sm text-slate-500">{{ t('tools.tftpServer.noEvents') }}</div>
          </div>
        </section>
      </div>
    </div>
  </div>
</template>
