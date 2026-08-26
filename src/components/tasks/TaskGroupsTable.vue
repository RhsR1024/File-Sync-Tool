<script setup lang="ts">
import {
  Trash2, Activity, Eye, Pause, PlayCircle, XCircle, RotateCcw, RefreshCw,
} from 'lucide-vue-next';
import type { TaskGroupListItem, TaskSummaryStatus } from '@/lib/tauri';
import { appStore, type ProgressState } from '@/lib/store';
import { useI18n } from 'vue-i18n';

defineProps<{
  rows: TaskGroupListItem[];
  selectedTaskGroupId: string | null;
}>();

const emit = defineEmits<{
  select: [taskGroupId: string];
  clear: [taskGroupId: string];
  pauseRun: [taskGroupId: string, runId: string];
  resumeRun: [taskGroupId: string, runId: string];
  cancelRun: [taskGroupId: string, runId: string];
  retryDeploy: [taskGroupId: string];
  retryRun: [taskGroupId: string];
}>();

const { t } = useI18n();
const startTimeTextClass = 'text-[12px] text-slate-500 font-medium tabular-nums whitespace-nowrap';
const inactiveMetricPlaceholderClass = 'inline-flex w-full items-center justify-center text-[12px] text-slate-300';

function isTerminal(status: TaskSummaryStatus): boolean {
  return status === 'completed' || status === 'failed' || status === 'cancelled'
    || status === 'interrupted' || status === 'partial_failed';
}

function isActive(status: TaskSummaryStatus): boolean {
  return status === 'queued' || status === 'copying' || status === 'paused'
    || status === 'cancelling' || status === 'copy_completed'
    || status === 'local_executing' || status === 'deploying';
}

function formatStartTime(isoStr: string): string {
  const d = new Date(isoStr);
  const year = d.getFullYear();
  const month = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  const hour = String(d.getHours()).padStart(2, '0');
  const min = String(d.getMinutes()).padStart(2, '0');
  const sec = String(d.getSeconds()).padStart(2, '0');
  return `${year}-${month}-${day} ${hour}:${min}:${sec}`;
}

function formatDuration(seconds: number): string {
  if (seconds == null || seconds < 0 || !isFinite(seconds)) return '-';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return `${m}m${s}s`;
}

function statusLabel(status: TaskSummaryStatus): string {
  const map: Record<TaskSummaryStatus, string> = {
    queued: t('console.phaseQueued'),
    copying: t('console.phaseCopying'),
    paused: t('console.phasePaused'),
    cancelling: t('console.phaseCancelling'),
    copy_completed: t('console.phaseCopyCompleted'),
    local_executing: t('console.phaseLocalExecuting'),
    deploying: t('console.phaseDeploying'),
    partial_failed: t('console.phasePartialFailed'),
    completed: t('console.phaseCompleted'),
    failed: t('console.phaseFailed'),
    cancelled: t('console.phaseCancelled'),
    interrupted: t('console.phaseInterrupted'),
  };
  return map[status] ?? status;
}

function statusBadgeClass(status: TaskSummaryStatus): string {
  const map: Record<TaskSummaryStatus, string> = {
    queued: 'bg-slate-100 text-slate-600 ring-slate-200',
    copying: 'bg-blue-50 text-blue-700 ring-blue-200',
    paused: 'bg-amber-50 text-amber-700 ring-amber-200',
    cancelling: 'bg-orange-50 text-orange-700 ring-orange-200',
    copy_completed: 'bg-cyan-50 text-cyan-700 ring-cyan-200',
    local_executing: 'bg-indigo-50 text-indigo-700 ring-indigo-200',
    deploying: 'bg-purple-50 text-purple-700 ring-purple-200',
    partial_failed: 'bg-amber-50 text-amber-700 ring-amber-200',
    completed: 'bg-emerald-50 text-emerald-700 ring-emerald-200',
    failed: 'bg-rose-50 text-rose-600 ring-rose-200',
    cancelled: 'bg-red-50 text-red-600 ring-red-200',
    interrupted: 'bg-orange-50 text-orange-600 ring-orange-200',
  };
  return map[status] ?? 'bg-slate-100 text-slate-600 ring-slate-200';
}

