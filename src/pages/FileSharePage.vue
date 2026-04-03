<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { Share2, Copy, QrCode, Plus, Trash2, Wifi, Clock, Play, Square, FolderOpen, ChevronDown, ChevronUp } from 'lucide-vue-next';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
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

// ─── State ───
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

const status = ref<FileShareStatus>({
  is_active: false,
  download_count: 0,
  uptime_secs: 0,
  server_url: '',
  all_urls: [],
  shared_dirs: [],
});

const logs = ref<{ level: string; message: string; time: string }[]>([]);
const qrCanvas = ref<HTMLCanvasElement | null>(null);

// ─── Computed ───
const formattedUptime = computed(() => {
  const s = status.value.uptime_secs;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
});

const altUrls = computed(() => (status.value.all_urls || []).filter((u) => u !== serverUrl.value));

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

// ─── Actions ───
async function pickDirectory() {
  try {
    const dir = await fileSharePickDirectory();
    if (dir) {
      // Deduplicate by path
      if (!sharedDirs.value.some((d) => d.path === dir.path)) {
        // Ensure alias is unique
        let alias = dir.alias;
        let counter = 1;
        while (sharedDirs.value.some((d) => d.alias === alias)) {
          alias = `${dir.alias}-${counter++}`;
        }
        sharedDirs.value.push({ ...dir, alias });
      }
    }
  } catch (e) {
    errorMsg.value = String(e);
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
  } catch (e: any) {
    errorMsg.value = t('tools.fileShare.errStartFailed', { error: String(e) });
  } finally {
    isStarting.value = false;
  }
}

async function stopShare() {
  try {
    await fileShareStop();
  } catch (_) {
    // ignore
  }
  isActive.value = false;
  serverUrl.value = '';
  showQr.value = false;
  status.value = {
    is_active: false,
    download_count: 0,
    uptime_secs: 0,
    server_url: '',
    all_urls: [],
    shared_dirs: [],
  };
}

