<script setup lang="ts">
import { computed } from 'vue';
import { Trash2, Activity, Server, Clock } from 'lucide-vue-next';
import type { TaskGroupListItem, TaskSummaryStatus, ServerRollup, AttemptStatus } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  rows: TaskGroupListItem[];
  selectedTaskGroupId: string | null;
}>();

const emit = defineEmits<{
  select: [taskGroupId: string];
  clear: [taskGroupId: string];
  clearAll: [];
}>();

const { t } = useI18n();

const tableGridStyle = {
  gridTemplateColumns: '140px 1fr 100px 100px 100px minmax(120px, auto) 80px 80px',
  minWidth: '900px',
};

function isTerminal(status: TaskSummaryStatus): boolean {
  return status === 'completed' || status === 'failed' || status === 'cancelled' || status === 'interrupted' || status === 'partial_failed';
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
  if (!seconds || seconds <= 0 || !isFinite(seconds)) return '-';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return `${m}m${s}s`;
}

function statusLabel(status: TaskSummaryStatus): string {
  const map: Record<TaskSummaryStatus, string> = {
    queued: t('console.phaseQueued'),
    copying: t('console.phaseCopying'),
    copy_completed: t('console.phaseCopyCompleted'),
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
    copy_completed: 'bg-cyan-50 text-cyan-700 ring-cyan-200',
    deploying: 'bg-purple-50 text-purple-700 ring-purple-200',
    partial_failed: 'bg-amber-50 text-amber-700 ring-amber-200',
    completed: 'bg-emerald-50 text-emerald-700 ring-emerald-200',
    failed: 'bg-rose-50 text-rose-600 ring-rose-200',
    cancelled: 'bg-red-50 text-red-600 ring-red-200',
    interrupted: 'bg-orange-50 text-orange-600 ring-orange-200',
  };
  return map[status] ?? 'bg-slate-100 text-slate-600 ring-slate-200';
}

function copyStatusLabel(status: string): string {
  const map: Record<string, string> = {
    pending: t('console.phaseQueued'),
    running: t('console.phaseCopying'),
    completed: t('console.phaseCompleted'),
    failed: t('console.phaseFailed'),
    cancelled: t('console.phaseCancelled'),
    interrupted: t('console.phaseInterrupted'),
  };
  return map[status] ?? status;
}

function deployStatusLabel(status: string): string {
  const map: Record<string, string> = {
    not_started: '-',
    pending: t('console.phaseQueued'),
    running: t('console.phaseDeploying'),
    completed: t('console.phaseCompleted'),
    partial_failed: t('console.phasePartialFailed'),
    failed: t('console.phaseFailed'),
    cancelled: t('console.phaseCancelled'),
    interrupted: t('console.phaseInterrupted'),
  };
  return map[status] ?? status;
}

function serverDotClass(status: AttemptStatus): string {
  const map: Record<AttemptStatus, string> = {
    running: 'bg-blue-500',
    success: 'bg-emerald-500',
    failed: 'bg-rose-500',
    cancelled: 'bg-red-400',
    interrupted: 'bg-orange-400',
  };
  return map[status] ?? 'bg-slate-400';
}

const hasAnyTerminal = computed(() => props.rows.some(r => isTerminal(r.summary_status)));
</script>

