<script setup lang="ts">
import { ref, onMounted, onActivated, watch, computed } from 'vue';
import { Play, Square, RefreshCw, Clock, Activity, Pause, PlayCircle, XCircle, Copy, Trash2 } from 'lucide-vue-next';
import { getConfig, cancelScan, pauseScan, resumeScan, addSystemEvent, type AppConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { appStore, addLog, markTaskRecordCancelled, setTaskRecordPaused, type TaskRecord } from '@/lib/store';
import { startScheduler, stopScheduler, executeScan } from '@/lib/scheduler';

defineOptions({
  name: 'TaskStatusPage'
});

const { t } = useI18n();
const config = ref<AppConfig | null>(null);
const isCancelling = ref(false);
const taskTableCols = 'grid-cols-[2.8fr_1fr_1.3fr_1.4fr_2.4fr_2.4fr_1fr_1.1fr_1fr]';

const orderedRecords = computed(() =>
  [...appStore.taskRecords].sort((a, b) => (b.updatedAt || 0) - (a.updatedAt || 0))
);

const activeActionRecord = computed(() =>
  orderedRecords.value.find(r => r.phase === 'copying' || r.phase === 'paused')
);

const isPaused = computed(() => activeActionRecord.value?.phase === 'paused');

function liveProgress(rec: TaskRecord) {
  if (!appStore.progress) return null;
  if (appStore.progress.folder === rec.folder) return appStore.progress;
  const recLocal = (rec.localPath || '').toLowerCase().replace(/\//g, '\\');
  const pLocal = (appStore.progress.localPath || '').toLowerCase().replace(/\//g, '\\');
  if (recLocal && pLocal && (pLocal.startsWith(recLocal) || recLocal.startsWith(pLocal))) return appStore.progress;
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
}

function formatStatus(phase: TaskRecord['phase']) {
  if (phase === 'paused') return '暂停中';
  if (phase === 'remote_pushing') return '远程推送中';
  if (phase === 'remote_deploying') return '远程部署中';
  if (phase === 'cancelled') return '已取消';
  if (phase === 'completed') return '已完成';
  return '复制中';
}

function statusClass(phase: TaskRecord['phase']) {
  if (phase === 'paused') return 'text-amber-600';
  if (phase === 'remote_pushing') return 'text-purple-600';
  if (phase === 'remote_deploying') return 'text-fuchsia-600';
  if (phase === 'cancelled') return 'text-red-600';
  if (phase === 'completed') return 'text-emerald-600';
  return 'text-blue-600';
}

function progressClass(phase: TaskRecord['phase']) {
  if (phase === 'paused') return 'bg-amber-500';
  if (phase === 'remote_pushing') return 'bg-purple-500';
  if (phase === 'remote_deploying') return 'bg-fuchsia-500';
  if (phase === 'cancelled') return 'bg-red-500';
  if (phase === 'completed') return 'bg-emerald-500';
  return 'bg-blue-500';
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
  return `${(bytesPerSec / Math.pow(k, i)).toFixed(1)}${sizes[i]}`;
}

function formatDuration(seconds: number) {
  if (!seconds || seconds <= 0 || !isFinite(seconds)) return '-';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return `${m}m ${s}s`;
}

function formatSize(copied: number, total: number) {
  if (!total || total <= 0) return '-';
  return `${(copied / 1024 / 1024).toFixed(2)}MB / ${(total / 1024 / 1024).toFixed(2)}MB`;
}

function displaySize(rec: TaskRecord): string {
  const total = rec.copyTotal > 0 ? rec.copyTotal : rec.total;
  if (total <= 0) return '-';

  if (rec.phase === 'copying' || rec.phase === 'paused') {
    return formatSize(rec.copied, Math.max(total, rec.total));
  }

  if (rec.phase === 'cancelled') {
    return formatSize(Math.min(rec.copied, total), total);
  }

  return formatSize(total, total);
}

function displaySpeed(rec: TaskRecord): string {
  if (rec.phase === 'completed' || rec.phase === 'cancelled' || rec.phase === 'remote_deploying') return '-';
  if (rec.phase === 'paused') return '-';
  return formatSpeed(rec.speed);
}

function displayEta(rec: TaskRecord): string {
  if (rec.phase === 'completed' || rec.phase === 'cancelled' || rec.phase === 'remote_deploying') return '-';
  if (rec.phase === 'paused') return '-';
  return formatDuration(liveProgress(rec)?.eta || 0);
}

function displayElapsed(rec: TaskRecord): string {
  if (rec.phase === 'completed' || rec.phase === 'cancelled') {
    const endMs = rec.finishedAtMs || rec.updatedAt;
    return formatDuration((endMs - rec.startedAtMs) / 1000);
  }
  const elapsed = liveProgress(rec)?.elapsed || 0;
  if (elapsed > 0) return formatDuration(elapsed);
  return formatDuration((Date.now() - rec.startedAtMs) / 1000);
}

function remotePathOf(rec: TaskRecord) {
  if (rec.remoteServers.length > 0) return rec.remoteServers[0].label;
  return liveProgress(rec)?.remotePath || '-';
}

async function copyToClipboard(text: string) {
  if (!text || text === '-') return;
  try {
    await navigator.clipboard.writeText(text);
    addLog(t('console.copied'), 'success');
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

        <div v-if="!orderedRecords.length" class="min-h-[220px] flex flex-col items-center justify-center text-slate-400 border-2 border-dashed border-slate-200 rounded-lg bg-white">
          <Activity class="w-12 h-12 mb-2 opacity-20" />
          <span>No active tasks running</span>
        </div>

        <div v-else class="bg-white border border-slate-200 rounded-lg overflow-hidden shadow-sm">
          <div :class="['grid gap-3 p-3 bg-slate-100 text-slate-600 font-bold border-b border-slate-200 text-sm', taskTableCols]">
            <div class="truncate">{{ t('console.name') }}</div>
            <div class="truncate">{{ t('console.status') }}</div>
            <div class="truncate">{{ t('console.progress') }}</div>
            <div class="truncate">{{ t('console.size') }}</div>
            <div class="truncate">{{ t('console.localPath') }}</div>
            <div class="truncate">{{ t('console.remotePath') }}</div>
            <div class="truncate">{{ t('console.speed') }}</div>
            <div class="truncate">{{ t('console.eta') }}</div>
            <div class="truncate">{{ t('console.elapsed') }}</div>
          </div>

          <div class="divide-y divide-slate-100">
            <div
              v-for="rec in orderedRecords"
              :key="rec.id"
              :class="['grid gap-3 p-4 items-center text-sm', taskTableCols]"
            >
              <div class="flex items-center gap-2 truncate font-medium text-slate-800" :title="rec.folder">
                <div class="w-8 h-8 bg-blue-100 text-blue-600 rounded flex items-center justify-center shrink-0">
                  <Activity class="w-4 h-4" />
                </div>
                <span class="truncate">{{ rec.folder }}</span>
              </div>

              <div class="font-bold truncate" :class="statusClass(rec.phase)">
                {{ formatStatus(rec.phase) }}
              </div>

              <div class="relative h-5 bg-slate-100 rounded-full overflow-hidden border border-slate-200 w-full max-w-[170px]">
                <div class="absolute inset-0 transition-all duration-300" :class="progressClass(rec.phase)" :style="{ width: `${progressValue(rec)}%` }"></div>
                <div class="absolute inset-0 flex items-center justify-center text-[11px] text-white font-bold drop-shadow-md z-10">
                  {{ progressValue(rec).toFixed(1) }}%
                </div>
              </div>

              <div class="truncate font-mono text-slate-600" :title="displaySize(rec)">
                {{ displaySize(rec) }}
              </div>

              <div class="flex items-center gap-1 overflow-hidden" :title="rec.localPath || '-'">
                <div class="truncate text-slate-500 text-xs flex-1">{{ rec.localPath || '-' }}</div>
                <button v-if="rec.localPath" @click="copyToClipboard(rec.localPath)" class="text-slate-400 hover:text-blue-600 transition-colors">
                  <Copy class="w-3 h-3" />
                </button>
              </div>

              <div class="flex items-center gap-1 overflow-hidden" :title="remotePathOf(rec)">
                <div class="truncate text-slate-500 text-xs flex-1">{{ remotePathOf(rec) }}</div>
                <button v-if="remotePathOf(rec) !== '-'" @click="copyToClipboard(remotePathOf(rec))" class="text-slate-400 hover:text-blue-600 transition-colors">
                  <Copy class="w-3 h-3" />
                </button>
              </div>

              <div class="truncate font-mono font-medium text-[13px]" :class="rec.phase === 'paused' ? 'text-slate-400' : 'text-blue-600'">
                {{ displaySpeed(rec) }}
              </div>

              <div class="truncate font-mono text-slate-600">
                {{ displayEta(rec) }}
              </div>

              <div class="truncate font-mono text-slate-600">
                {{ displayElapsed(rec) }}
              </div>
            </div>
          </div>

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
  </div>
</template>