async function copyUrl() {
  try {
    await navigator.clipboard.writeText(serverUrl.value);
    copiedUrl.value = true;
    setTimeout(() => (copiedUrl.value = false), 1800);
  } catch (_) {
    // ignore
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

// ─── Event Listeners ───
let unlistenStatus: UnlistenFn | null = null;
let unlistenLog: UnlistenFn | null = null;

onMounted(async () => {
  try {
    const s = await fileShareGetStatus();
    if (s.is_active) {
      isActive.value = true;
      serverUrl.value = s.server_url;
      sharedDirs.value = s.shared_dirs;
      status.value = s;
    }
  } catch (_) {}

  unlistenStatus = await listen<FileShareStatus>('file-share-status', (event) => {
    status.value = event.payload;
    if (event.payload.is_active && !isActive.value) {
      isActive.value = true;
      serverUrl.value = event.payload.server_url;
    }
    // Server stopped unexpectedly (e.g. crash) — sync frontend state
    if (!event.payload.is_active && isActive.value) {
      isActive.value = false;
      serverUrl.value = '';
      showQr.value = false;
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
    <div class="mx-auto w-full max-w-6xl space-y-5 p-6 pb-10">

      <!-- Header -->
      <div class="flex items-center gap-3">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-br from-cyan-500 to-teal-600 shadow-sm">
          <Share2 class="h-5 w-5 text-white" />
        </div>
        <div>
          <h1 class="text-2xl font-bold text-slate-900">{{ t('sidebar.fileShare') }}</h1>
        </div>
      </div>

      <!-- Main Card -->
      <div class="overflow-hidden rounded-xl border border-slate-200/80 bg-white shadow-sm">
        <div class="grid grid-cols-1 divide-y lg:grid-cols-5 lg:divide-x lg:divide-y-0">

          <!-- Left: Config -->
          <div class="col-span-3 space-y-5 p-5">

            <!-- Shared Dirs -->
            <div>
              <div class="mb-2 flex items-center justify-between">
                <label class="text-sm font-medium text-slate-700">{{ t('tools.fileShare.sharedDirs') }}</label>
                <button
                  type="button"
                  :disabled="isActive"
                  @click="pickDirectory"
                  class="inline-flex items-center gap-1.5 rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-xs font-semibold text-slate-700 transition hover:bg-slate-50 disabled:opacity-50"
                >
                  <Plus class="h-3.5 w-3.5" />
                  {{ t('tools.fileShare.addDir') }}
                </button>
              </div>

              <!-- Empty state -->
              <div
                v-if="sharedDirs.length === 0"
                class="flex items-center gap-3 rounded-lg border border-dashed border-slate-300 bg-slate-50 px-4 py-5 text-sm text-slate-400"
              >
                <FolderOpen class="h-5 w-5 shrink-0 text-slate-300" />
                {{ t('tools.fileShare.noDirs') }}
              </div>

              <!-- Dir list -->
              <div v-else class="space-y-2">
                <div
                  v-for="(dir, i) in sharedDirs"
                  :key="dir.path"
                  class="flex items-start gap-3 rounded-lg border border-slate-200 bg-slate-50 px-3 py-2.5"
                >
                  <FolderOpen class="mt-0.5 h-4 w-4 shrink-0 text-cyan-500" />
                  <div class="min-w-0 flex-1">
                    <div class="flex items-center gap-2">
                      <span class="text-xs font-semibold text-slate-500 uppercase tracking-wide">{{ t('tools.fileShare.aliasLabel') }}</span>
                      <span class="rounded bg-cyan-50 px-1.5 py-0.5 text-xs font-mono font-bold text-cyan-700 border border-cyan-200">{{ dir.alias }}</span>
                    </div>
                    <div class="mt-0.5 truncate text-xs text-slate-500" :title="dir.path">{{ dir.path }}</div>
                  </div>
                  <button
                    v-if="!isActive"
                    type="button"
                    @click="removeDir(i)"
                    class="shrink-0 rounded p-1 text-slate-400 transition hover:bg-red-50 hover:text-red-500"
                    :title="t('tools.fileShare.removeDir')"
                  >
                    <Trash2 class="h-3.5 w-3.5" />
                  </button>
                </div>
              </div>
            </div>

            <!-- Port + Password -->
            <div class="grid grid-cols-2 gap-4">
              <div>
                <label class="mb-1.5 block text-sm font-medium text-slate-700">{{ t('tools.fileShare.port') }}</label>
                <input
                  v-model.number="port"
                  type="number"
                  min="1024"
                  max="65535"
                  :disabled="isActive"
                  class="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm text-slate-800 transition focus:border-cyan-500 focus:outline-none focus:ring-1 focus:ring-cyan-500 disabled:opacity-50"
                >
                <p class="mt-1 text-xs text-slate-400">{{ t('tools.fileShare.portHint') }}</p>
              </div>
              <div>
                <label class="mb-1.5 flex items-center gap-2 text-sm font-medium text-slate-700">
                  <input
                    v-model="passwordEnabled"
                    type="checkbox"
                    :disabled="isActive"
                    class="h-4 w-4 rounded border-slate-300 text-cyan-600 focus:ring-cyan-500"
                  >
                  {{ t('tools.fileShare.passwordToggle') }}
                </label>
                <input
                  v-if="passwordEnabled"
                  v-model="password"
                  type="password"
                  :disabled="isActive"
                  :placeholder="t('tools.fileShare.passwordInput')"
                  class="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 text-sm text-slate-800 transition focus:border-cyan-500 focus:outline-none focus:ring-1 focus:ring-cyan-500 disabled:opacity-50"
                >
              </div>
            </div>

            <!-- Error -->
            <div
              v-if="errorMsg"
              class="rounded-lg border border-red-200 bg-red-50 px-4 py-2 text-sm text-red-600"
            >
              {{ errorMsg }}
            </div>

            <!-- Start / Stop -->
            <button
              v-if="!isActive"
              type="button"
              @click="startShare"
              :disabled="isStarting || sharedDirs.length === 0"
              class="inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-cyan-600 to-teal-600 px-6 py-3 text-sm font-semibold text-white shadow-lg shadow-cyan-500/25 transition hover:from-cyan-700 hover:to-teal-700 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Play class="h-4 w-4" />
              {{ isStarting ? t('tools.fileShare.starting') : t('tools.fileShare.startShare') }}
            </button>
            <button
              v-else
              type="button"
              @click="stopShare"
              class="inline-flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-red-500 to-rose-600 px-6 py-3 text-sm font-semibold text-white shadow-lg shadow-red-500/25 transition hover:from-red-600 hover:to-rose-700"
            >
              <Square class="h-4 w-4" />
              {{ t('tools.fileShare.stopShare') }}
            </button>
          </div>

          <!-- Right: Status Panel -->
          <div class="col-span-2 flex flex-col bg-slate-50/50 p-5">
            <!-- Status indicator -->
            <div class="mb-4 flex items-center gap-2">
              <div
                class="h-2.5 w-2.5 rounded-full"
                :class="isActive
                  ? 'bg-cyan-500 shadow-[0_0_6px_rgba(6,182,212,0.5)] animate-pulse'
                  : 'bg-slate-300'"
              ></div>
              <span
                class="text-sm font-semibold"
                :class="isActive ? 'text-cyan-600' : 'text-slate-400'"
              >
                {{ isActive ? t('tools.fileShare.statusActive') : t('tools.fileShare.statusIdle') }}
              </span>
            </div>

            <!-- Active: URL + Stats -->
            <template v-if="isActive && serverUrl">
              <!-- Access URL -->
              <div class="mb-4">
                <label class="mb-1 block text-xs font-medium uppercase tracking-wider text-slate-400">
                  {{ t('tools.fileShare.accessUrl') }}
                </label>
                <div class="flex items-center gap-2 rounded-lg border border-slate-200 bg-white px-3 py-2">
                  <code class="flex-1 truncate text-sm font-bold text-cyan-600">{{ serverUrl }}</code>
                  <button
                    type="button"
                    @click="copyUrl"
                    class="rounded p-1 text-slate-400 transition hover:bg-slate-100 hover:text-slate-600"
                    :title="t('tools.fileShare.copyUrl')"
                    aria-label="Copy URL"
                  >
                    <Copy class="h-4 w-4" :class="copiedUrl ? 'text-cyan-500' : ''" />
                  </button>
                  <button
                    type="button"
                    @click="showQr = !showQr"
                    class="rounded p-1 text-slate-400 transition hover:bg-slate-100 hover:text-slate-600"
                    :title="showQr ? t('tools.fileShare.hideQrCode') : t('tools.fileShare.showQrCode')"
                    aria-label="Toggle QR Code"
                  >
                    <QrCode class="h-4 w-4" />
                  </button>
                </div>
                <p v-if="copiedUrl" class="mt-1 text-xs text-cyan-500">{{ t('tools.fileShare.copied') }}</p>
                <p v-else class="mt-1 text-xs text-slate-400">{{ t('tools.fileShare.qrCodeHint') }}</p>
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
                  {{ t('tools.fileShare.altUrls', { n: altUrls.length }) }}
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

              <!-- Stats -->
              <div class="grid grid-cols-2 gap-3">
                <div class="rounded-lg border border-slate-200 bg-white p-3">
                  <div class="flex items-center gap-2 text-slate-400">
                    <Wifi class="h-3.5 w-3.5" />
                    <span class="text-xs">{{ t('tools.fileShare.downloadCount') }}</span>
                  </div>
                  <div class="mt-1 text-xl font-bold text-slate-800">{{ status.download_count }}</div>
                </div>
                <div class="rounded-lg border border-slate-200 bg-white p-3">
                  <div class="flex items-center gap-2 text-slate-400">
                    <Clock class="h-3.5 w-3.5" />
                    <span class="text-xs">{{ t('tools.fileShare.uptime') }}</span>
                  </div>
                  <div class="mt-1 text-xl font-bold tabular-nums text-slate-800">{{ formattedUptime }}</div>
                </div>
              </div>
            </template>

            <!-- Idle placeholder -->
            <template v-else>
              <div class="flex flex-1 items-center justify-center">
                <div class="text-center text-slate-300">
                  <Share2 class="mx-auto mb-2 h-12 w-12 opacity-40" />
                  <p class="text-sm">{{ t('tools.fileShare.emptyPlaceholder') }}</p>
                </div>
              </div>
            </template>

            <!-- Logs -->
            <div v-if="logs.length > 0" class="mt-4 border-t border-slate-200 pt-3">
              <h3 class="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-400">
                {{ t('tools.fileShare.logTitle') }}
              </h3>
              <div class="max-h-36 space-y-1 overflow-y-auto">
                <div v-for="(log, i) in logs" :key="i" class="flex gap-2 text-xs">
                  <span class="shrink-0 font-mono text-slate-400">{{ log.time }}</span>
                  <span :class="log.level === 'error' ? 'text-red-500' : 'text-slate-600'">
                    {{ log.message }}
                  </span>
                </div>
              </div>
            </div>
          </div>

        </div>
      </div>
    </div>
  </div>
</template>
