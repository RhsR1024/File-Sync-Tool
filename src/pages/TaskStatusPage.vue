<script setup lang="ts">
import { ref, onMounted, onUnmounted, onActivated, watch, computed } from 'vue';
import { Play, Square, RefreshCw, Clock, Activity, Pause, PlayCircle, XCircle, Copy, Trash2, FolderOpen, HardDrive, Cloud, ChevronDown } from 'lucide-vue-next';
import Empty from '@/components/Empty.vue';
import ManualCopyModal from '@/components/ManualCopyModal.vue';
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
const expandedPathId = ref<string | null>(null);
const taskTableCols = 'grid-cols-[100px_170px_76px_1fr_76px_64px_64px_36px]';

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

function togglePathExpand(id: string) {
  expandedPathId.value = expandedPathId.value === id ? null : id;
}

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
  expandedPathId.value = null;
}

function formatStartTime(ms: number): string {
  const d = new Date(ms);
  const month = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  const hour = String(d.getHours()).padStart(2, '0');
  const min = String(d.getMinutes()).padStart(2, '0');
  const sec = String(d.getSeconds()).padStart(2, '0');
  return `${month}-${day} ${hour}:${min}:${sec}`;
}

function formatStatus(phase: TaskRecord['phase']) {
  if (phase === 'paused') return t('console.phasePaused');
  if (phase === 'remote_pushing') return t('console.phaseRemotePushing');
  if (phase === 'remote_deploying') return t('console.phaseRemoteDeploying');
  if (phase === 'cancelled') return t('console.phaseCancelled');
  if (phase === 'completed') return t('console.phaseCompleted');
  return t('console.phaseCopying');
}

function statusBadgeClass(phase: TaskRecord['phase']) {
  if (phase === 'paused') return 'bg-amber-50 text-amber-700 ring-amber-200';
  if (phase === 'remote_pushing') return 'bg-purple-50 text-purple-700 ring-purple-200';
  if (phase === 'remote_deploying') return 'bg-fuchsia-50 text-fuchsia-700 ring-fuchsia-200';
  if (phase === 'cancelled') return 'bg-red-50 text-red-600 ring-red-200';
  if (phase === 'completed') return 'bg-emerald-50 text-emerald-700 ring-emerald-200';
  return 'bg-blue-50 text-blue-700 ring-blue-200';
}

function progressBarClass(phase: TaskRecord['phase']) {
  if (phase === 'paused') return 'bg-gradient-to-r from-amber-400 to-amber-500';
  if (phase === 'remote_pushing') return 'bg-gradient-to-r from-purple-400 to-purple-500';
  if (phase === 'remote_deploying') return 'bg-gradient-to-r from-fuchsia-400 to-fuchsia-500';
  if (phase === 'cancelled') return 'bg-gradient-to-r from-red-400 to-red-500';
  if (phase === 'completed') return 'bg-gradient-to-r from-emerald-400 to-emerald-500';
  return 'bg-gradient-to-r from-blue-400 to-blue-500';
}

