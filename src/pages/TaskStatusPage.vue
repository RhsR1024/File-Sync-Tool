<script setup lang="ts">
import { ref, onMounted, onUnmounted, onActivated, watch, computed } from 'vue';
import { Play, Square, RefreshCw, Clock, Activity, Pause, PlayCircle, XCircle, Copy, Trash2, FolderOpen, HardDrive, Cloud, Info, X } from 'lucide-vue-next';
import Empty from '@/components/Empty.vue';
import ManualCopyModal from '@/components/ManualCopyModal.vue';
import { getConfig, cancelScan, pauseScan, resumeScan, addSystemEvent, type AppConfig, type DeployServer, type ScanTask } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { appStore, addLog, markTaskRecordCancelled, setTaskRecordPaused, type TaskRecord } from '@/lib/store';
import { startScheduler, stopScheduler, executeScan } from '@/lib/scheduler';

defineOptions({
  name: 'TaskStatusPage'
});

const { t } = useI18n();
const config = ref<AppConfig | null>(null);
const isCancelling = ref(false);
const selectedPathRecord = ref<TaskRecord | null>(null);
const copyToastMessage = ref('');
let copyToastTimer: ReturnType<typeof setTimeout> | null = null;
const taskTableStyle = {
  gridTemplateColumns: '140px 1fr 100px 196px 100px 116px 92px 100px',
  minWidth: '900px',
};

// Manual copy modal
const isManualCopyModalOpen = ref(false);

// Sort by startedAtMs descending (newest first)
const orderedRecords = computed(() =>
  [...appStore.taskRecords].sort((a, b) => (b.startedAtMs || 0) - (a.startedAtMs || 0))
);

const activeActionRecord = computed(() =>
  orderedRecords.value.find(r => r.phase === 'copying' || r.phase === 'paused')
);

const isPaused = computed(() => activeActionRecord.value?.phase === 'paused');