<template>
  <div class="bg-white border border-slate-200 rounded-lg overflow-hidden shadow-sm">
    <div class="overflow-x-auto">
      <!-- Table Header -->
      <div
        class="grid gap-4 px-4 py-3 bg-slate-50 text-xs text-slate-500 font-semibold border-b border-slate-200 select-none items-center"
        :style="tableGridStyle"
      >
        <div>{{ t('console.startTime') }}</div>
        <div>{{ t('console.name') }}</div>
        <div class="text-center">{{ t('console.status') }}</div>
        <div class="text-center">{{ t('console.copyStatus') }}</div>
        <div class="text-center">{{ t('console.deployStatus') }}</div>
        <div class="text-center">{{ t('console.serverStatus') }}</div>
        <div class="text-center">{{ t('console.elapsed') }}</div>
        <div class="text-center">
          <button
            v-if="hasAnyTerminal"
            @click.stop="emit('clearAll')"
            class="text-slate-500 hover:text-red-600 px-1.5 py-0.5 rounded hover:bg-red-50 transition-colors flex items-center gap-1 text-xs mx-auto"
          >
            <Trash2 class="w-3.5 h-3.5" />
            {{ t('console.clearAllGroups') }}
          </button>
          <span v-else>{{ t('console.actions') }}</span>
        </div>
      </div>

      <!-- Table Body -->
      <div class="divide-y divide-slate-100/80">
        <div
          v-for="row in rows"
          :key="row.task_group_id"
          class="grid gap-4 px-4 py-3 items-center text-sm transition-colors cursor-pointer"
          :class="[
            row.task_group_id === selectedTaskGroupId
              ? 'bg-blue-50/60 hover:bg-blue-50'
              : 'hover:bg-slate-50/60',
          ]"
          :style="tableGridStyle"
          @click="emit('select', row.task_group_id)"
        >
          <!-- Start Time -->
          <div class="text-xs text-slate-500 font-mono tabular-nums leading-tight">
            {{ formatStartTime(row.started_at) }}
          </div>

          <!-- Name -->
          <div class="flex items-center gap-1.5 min-w-0" :title="row.display_name">
            <div
              class="w-5 h-5 rounded flex items-center justify-center shrink-0"
              :class="row.merge_key.startsWith('manual') ? 'bg-purple-100 text-purple-600' : 'bg-blue-100 text-blue-600'"
            >
              <Activity class="w-3 h-3" />
            </div>
            <span class="block w-full max-w-[50ch] truncate font-medium text-slate-800 text-[13px]">
              {{ row.display_name }}
            </span>
          </div>

          <!-- Summary Status Badge -->
          <div class="flex justify-center">
            <span
              class="inline-flex items-center px-2 py-1 rounded text-[11px] font-bold ring-1 ring-inset leading-none whitespace-nowrap"
              :class="statusBadgeClass(row.summary_status)"
            >
              {{ statusLabel(row.summary_status) }}
            </span>
          </div>

          <!-- Copy Status -->
          <div class="text-center text-xs text-slate-600">
            {{ copyStatusLabel(row.copy_status) }}
          </div>

          <!-- Deploy Status -->
          <div class="text-center text-xs text-slate-600">
            {{ deployStatusLabel(row.deploy_status) }}
          </div>

          <!-- Server Rollups (colored dots) -->
          <div class="flex justify-center items-center gap-1 flex-wrap">
            <template v-if="row.server_rollups.length > 0">
              <div
                v-for="rollup in row.server_rollups"
                :key="rollup.server_id"
                class="flex items-center gap-1"
                :title="`${rollup.server_name}: ${rollup.latest_status}`"
              >
                <span class="w-2.5 h-2.5 rounded-full inline-block" :class="serverDotClass(rollup.latest_status)"></span>
                <span class="text-[11px] text-slate-500 hidden xl:inline">{{ rollup.server_name }}</span>
              </div>
            </template>
            <span v-else class="text-[11px] text-slate-400">-</span>
          </div>

          <!-- Elapsed -->
          <div class="text-center font-mono text-[13px] text-slate-500 tabular-nums">
            {{ formatDuration(row.elapsed_seconds) }}
          </div>

          <!-- Actions -->
          <div class="flex justify-center">
            <button
              v-if="isTerminal(row.summary_status)"
              @click.stop="emit('clear', row.task_group_id)"
              class="inline-flex items-center justify-center rounded-md border border-slate-200 bg-white text-slate-500 p-1.5 hover:bg-slate-50 hover:border-slate-300 transition-colors active:scale-95"
              :title="t('console.clearGroup')"
            >
              <Trash2 class="w-4 h-4" />
            </button>
            <span v-else class="text-xs text-slate-400">-</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