function progressBarClass(status: TaskSummaryStatus): string {
  const map: Record<TaskSummaryStatus, string> = {
    queued: 'bg-gradient-to-r from-slate-300 to-slate-400',
    copying: 'bg-gradient-to-r from-blue-400 to-blue-600',
    paused: 'bg-gradient-to-r from-amber-300 to-amber-400',
    cancelling: 'bg-gradient-to-r from-orange-400 to-orange-500',
    copy_completed: 'bg-gradient-to-r from-cyan-400 to-cyan-600',
    local_executing: 'bg-gradient-to-r from-indigo-400 to-indigo-600',
    deploying: 'bg-gradient-to-r from-purple-400 to-purple-600',
    partial_failed: 'bg-gradient-to-r from-amber-400 to-amber-500',
    completed: 'bg-gradient-to-r from-emerald-400 to-emerald-500',
    failed: 'bg-gradient-to-r from-rose-400 to-rose-500',
    cancelled: 'bg-gradient-to-r from-red-400 to-red-500',
    interrupted: 'bg-gradient-to-r from-orange-400 to-orange-500',
  };
  return map[status] ?? 'bg-slate-300';
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatSizePair(copied: number, total: number): string {
  if (total <= 0) return '-';
  const MB = 1024 * 1024;
  const GB = 1024 * 1024 * 1024;
  if (total >= GB) {
    return `${(copied / GB).toFixed(2)}GB/${(total / GB).toFixed(2)}GB`;
  }
  if (total >= MB) {
    return `${(copied / MB).toFixed(1)}MB/${(total / MB).toFixed(1)}MB`;
  }
  return `${(copied / 1024).toFixed(1)}KB/${(total / 1024).toFixed(1)}KB`;
}

function formatEta(seconds: number): string {
  if (!seconds || seconds <= 0 || !isFinite(seconds)) return '-';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.round(seconds % 60);
    return `${m}m${s}s`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.round((seconds % 3600) / 60);
  return `${h}h${m}m`;
}

function getRowProgress(row: TaskGroupListItem): ProgressState | null {
  const s = row.summary_status;
  const showsProgress = s === 'copying' || s === 'paused' || s === 'cancelling';
  if (!showsProgress || !appStore.progress) return null;
  if (appStore.progress.folder === row.folder_name) return appStore.progress;
  return null;
}

function getLiveElapsed(row: TaskGroupListItem): number {
  const s = row.summary_status;
  // Terminal / paused: freeze on backend-reported elapsed
  if (s === 'completed' || s === 'failed' || s === 'cancelled' || s === 'interrupted'
      || s === 'partial_failed' || s === 'paused') {
    return row.elapsed_seconds;
  }
  // Actively copying: backend emits live elapsed every 500ms via copy-progress event
  if (s === 'copying' && appStore.progress && appStore.progress.folder === row.folder_name) {
    // Read nowTick so Vue tracks it as a reactivity dep (updates 1/sec even if no new progress yet)
    void appStore.nowTick;
    return Math.max(row.elapsed_seconds, Math.round(appStore.progress.elapsed));
  }
  // Other active phases (cancelling, copy_completed, local_executing, deploying, queued):
  // derive from started_at against the 1Hz live ticker
  const start = Date.parse(row.started_at);
  if (isNaN(start)) return row.elapsed_seconds;
  const liveDelta = Math.max(0, Math.floor((appStore.nowTick - start) / 1000));
  return Math.max(row.elapsed_seconds, liveDelta);
}

function isLiveCopying(row: TaskGroupListItem): boolean {
  return row.summary_status === 'copying';
}

function progressPercent(row: TaskGroupListItem): number {
  const p = getRowProgress(row);
  if (p) return Math.min(100, Math.max(0, p.percentage));
  if (row.summary_status === 'completed') return 100;
  return 0;
}

function progressPercentText(row: TaskGroupListItem): string {
  if (row.summary_status === 'completed') return '100%';
  const p = getRowProgress(row);
  if (!p) return '-';
  return `${Math.min(100, Math.max(0, p.percentage)).toFixed(1)}%`;
}

function progressSizeText(row: TaskGroupListItem): string {
  const p = getRowProgress(row);
  if (!p) return '';
  return formatSizePair(p.copied, p.total);
}
</script>

<template>
  <div class="min-w-0 overflow-hidden rounded-md bg-white">
    <div class="overflow-x-auto">
      <table class="w-full table-fixed" style="min-width: 1150px">
        <colgroup>
          <col style="width: 140px">
          <col style="width: 220px">
          <col style="width: 110px">
          <col style="width: 200px">
          <col style="width: 96px">
          <col style="width: 96px">
          <col style="width: 88px">
          <col style="width: 120px">
          <col style="width: 80px">
        </colgroup>
        <thead>
          <tr class="bg-slate-50/80 text-[11px] text-slate-500 font-semibold uppercase tracking-wider border-b border-slate-200 select-none">
            <th scope="col" class="text-left py-3 px-4">{{ t('console.startTime') }}</th>
            <th scope="col" class="text-left py-3 px-4">{{ t('console.name') }}</th>
            <th scope="col" class="text-center py-3 px-2">{{ t('console.status') }}</th>
            <th scope="col" class="text-left py-3 px-4">{{ t('console.progress') }}</th>
            <th scope="col" class="text-right py-3 px-2">{{ t('console.speed') }}</th>
            <th scope="col" class="text-right py-3 px-2">{{ t('console.eta') }}</th>
            <th scope="col" class="text-center py-3 px-2">{{ t('console.elapsed') }}</th>
            <th scope="col" class="text-center py-3 px-2">{{ t('console.actions') }}</th>
            <th scope="col" class="text-center py-3 px-2">{{ t('console.taskDetail') }}</th>
          </tr>
        </thead>

        <tbody class="divide-y divide-slate-100/70">
          <tr
            v-for="row in rows"
            :key="row.task_group_id"
            class="group transition-colors relative"
            :class="[
              row.task_group_id === selectedTaskGroupId
                ? 'bg-blue-50/50'
                : 'hover:bg-slate-50/50',
            ]"
          >
            <!-- Start Time -->
            <td class="py-2.5 px-3 align-middle">
              <span :class="startTimeTextClass">
                {{ formatStartTime(row.started_at) }}
              </span>
            </td>

            <!-- Name -->
            <td class="py-2.5 px-3 align-middle">
              <div class="flex items-center gap-2 min-w-0">
                <div
                  class="w-5 h-5 rounded-md flex items-center justify-center shrink-0"
                  :class="row.merge_key.startsWith('manual')
                    ? 'bg-purple-100 text-purple-500'
                    : 'bg-blue-100 text-blue-500'"
                >
                  <Activity class="w-3 h-3" />
                </div>
                <span
                  class="truncate font-medium text-slate-700 text-[13px] min-w-0 flex-1"
                  :title="row.display_name"
                >
                  {{ row.display_name }}
                </span>
              </div>
            </td>

            <!-- Status -->
            <td class="py-2.5 px-2 align-middle text-center">
              <span
                class="inline-flex items-center px-2 py-0.5 rounded text-[11px] font-semibold ring-1 ring-inset whitespace-nowrap"
                :class="statusBadgeClass(row.summary_status)"
              >
                {{ statusLabel(row.summary_status) }}
              </span>
            </td>

            <!-- Progress: % + size pair + bar -->
            <td class="py-2.5 px-3 align-middle">
              <template v-if="getRowProgress(row) || row.summary_status === 'completed'">
                <div class="flex items-center justify-between gap-2 mb-1">
                  <span
                    class="text-[12px] font-mono tabular-nums font-semibold"
                    :class="row.summary_status === 'completed' ? 'text-emerald-600' : 'text-slate-700'"
                  >
                    {{ progressPercentText(row) }}
                  </span>
                  <span class="text-[11px] font-mono tabular-nums text-slate-500 truncate">
                    {{ progressSizeText(row) }}
                  </span>
                </div>
                <div class="h-1.5 w-full bg-slate-100 rounded-full overflow-hidden">
                  <div
                    class="h-full rounded-full transition-all duration-300 motion-reduce:transition-none"
                    :class="progressBarClass(row.summary_status)"
                    :style="{ width: `${progressPercent(row)}%` }"
                  ></div>
                </div>
              </template>
              <span v-else :class="inactiveMetricPlaceholderClass" aria-hidden="true">--</span>
            </td>

            <!-- Speed -->
            <td class="py-2.5 px-2 align-middle text-right">
              <span v-if="isLiveCopying(row) && getRowProgress(row)" class="text-[12px] font-mono tabular-nums text-blue-600 whitespace-nowrap">
                {{ formatBytes(getRowProgress(row)!.speed) }}/s
              </span>
              <span v-else :class="inactiveMetricPlaceholderClass" aria-hidden="true">--</span>
            </td>

            <!-- ETA -->
            <td class="py-2.5 px-2 align-middle text-right">
              <span v-if="isLiveCopying(row) && getRowProgress(row) && getRowProgress(row)!.eta > 0" class="text-[12px] font-mono tabular-nums text-amber-600 whitespace-nowrap">
                {{ formatEta(getRowProgress(row)!.eta) }}
              </span>
              <span v-else :class="inactiveMetricPlaceholderClass" aria-hidden="true">--</span>
            </td>

            <!-- Elapsed -->
            <td class="py-2.5 px-2 align-middle text-center">
              <span class="text-[12px] font-mono tabular-nums text-slate-500 whitespace-nowrap">
                {{ formatDuration(getLiveElapsed(row)) }}
              </span>
            </td>

            <!-- Actions -->
            <td class="py-3 px-2 align-middle">
              <div class="flex items-center justify-center gap-1">
                <!-- Active task: pause⇄resume toggle + cancel -->
                <template v-if="isActive(row.summary_status) && row.latest_run_id">
                  <button
                    v-if="row.summary_status === 'paused'"
                    @click.stop="emit('resumeRun', row.task_group_id, row.latest_run_id!)"
                    class="inline-flex w-8 h-8 items-center justify-center rounded-md text-emerald-500 hover:text-emerald-700 hover:bg-emerald-50 transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/50 focus-visible:ring-offset-1"
                    :title="t('console.resume')"
                    :aria-label="t('console.resume')"
                  >
                    <PlayCircle class="w-4 h-4" aria-hidden="true" />
                  </button>
                  <button
                    v-else
                    @click.stop="emit('pauseRun', row.task_group_id, row.latest_run_id!)"
                    class="inline-flex w-8 h-8 items-center justify-center rounded-md text-amber-500 hover:text-amber-700 hover:bg-amber-50 transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500/50 focus-visible:ring-offset-1"
                    :title="t('console.pause')"
                    :aria-label="t('console.pause')"
                  >
                    <Pause class="w-4 h-4" aria-hidden="true" />
                  </button>
                  <button
                    @click.stop="emit('cancelRun', row.task_group_id, row.latest_run_id!)"
                    class="inline-flex w-8 h-8 items-center justify-center rounded-md text-red-400 hover:text-red-600 hover:bg-red-50 transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500/50 focus-visible:ring-offset-1"
                    :title="t('console.cancel')"
                    :aria-label="t('console.cancel')"
                  >
                    <XCircle class="w-4 h-4" aria-hidden="true" />
                  </button>
                </template>
                <!-- Terminal: retry deploy (if had_failures) / retry run (if cancelled or interrupted) + clear -->
                <template v-else-if="isTerminal(row.summary_status)">
                  <button
                    v-if="row.summary_status === 'cancelled' || row.summary_status === 'interrupted'"
                    @click.stop="emit('retryRun', row.task_group_id)"
                    class="inline-flex w-8 h-8 items-center justify-center rounded-md bg-emerald-50 text-emerald-600 hover:bg-emerald-100 hover:text-emerald-700 border border-emerald-200 transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/50 focus-visible:ring-offset-1"
                    :title="t('console.retryRun')"
                    :aria-label="t('console.retryRun')"
                  >
                    <RefreshCw class="w-4 h-4" aria-hidden="true" />
                  </button>
                  <button
                    v-else-if="row.had_failures && row.task_config_id"
                    @click.stop="emit('retryDeploy', row.task_group_id)"
                    class="inline-flex w-8 h-8 items-center justify-center rounded-md bg-amber-50 text-amber-600 hover:bg-amber-100 hover:text-amber-700 border border-amber-200 transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500/50 focus-visible:ring-offset-1"
                    :title="t('console.retryDeploy')"
                    :aria-label="t('console.retryDeploy')"
                  >
                    <RotateCcw class="w-4 h-4" aria-hidden="true" />
                  </button>
                  <span v-else class="inline-block w-8 h-8 shrink-0" aria-hidden="true"></span>
                  <button
                    @click.stop="emit('clear', row.task_group_id)"
                    class="inline-flex w-8 h-8 items-center justify-center rounded-md text-slate-400 hover:text-red-500 hover:bg-red-50 transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500/50 focus-visible:ring-offset-1"
                    :title="t('console.clearGroup')"
                    :aria-label="t('console.clearGroup')"
                  >
                    <Trash2 class="w-4 h-4" aria-hidden="true" />
                  </button>
                </template>
                <span v-else class="text-[11px] text-slate-300" aria-hidden="true">—</span>
              </div>
            </td>

            <!-- Detail: clickable "view" button -->
            <td class="py-3 px-2 align-middle text-center">
              <button
                @click.stop="emit('select', row.task_group_id)"
                class="inline-flex w-8 h-8 items-center justify-center rounded-md text-slate-500 hover:text-blue-600 hover:bg-blue-50 transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-1"
                :title="t('console.viewPathInfo')"
                :aria-label="t('console.viewPathInfo')"
              >
                <Eye class="w-4 h-4" aria-hidden="true" />
              </button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