function normalizePath(path: string | undefined): string {
  if (!path) return '';
  return path.replace(/\//g, '\\').replace(/\\+$/g, '').toLowerCase();
}

function samePath(aRaw: string | undefined, bRaw: string | undefined): boolean {
  const a = normalizePath(aRaw);
  const b = normalizePath(bRaw);
  return !!a && a === b;
}

function pathStartsWith(pathRaw: string | undefined, prefixRaw: string | undefined): boolean {
  const path = normalizePath(pathRaw);
  const prefix = normalizePath(prefixRaw);
  if (!path || !prefix) return false;
  return path === prefix || path.startsWith(`${prefix}\\`);
}

function truncateName(name: string, maxLength = 50): string {
  if (name.length <= maxLength) return name;
  return `${name.slice(0, maxLength)}...`;
}

function openPathInfo(record: TaskRecord) {
  selectedPathRecord.value = record;
}

function closePathInfo() {
  selectedPathRecord.value = null;
}

function showCopyToast(message: string) {
  copyToastMessage.value = message;
  if (copyToastTimer) {
    clearTimeout(copyToastTimer);
  }
  copyToastTimer = setTimeout(() => {
    copyToastMessage.value = '';
    copyToastTimer = null;
  }, 1800);
}

function liveProgress(rec: TaskRecord) {
  if (!appStore.progress) return null;
  if (appStore.progress.folder === rec.folder) return appStore.progress;
  if (samePath(rec.localPath, appStore.progress.localPath)) return appStore.progress;
  return null;
}

async function handleCancel() {
  const target = activeActionRecord.value;
  if (!target || isCancelling.value) return;

  isCancelling.value = true;
  const msg = `${t('console.cancelling')} (${target.folder})`;
  addLog(msg, 'info');
  markTaskRecordCancelled(target.folder);

  try {
    await cancelScan();
  } catch (e) {
    addLog(`Cancel failed: ${e}`, 'error');
  } finally {
    isCancelling.value = false;
  }
}

async function togglePause() {
  const target = activeActionRecord.value;
  if (!target) return;

  if (isPaused.value) {
    await resumeScan();
    setTaskRecordPaused(target.folder, false);
    const msg = `${t('console.resumed')} (${target.folder})`;
    addLog(msg, 'info');
    await addSystemEvent('RESUME', msg);
  } else {
    await pauseScan();
    setTaskRecordPaused(target.folder, true);
    const msg = `${t('console.paused')} (${target.folder})`;
    addLog(msg, 'info');
    await addSystemEvent('PAUSE', msg);
  }
}

function clearRecords() {
  appStore.taskRecords.splice(0, appStore.taskRecords.length);
  selectedPathRecord.value = null;
}

function formatStartTime(ms: number): string {
  const d = new Date(ms);
  const year = d.getFullYear();
  const month = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  const hour = String(d.getHours()).padStart(2, '0');
  const min = String(d.getMinutes()).padStart(2, '0');
  const sec = String(d.getSeconds()).padStart(2, '0');
  return `${year}-${month}-${day} ${hour}:${min}:${sec}`;
}

function formatStatus(phase: TaskRecord['phase']) {
  if (phase === 'queued') return t('console.phaseQueued');
  if (phase === 'paused') return t('console.phasePaused');
  if (phase === 'remote_pushing') return t('console.phaseRemotePushing');
  if (phase === 'remote_deploying') return t('console.phaseRemoteDeploying');
  if (phase === 'failed') return t('console.phaseFailed');
  if (phase === 'cancelled') return t('console.phaseCancelled');
  if (phase === 'completed') return t('console.phaseCompleted');
  return t('console.phaseCopying');
}

function statusBadgeClass(phase: TaskRecord['phase']) {
  if (phase === 'queued') return 'bg-slate-100 text-slate-600 ring-slate-200';
  if (phase === 'paused') return 'bg-amber-50 text-amber-700 ring-amber-200';
  if (phase === 'remote_pushing') return 'bg-purple-50 text-purple-700 ring-purple-200';
  if (phase === 'remote_deploying') return 'bg-fuchsia-50 text-fuchsia-700 ring-fuchsia-200';
  if (phase === 'failed') return 'bg-rose-50 text-rose-600 ring-rose-200';
  if (phase === 'cancelled') return 'bg-red-50 text-red-600 ring-red-200';
  if (phase === 'completed') return 'bg-emerald-50 text-emerald-700 ring-emerald-200';
  return 'bg-blue-50 text-blue-700 ring-blue-200';
}

function progressBarClass(phase: TaskRecord['phase']) {
  if (phase === 'queued') return 'bg-gradient-to-r from-slate-300 to-slate-400';
  if (phase === 'paused') return 'bg-gradient-to-r from-amber-400 to-amber-500';
  if (phase === 'remote_pushing') return 'bg-gradient-to-r from-purple-400 to-purple-500';
  if (phase === 'remote_deploying') return 'bg-gradient-to-r from-fuchsia-400 to-fuchsia-500';
  if (phase === 'failed') return 'bg-gradient-to-r from-rose-400 to-rose-500';
  if (phase === 'cancelled') return 'bg-gradient-to-r from-red-400 to-red-500';
  if (phase === 'completed') return 'bg-gradient-to-r from-emerald-400 to-emerald-500';
  return 'bg-gradient-to-r from-blue-400 to-blue-500';
}

function progressPercentColor(rec: TaskRecord): string {
  const pv = progressValue(rec);
  if (rec.phase === 'completed' || pv >= 100) return 'text-emerald-600';
  if (rec.phase === 'failed') return 'text-rose-600';
  if (rec.phase === 'cancelled') return 'text-red-500';
  if (rec.phase === 'paused') return 'text-amber-600';
  if (rec.phase === 'queued') return 'text-slate-500';
  return 'text-slate-700';
}

function progressValue(rec: TaskRecord): number {
  if (rec.phase === 'remote_pushing' || rec.phase === 'remote_deploying') {
    if (rec.deployPercentage > 0) return rec.deployPercentage;
    return rec.copyCompleted ? 100 : rec.copyPercentage;
  }
  if (rec.phase === 'completed' && rec.copyPercentage < 100) return 100;
  return rec.copyPercentage;
}

function formatSpeed(bytesPerSec: number) {
  if (!bytesPerSec || bytesPerSec <= 0) return '-';
  const k = 1024;
  const sizes = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  const i = Math.floor(Math.log(bytesPerSec) / Math.log(k));
  return `${(bytesPerSec / Math.pow(k, i)).toFixed(1)}${sizes[Math.min(i, 3)]}`;
}

function formatDuration(seconds: number) {
  if (!seconds || seconds <= 0 || !isFinite(seconds)) return '-';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return `${m}m${s}s`;
}

function displaySizeMerged(rec: TaskRecord): string {
  const total = rec.copyTotal > 0 ? rec.copyTotal : rec.total;
  if (total <= 0) return '';

  let copied = rec.copied;
  if (rec.phase === 'completed') copied = total;
  if (rec.phase === 'cancelled') copied = Math.min(rec.copied, total);

  const MB = 1024 * 1024;
  const GB = 1024 * 1024 * 1024;

  if (total >= GB) {
    return `${(copied / GB).toFixed(2)}GB/${(total / GB).toFixed(2)}GB`;
  }
  return `${(copied / MB).toFixed(1)}MB/${(total / MB).toFixed(1)}MB`;
}

function displaySpeed(rec: TaskRecord): string {
  if (rec.phase === 'queued') return '-';
  if (rec.phase === 'completed' || rec.phase === 'failed' || rec.phase === 'cancelled' || rec.phase === 'remote_deploying') return '-';
  if (rec.phase === 'paused') return '-';
  return formatSpeed(rec.speed);
}

function displayEta(rec: TaskRecord): string {
  if (rec.phase === 'queued') return '-';
  if (rec.phase === 'completed' || rec.phase === 'failed' || rec.phase === 'cancelled' || rec.phase === 'remote_deploying') return '-';
  if (rec.phase === 'paused') return '-';
  return formatDuration(liveProgress(rec)?.eta || 0);
}

function displayElapsed(rec: TaskRecord): string {
  if (rec.phase === 'completed' || rec.phase === 'failed' || rec.phase === 'cancelled') {
    const endMs = rec.finishedAtMs || rec.updatedAt;
    return formatDuration((endMs - rec.startedAtMs) / 1000);
  }
  const elapsed = liveProgress(rec)?.elapsed || 0;
  if (elapsed > 0) return formatDuration(elapsed);
  return formatDuration((Date.now() - rec.startedAtMs) / 1000);
}

function displayRemoteTarget(server: DeployServer, folder: string): string {
  const base = server.remote_path.replace(/[\\/]+$/g, '');
  return `[${server.name}] ${base}/${folder}`;
}

function resolveScheduledTask(rec: TaskRecord): ScanTask | undefined {
  if (!config.value || rec.source !== 'scheduled' || !rec.sourcePath || !rec.localPath) return undefined;

  return config.value.tasks.find(task => {
    if (!pathStartsWith(rec.sourcePath, task.remote_path)) return false;
    const localBase = task.local_path || config.value?.local_path || '';
    return !localBase || pathStartsWith(rec.localPath, localBase);
  });
}

function remotePathOf(rec: TaskRecord): string[] {
  const byServers = rec.remoteServers.map(s => s.label).filter(Boolean);
  if (byServers.length > 0) {
    return Array.from(new Set(byServers));
  }

  if (!config.value?.deploy_enabled || rec.source !== 'scheduled') {
    return [];
  }

  const task = resolveScheduledTask(rec);
  if (!task || task.server_bindings.length === 0) {
    return [];
  }

  const targets = task.server_bindings
    .map(binding => config.value?.servers.find(server => server.id === binding.server_id && server.enabled))
    .filter((server): server is DeployServer => Boolean(server))
    .map(server => displayRemoteTarget(server, rec.folder));

  return Array.from(new Set(targets));
}

async function copyToClipboard(text: string) {
  if (!text || text === '-') return;
  try {
    await navigator.clipboard.writeText(text);
    addLog(t('console.copied'), 'success');
    showCopyToast(t('settings.pathCopied'));
  } catch (err) {
    addLog(`Failed to copy: ${err}`, 'error');
  }
}

async function loadConfig() {
  try {
    const newConfig = await getConfig();
    config.value = newConfig;
  } catch (e) {
    addLog(t('console.failedLoadConfig', { error: e }), 'error');
  }
}

async function handleScanClick() {
  if (appStore.isRunning) return;
  await executeScan();
}

onActivated(() => {
  loadConfig();
});

onMounted(() => {
  loadConfig();
});

onUnmounted(() => {
  if (copyToastTimer) {
    clearTimeout(copyToastTimer);
    copyToastTimer = null;
  }
});

watch(activeActionRecord, (val) => {
  if (!val) isCancelling.value = false;
});
</script>

<template>
  <div class="p-6 h-full flex flex-col gap-6 bg-slate-50">
    <h2 class="text-2xl font-bold text-slate-800">{{ t('sidebar.tasks') }}</h2>

    <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
      <div class="bg-white p-5 rounded-xl border border-slate-200 shadow-sm relative overflow-hidden group hover:shadow-md transition-shadow">
        <div class="absolute top-0 right-0 p-4 opacity-10 group-hover:opacity-20 transition-opacity">
          <Activity class="w-16 h-16 text-blue-600" />
        </div>
        <div class="text-slate-500 text-sm font-medium uppercase tracking-wider mb-2">{{ t('console.status') }}</div>
        <div class="flex items-center gap-3 font-bold text-2xl" :class="appStore.isRunning ? 'text-emerald-600' : 'text-slate-700'">
          <div class="relative flex h-3 w-3">
             <span v-if="appStore.isRunning" class="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
             <span class="relative inline-flex rounded-full h-3 w-3" :class="appStore.isRunning ? 'bg-emerald-500' : 'bg-slate-400'"></span>
          </div>
          {{ appStore.isRunning ? t('console.running') : t('console.stopped') }}
        </div>
      </div>

      <div class="bg-white p-5 rounded-xl border border-slate-200 shadow-sm relative overflow-hidden group hover:shadow-md transition-shadow">
        <div class="absolute top-0 right-0 p-4 opacity-10 group-hover:opacity-20 transition-opacity">
          <Clock class="w-16 h-16 text-blue-600" />
        </div>
        <div class="text-slate-500 text-sm font-medium uppercase tracking-wider mb-2">{{ t('console.nextRun') }}</div>
        <div class="flex items-center gap-2 font-bold text-2xl text-slate-800 font-mono">
          {{ appStore.nextRunTime }}
        </div>
      </div>
    </div>

    <div class="bg-white rounded-xl border border-slate-200 shadow-sm flex flex-col relative overflow-hidden flex-1">
      <div class="p-4 flex gap-3 border-b border-slate-100 items-center justify-between">
        <h3 class="text-lg font-semibold text-slate-700">{{ t('console.schedulerControls') }}</h3>
        <div class="flex gap-3">
          <button
            @click="appStore.isRunning ? stopScheduler() : startScheduler()"
            class="px-6 py-2 rounded-lg font-bold transition-all flex items-center justify-center gap-2 shadow-sm active:scale-95"
            :class="appStore.isRunning
              ? 'bg-red-50 text-red-600 hover:bg-red-100 border border-red-200'
              : 'bg-emerald-600 text-white hover:bg-emerald-700 shadow-emerald-200'"
          >
            <component :is="appStore.isRunning ? Square : Play" class="w-4 h-4 fill-current" />
            {{ appStore.isRunning ? t('console.stop') : t('console.start') }}
          </button>

          <button
            @click="handleScanClick"
            class="px-4 py-2 rounded-lg font-bold bg-white text-blue-600 border border-blue-200 hover:bg-blue-50 hover:border-blue-300 transition-all flex items-center gap-2 shadow-sm active:scale-95"
            :disabled="appStore.isRunning"
            :class="{ 'opacity-50 cursor-not-allowed': appStore.isRunning }"
          >
            <RefreshCw class="w-4 h-4" :class="{ 'animate-spin': appStore.isRunning }" />
            {{ t('console.scanNow') }}
          </button>

          <button
            @click="isManualCopyModalOpen = true"
            class="px-4 py-2 rounded-lg font-bold bg-white text-purple-600 border border-purple-200 hover:bg-purple-50 hover:border-purple-300 transition-all flex items-center gap-2 shadow-sm active:scale-95"
          >
            <Copy class="w-4 h-4" />
            {{ t('manualCopy.title') }}
          </button>
        </div>
      </div>

      <div class="flex-1 bg-slate-50 p-4 overflow-auto">
        <div v-if="orderedRecords.length" class="pb-2 flex justify-end">
          <button
            @click="clearRecords"
            class="text-slate-500 hover:text-red-600 px-2 py-1 rounded-md hover:bg-red-50 transition-colors flex items-center gap-1 text-sm"
          >
            <Trash2 class="w-4 h-4" />
            {{ t('console.clearRecords') }}
          </button>
        </div>

        <Empty
          v-if="!orderedRecords.length"
          :icon="Activity"
          :title="t('console.noRecords')"
          class="min-h-[220px]"
        />

        <div v-else class="bg-white border border-slate-200 rounded-lg overflow-hidden shadow-sm">
          <div class="overflow-x-auto">
          <!-- Table Header -->
          <div
            class="grid gap-4 px-4 py-3 bg-slate-50 text-xs text-slate-500 font-semibold border-b border-slate-200 select-none"
            :style="taskTableStyle"
          >
            <div>{{ t('console.startTime') }}</div>
            <div>{{ t('console.name') }}</div>
            <div class="text-center">{{ t('console.status') }}</div>
            <div class="text-center">{{ t('console.progress') }}</div>
            <div class="text-center">{{ t('console.speed') }}</div>
            <div class="text-center">{{ t('console.eta') }}</div>
            <div class="text-center">{{ t('console.elapsed') }}</div>
            <div class="text-center">{{ t('console.pathInfo') }}</div>
          </div>

          <!-- Table Body -->
          <div class="divide-y divide-slate-100/80">
            <div
              v-for="rec in orderedRecords"
              :key="rec.id"
            >

              <!-- Main Row -->
              <div
                class="grid gap-4 px-4 py-3 items-center text-sm transition-colors hover:bg-slate-50/60"
                :style="taskTableStyle"
              >
                <!-- Start Time -->
                <div class="text-xs text-slate-500 font-mono tabular-nums leading-tight">
                  {{ formatStartTime(rec.startedAtMs) }}
                </div>

                <!-- Name -->
                <div class="flex items-center gap-1.5 min-w-0" :title="rec.folder">
                  <div class="w-5 h-5 rounded flex items-center justify-center shrink-0"
                    :class="rec.source === 'manual' ? 'bg-purple-100 text-purple-600' : 'bg-blue-100 text-blue-600'">
                    <Activity class="w-3 h-3" />
                  </div>
                  <span class="block w-full max-w-[50ch] truncate font-medium text-slate-800 text-[13px]">{{ truncateName(rec.folder) }}</span>
                </div>

                <!-- Status Badge -->
                <div class="flex justify-center">
                  <span
                    class="inline-flex items-center px-2 py-1 rounded text-[11px] font-bold ring-1 ring-inset leading-none whitespace-nowrap"
                    :class="statusBadgeClass(rec.phase)"
                  >
                    {{ formatStatus(rec.phase) }}
                  </span>
                </div>

                <!-- Progress (merged: bar + percentage + size) -->
                <div class="flex justify-center">
                  <div class="flex w-[188px] min-w-0 flex-col items-start gap-1.5">
                    <div class="flex w-full items-baseline gap-3">
                      <span class="w-12 shrink-0 text-right text-[13px] font-bold tabular-nums" :class="progressPercentColor(rec)">
                        {{ progressValue(rec).toFixed(1) }}%
                      </span>
                      <span class="min-w-0 flex-1 truncate text-center text-[12px] text-slate-500 font-semibold font-mono tabular-nums">
                        {{ displaySizeMerged(rec) }}
                      </span>
                    </div>
                    <div class="h-1.5 w-full rounded-full bg-slate-100 overflow-hidden">
                      <div
                        class="h-full rounded-full transition-all duration-300 ease-out"
                        :class="progressBarClass(rec.phase)"
                        :style="{ width: `${Math.min(progressValue(rec), 100)}%` }"
                      ></div>
                    </div>
                  </div>
                </div>

                <!-- Speed -->
                <div
                  class="w-full truncate text-center font-mono text-[13px] tabular-nums"
                  :class="rec.speed > 0 && rec.phase !== 'paused' ? 'text-blue-600' : 'text-slate-400'"
                >
                  {{ displaySpeed(rec) }}
                </div>

                <!-- ETA -->
                <div class="w-full truncate text-center font-mono text-[13px] text-slate-500 tabular-nums">
                  {{ displayEta(rec) }}
                </div>

                <!-- Elapsed -->
                <div class="w-full truncate text-center font-mono text-[13px] text-slate-500 tabular-nums">
                  {{ displayElapsed(rec) }}
                </div>

                <!-- Path Info Action -->
                <div class="flex justify-center">
                  <button
                    @click="openPathInfo(rec)"
                    class="inline-flex items-center justify-center gap-1.5 rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-[13px] font-medium text-slate-600 transition-colors hover:border-blue-200 hover:bg-blue-50 hover:text-blue-600"
                    :title="t('console.viewPathInfo')"
                  >
                    <Info class="w-3.5 h-3.5" />
                    <span>{{ t('console.viewPathInfo') }}</span>
                  </button>
                </div>
              </div>
            </div>
          </div>
          </div><!-- /overflow-x-auto -->

          <!-- Action Bar -->
          <div v-if="activeActionRecord" class="p-3 bg-slate-50 border-t border-slate-100 flex justify-end gap-3">
            <button
              @click="togglePause"
              class="px-4 py-2 rounded-lg font-medium border flex items-center gap-2 transition-colors shadow-sm active:scale-95"
              :class="isPaused ? 'bg-emerald-50 text-emerald-700 border-emerald-200 hover:bg-emerald-100' : 'bg-amber-50 text-amber-700 border-amber-200 hover:bg-amber-100'"
            >
              <component :is="isPaused ? PlayCircle : Pause" class="w-4 h-4" />
              {{ isPaused ? t('console.resume') : t('console.pause') }}
            </button>

            <button
              @click="handleCancel"
              class="px-4 py-2 rounded-lg font-medium bg-white text-red-600 border border-red-200 hover:bg-red-50 hover:border-red-300 transition-colors shadow-sm active:scale-95 flex items-center gap-2"
              :disabled="isCancelling"
            >
              <XCircle class="w-4 h-4" />
              {{ isCancelling ? t('console.cancelling') : t('console.cancel') }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Manual Copy Modal -->
    <ManualCopyModal
      :is-open="isManualCopyModalOpen"
      @close="isManualCopyModalOpen = false"
      @success="() => {}"
    />

    <Transition
      enter-active-class="transition-all duration-200"
      leave-active-class="transition-all duration-150"
      enter-from-class="translate-y-1 opacity-0"
      leave-to-class="translate-y-1 opacity-0"
    >
      <div
        v-if="copyToastMessage"
        class="pointer-events-none fixed right-6 top-6 z-[60] rounded-xl border border-emerald-200 bg-white/95 px-4 py-3 text-sm font-medium text-emerald-700 shadow-lg shadow-emerald-100 backdrop-blur"
      >
        {{ copyToastMessage }}
      </div>
    </Transition>

    <Transition
      enter-active-class="transition-opacity duration-200"
      leave-active-class="transition-opacity duration-150"
      enter-from-class="opacity-0"
      leave-to-class="opacity-0"
    >
      <div
        v-if="selectedPathRecord"
        class="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/40 px-4 backdrop-blur-sm"
        @click="closePathInfo"
      >
        <div
          class="w-full max-w-4xl rounded-2xl border border-slate-200 bg-white shadow-2xl"
          @click.stop
        >
          <div class="flex items-start justify-between gap-4 border-b border-slate-200 px-6 py-5">
            <div class="min-w-0">
              <h3 class="text-lg font-bold text-slate-800">{{ t('console.pathInfo') }}</h3>
              <p class="mt-1 truncate text-sm text-slate-500" :title="selectedPathRecord.folder">
                {{ selectedPathRecord.folder }}
              </p>
            </div>
            <button
              @click="closePathInfo"
              class="rounded-lg p-2 text-slate-400 transition-colors hover:bg-slate-100 hover:text-slate-600"
              :title="t('settings.close')"
            >
              <X class="h-5 w-5" />
            </button>
          </div>

          <div class="grid gap-4 px-6 py-6 md:grid-cols-3">
            <div class="flex min-h-[160px] flex-col rounded-xl border border-slate-200 bg-slate-50 p-4">
              <div class="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-700">
                <FolderOpen class="h-4 w-4 text-orange-500" />
                {{ t('console.sourcePath') }}
              </div>
              <div class="flex-1 break-all font-mono text-[12px] leading-6 text-slate-700">
                {{ selectedPathRecord.sourcePath || '-' }}
              </div>
              <button
                v-if="selectedPathRecord.sourcePath"
                @click="copyToClipboard(selectedPathRecord.sourcePath)"
                class="mt-4 inline-flex items-center gap-2 self-start rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-xs font-medium text-slate-600 transition-colors hover:border-blue-200 hover:bg-blue-50 hover:text-blue-600"
              >
                <Copy class="h-3.5 w-3.5" />
                {{ t('settings.copyPath') }}
              </button>
            </div>

            <div class="flex min-h-[160px] flex-col rounded-xl border border-slate-200 bg-slate-50 p-4">
              <div class="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-700">
                <HardDrive class="h-4 w-4 text-blue-500" />
                {{ t('console.localCopyPath') }}
              </div>
              <div class="flex-1 break-all font-mono text-[12px] leading-6 text-slate-700">
                {{ selectedPathRecord.localPath || '-' }}
              </div>
              <button
                v-if="selectedPathRecord.localPath"
                @click="copyToClipboard(selectedPathRecord.localPath)"
                class="mt-4 inline-flex items-center gap-2 self-start rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-xs font-medium text-slate-600 transition-colors hover:border-blue-200 hover:bg-blue-50 hover:text-blue-600"
              >
                <Copy class="h-3.5 w-3.5" />
                {{ t('settings.copyPath') }}
              </button>
            </div>

            <div class="flex min-h-[160px] flex-col rounded-xl border border-slate-200 bg-slate-50 p-4">
              <div class="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-700">
                <Cloud class="h-4 w-4 text-purple-500" />
                {{ t('console.remotePushPath') }}
              </div>
              <div class="flex-1">
                <template v-if="remotePathOf(selectedPathRecord).length > 0">
                  <div
                    v-for="(remotePath, index) in remotePathOf(selectedPathRecord)"
                    :key="`${selectedPathRecord.id}-modal-remote-${index}`"
                    class="break-all font-mono text-[12px] leading-6 text-slate-700"
                  >
                    {{ remotePath }}
                  </div>
                </template>
                <div v-else class="font-mono text-[12px] leading-6 text-slate-400">
                  -
                </div>
              </div>
              <button
                v-if="remotePathOf(selectedPathRecord).length > 0"
                @click="copyToClipboard(remotePathOf(selectedPathRecord).join('\n'))"
                class="mt-4 inline-flex items-center gap-2 self-start rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-xs font-medium text-slate-600 transition-colors hover:border-blue-200 hover:bg-blue-50 hover:text-blue-600"
              >
                <Copy class="h-3.5 w-3.5" />
                {{ t('settings.copyPath') }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>