function progressPercentColor(rec: TaskRecord): string {
  const pv = progressValue(rec);
  if (rec.phase === 'completed' || pv >= 100) return 'text-emerald-600';
  if (rec.phase === 'cancelled') return 'text-red-500';
  if (rec.phase === 'paused') return 'text-amber-600';
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
  const byServers = rec.remoteServers.map(s => s.label).filter(Boolean);
  if (byServers.length > 0) {
    return Array.from(new Set(byServers));
  }

  const live = liveProgress(rec)?.remotePath;
  return live ? [live] : [];
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

// Close expanded path when clicking outside
function handleGlobalClick(e: MouseEvent) {
  const target = e.target as HTMLElement;
  if (expandedPathId.value && !target.closest('[data-path-region]')) {
    expandedPathId.value = null;
  }
}

onActivated(() => {
  loadConfig();
});

onMounted(() => {
  loadConfig();
  document.addEventListener('click', handleGlobalClick);
});

onUnmounted(() => {
  document.removeEventListener('click', handleGlobalClick);
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
          <!-- Table Header -->
          <div :class="['grid gap-2 px-3 py-2.5 bg-slate-50 text-[11px] text-slate-500 font-semibold uppercase tracking-wider border-b border-slate-200 select-none', taskTableCols]">
            <div>{{ t('console.startTime') }}</div>
            <div>{{ t('console.name') }}</div>
            <div>{{ t('console.status') }}</div>
            <div>{{ t('console.progress') }}</div>
            <div>{{ t('console.speed') }}</div>
            <div>{{ t('console.eta') }}</div>
            <div>{{ t('console.elapsed') }}</div>
            <div></div>
          </div>

          <!-- Table Body -->
          <div class="divide-y divide-slate-100/80">
            <div
              v-for="rec in orderedRecords"
              :key="rec.id"
              data-path-region
            >
              <!-- Main Row -->
              <div
                :class="['grid gap-2 px-3 py-2.5 items-center text-[13px] transition-colors', taskTableCols,
                  expandedPathId === rec.id ? 'bg-blue-50/40' : 'hover:bg-slate-50/60']"
              >
                <!-- Start Time -->
                <div class="text-[11px] text-slate-400 font-mono tabular-nums leading-tight">
                  {{ formatStartTime(rec.startedAtMs) }}
                </div>

                <!-- Name -->
                <div class="flex items-center gap-1.5 min-w-0" :title="rec.folder">
                  <div class="w-5 h-5 rounded flex items-center justify-center shrink-0"
                    :class="rec.source === 'manual' ? 'bg-purple-100 text-purple-600' : 'bg-blue-100 text-blue-600'">
                    <Activity class="w-3 h-3" />
                  </div>
                  <span class="truncate font-medium text-slate-800 text-[12px]">{{ rec.folder }}</span>
                </div>

                <!-- Status Badge -->
                <div>
                  <span
                    class="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-bold ring-1 ring-inset leading-none whitespace-nowrap"
                    :class="statusBadgeClass(rec.phase)"
                  >
                    {{ formatStatus(rec.phase) }}
                  </span>
                </div>

                <!-- Progress (merged: bar + percentage + size) -->
                <div class="flex flex-col gap-1 min-w-0">
                  <div class="flex items-baseline justify-between gap-2">
                    <span class="text-[12px] font-bold tabular-nums" :class="progressPercentColor(rec)">
                      {{ progressValue(rec).toFixed(1) }}%
                    </span>
                    <span class="text-[10px] text-slate-400 font-mono tabular-nums truncate">
                      {{ displaySizeMerged(rec) }}
                    </span>
                  </div>
                  <div class="h-[5px] bg-slate-100 rounded-full overflow-hidden">
                    <div
                      class="h-full rounded-full transition-all duration-300 ease-out"
                      :class="progressBarClass(rec.phase)"
                      :style="{ width: `${Math.min(progressValue(rec), 100)}%` }"
                    ></div>
                  </div>
                </div>

                <!-- Speed -->
                <div class="truncate font-mono text-[11px] tabular-nums text-right" :class="rec.speed > 0 && rec.phase !== 'paused' ? 'text-blue-600' : 'text-slate-400'">
                  {{ displaySpeed(rec) }}
                </div>

                <!-- ETA -->
                <div class="truncate font-mono text-[11px] text-slate-500 tabular-nums text-right">
                  {{ displayEta(rec) }}
                </div>

                <!-- Elapsed -->
                <div class="truncate font-mono text-[11px] text-slate-500 tabular-nums text-right">
                  {{ displayElapsed(rec) }}
                </div>

                <!-- Path Info Toggle -->
                <button
                  @click.stop="togglePathExpand(rec.id)"
                  class="w-7 h-7 rounded-md flex items-center justify-center transition-all"
                  :class="expandedPathId === rec.id
                    ? 'bg-blue-100 text-blue-600 ring-1 ring-blue-200'
                    : 'text-slate-400 hover:text-slate-600 hover:bg-slate-100'"
                  :title="t('console.pathInfo')"
                >
                  <ChevronDown
                    class="w-3.5 h-3.5 transition-transform duration-200"
                    :class="expandedPathId === rec.id ? 'rotate-180' : ''"
                  />
                </button>
              </div>

              <!-- Expanded Path Details -->
              <Transition
                enter-active-class="transition-all duration-200 ease-out"
                leave-active-class="transition-all duration-150 ease-in"
                enter-from-class="opacity-0 max-h-0"
                enter-to-class="opacity-100 max-h-48"
                leave-from-class="opacity-100 max-h-48"
                leave-to-class="opacity-0 max-h-0"
              >
                <div v-if="expandedPathId === rec.id" class="overflow-hidden">
                  <div class="px-3 py-3 bg-gradient-to-b from-slate-50/80 to-white border-t border-slate-100">
                    <div class="grid grid-cols-1 md:grid-cols-3 gap-2">
                      <!-- Source Remote Path -->
                      <div class="flex items-start gap-2 px-3 py-2.5 rounded-lg bg-white border border-slate-100 group/card">
                        <FolderOpen class="w-3.5 h-3.5 text-orange-400 mt-0.5 shrink-0" />
                        <div class="min-w-0 flex-1">
                          <div class="text-[10px] text-slate-400 font-semibold uppercase tracking-wider mb-1">{{ t('console.sourcePath') }}</div>
                          <div class="text-[11px] text-slate-700 font-mono break-all leading-relaxed">{{ rec.sourcePath || '-' }}</div>
                        </div>
                        <button
                          v-if="rec.sourcePath"
                          @click.stop="copyToClipboard(rec.sourcePath)"
                          class="opacity-0 group-hover/card:opacity-100 text-slate-300 hover:text-blue-500 transition-all shrink-0 mt-0.5"
                        >
                          <Copy class="w-3 h-3" />
                        </button>
                      </div>

                      <!-- Local Copy Path -->
                      <div class="flex items-start gap-2 px-3 py-2.5 rounded-lg bg-white border border-slate-100 group/card">
                        <HardDrive class="w-3.5 h-3.5 text-blue-400 mt-0.5 shrink-0" />
                        <div class="min-w-0 flex-1">
                          <div class="text-[10px] text-slate-400 font-semibold uppercase tracking-wider mb-1">{{ t('console.localCopyPath') }}</div>
                          <div class="text-[11px] text-slate-700 font-mono break-all leading-relaxed">{{ rec.localPath || '-' }}</div>
                        </div>
                        <button
                          v-if="rec.localPath"
                          @click.stop="copyToClipboard(rec.localPath)"
                          class="opacity-0 group-hover/card:opacity-100 text-slate-300 hover:text-blue-500 transition-all shrink-0 mt-0.5"
                        >
                          <Copy class="w-3 h-3" />
                        </button>
                      </div>

                      <!-- Remote Push Paths -->
                      <div class="flex items-start gap-2 px-3 py-2.5 rounded-lg bg-white border border-slate-100 group/card">
                        <Cloud class="w-3.5 h-3.5 text-purple-400 mt-0.5 shrink-0" />
                        <div class="min-w-0 flex-1">
                          <div class="text-[10px] text-slate-400 font-semibold uppercase tracking-wider mb-1">{{ t('console.remotePushPath') }}</div>
                          <template v-if="remotePathOf(rec).length > 0">
                            <div
                              v-for="(rp, idx) in remotePathOf(rec)"
                              :key="`${rec.id}-rp-${idx}`"
                              class="text-[11px] text-slate-700 font-mono break-all leading-relaxed"
                            >
                              {{ rp }}
                            </div>
                          </template>
                          <div v-else class="text-[11px] text-slate-400 italic">{{ t('console.noRemotePush') }}</div>
                        </div>
                        <button
                          v-if="remotePathOf(rec).length > 0"
                          @click.stop="copyToClipboard(remotePathOf(rec).join('\n'))"
                          class="opacity-0 group-hover/card:opacity-100 text-slate-300 hover:text-blue-500 transition-all shrink-0 mt-0.5"
                        >
                          <Copy class="w-3 h-3" />
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              </Transition>
            </div>
          </div>

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
  </div>
</template>
