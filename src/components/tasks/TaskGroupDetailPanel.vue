<script setup lang="ts">
import { computed, ref } from 'vue';
import {
  X, RotateCcw, Pause, PlayCircle, XCircle, Server, Clock,
  FolderOpen, HardDrive, AlertTriangle, ClipboardCopy, Check, ExternalLink,
} from 'lucide-vue-next';
import type {
  TaskGroup, TaskLogEntry, TaskRun, ServerRollup,
  TaskSummaryStatus, AttemptStatus, DeployState,
} from '@/lib/tauri';
import { openPathParent } from '@/lib/tauri';
import { buildTaskDetailSections } from '@/lib/taskStatusView';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  group: TaskGroup | null;
  taskLogs: TaskLogEntry[];
  isLoading: boolean;
}>();

const emit = defineEmits<{
  retryDeploy: [taskGroupId: string];
  pauseRun: [taskGroupId: string, runId: string];
  resumeRun: [taskGroupId: string, runId: string];
  cancelRun: [taskGroupId: string, runId: string];
  close: [];
}>();

const { t } = useI18n();

const copiedField = ref<string | null>(null);

async function copyToClipboard(text: string, field: string) {
  try {
    await navigator.clipboard.writeText(text);
    copiedField.value = field;
    setTimeout(() => {
      if (copiedField.value === field) copiedField.value = null;
    }, 1500);
  } catch {
    // fallback
  }
}

async function openFolder(path: string) {
  if (!path) return;
  try {
    await openPathParent(path);
  } catch {
    // silently ignore; user can still copy the path
  }
}

const sections = computed(() => {
  if (!props.group) return null;
  return buildTaskDetailSections(props.group);
});

const filteredLogs = computed(() => {
  if (!props.group) return [];
  return props.taskLogs.filter(log => log.task_group_id === props.group!.task_group_id);
});

const latestRun = computed<TaskRun | null>(() => {
  if (!props.group || props.group.runs.length === 0) return null;
  return props.group.runs[props.group.runs.length - 1];
});

const isActive = computed(() => {
  if (!props.group) return false;
  const s = props.group.summary_status;
  return s === 'queued' || s === 'copying' || s === 'paused' || s === 'cancelling'
    || s === 'copy_completed' || s === 'local_executing' || s === 'deploying';
});

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

function serverStatusDotClass(status: AttemptStatus): string {
  const map: Record<AttemptStatus, string> = {
    running: 'bg-blue-500',
    success: 'bg-emerald-500',
    failed: 'bg-rose-500',
    cancelled: 'bg-red-400',
    interrupted: 'bg-orange-400',
  };
  return map[status] ?? 'bg-slate-400';
}

function deployStatusLabel(status: DeployState): string {
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

function localExecStatusLabel(status: string): string {
  const map: Record<string, string> = {
    not_started: '-',
    running: t('console.phaseLocalExecuting'),
    completed: t('console.phaseCompleted'),
    partial_failed: t('console.phasePartialFailed'),
    failed: t('console.phaseFailed'),
    cancelled: t('console.phaseCancelled'),
    interrupted: t('console.phaseInterrupted'),
  };
  return map[status] ?? status;
}

function formatTime(isoStr: string): string {
  const d = new Date(isoStr);
  const h = String(d.getHours()).padStart(2, '0');
  const m = String(d.getMinutes()).padStart(2, '0');
  const s = String(d.getSeconds()).padStart(2, '0');
  return `${h}:${m}:${s}`;
}

function formatFullTime(isoStr: string): string {
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

function runTypeLabel(runType: string): string {
  const map: Record<string, string> = {
    copy_and_deploy: t('console.runTypeCopyAndDeploy'),
    deploy_retry: t('console.runTypeDeployRetry'),
    manual_deploy: t('console.runTypeManualDeploy'),
  };
  return map[runType] ?? runType;
}

function logLevelClass(level: string): string {
  if (level === 'error') return 'text-rose-600';
  if (level === 'warn') return 'text-amber-600';
  if (level === 'success') return 'text-emerald-600';
  if (level === 'command') return 'text-purple-600';
  return 'text-slate-600';
}
</script>

<template>
  <!-- Centered Modal -->
  <Teleport to="body">
    <Transition
      enter-active-class="transition-opacity duration-200"
      leave-active-class="transition-opacity duration-150"
      enter-from-class="opacity-0"
      leave-to-class="opacity-0"
    >
      <div
        v-if="group"
        class="fixed inset-0 z-[100] flex items-center justify-center p-6"
      >
        <!-- Backdrop -->
        <div
          class="absolute inset-0 bg-slate-950/40 backdrop-blur-[2px]"
          @click="emit('close')"
        ></div>

        <!-- Modal -->
        <Transition
          appear
          enter-active-class="transition-all duration-200 ease-out"
          enter-from-class="opacity-0 scale-95 translate-y-2"
        >
          <div
            class="relative z-10 w-full max-w-2xl max-h-[85vh] bg-white rounded-2xl shadow-2xl shadow-slate-900/10 border border-slate-200/80 flex flex-col overflow-hidden"
          >
            <!-- Header -->
            <div class="flex items-center justify-between gap-4 border-b border-slate-100 px-6 py-4 shrink-0 bg-slate-50/50">
              <div class="flex items-center gap-3 min-w-0">
                <h3 class="text-base font-bold text-slate-800 shrink-0">{{ t('console.taskDetail') }}</h3>
                <span class="text-slate-300">|</span>
                <span class="truncate text-sm text-slate-500" :title="group.display_name">
                  {{ group.display_name }}
                </span>
                <span
                  class="inline-flex items-center px-2 py-0.5 rounded text-[10px] font-bold ring-1 ring-inset leading-none whitespace-nowrap shrink-0"
                  :class="statusBadgeClass(group.summary_status)"
                >
                  {{ statusLabel(group.summary_status) }}
                </span>
              </div>
              <button
                @click="emit('close')"
                class="rounded-lg p-1.5 text-slate-400 transition-colors hover:bg-slate-200/60 hover:text-slate-600 shrink-0"
                :title="t('console.closeDetail')"
              >
                <X class="h-4 w-4" />
              </button>
            </div>

            <!-- Loading state -->
            <div v-if="isLoading" class="flex-1 flex items-center justify-center py-16">
              <div class="text-slate-400 text-sm">Loading...</div>
            </div>

            <!-- Content -->
            <div v-else class="flex-1 overflow-y-auto px-6 py-5 space-y-4">
              <!-- Path info cards -->
              <div class="grid gap-3 sm:grid-cols-2">
                <div class="rounded-lg border border-slate-200 bg-slate-50/70 p-3">
                  <div class="flex items-center justify-between text-[11px] font-semibold text-slate-500 mb-1.5">
                    <span class="flex items-center gap-1.5 uppercase tracking-wider">
                      <FolderOpen class="w-3 h-3 text-orange-500" />
                      {{ t('console.sourcePath') }}
                    </span>
                    <div v-if="group.source_path" class="flex items-center gap-1">
                      <button
                        @click="openFolder(group.source_path)"
                        class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium transition-all text-slate-400 hover:text-blue-600 hover:bg-blue-50"
                        :title="t('settings.openFolder')"
                      >
                        <ExternalLink class="w-3 h-3" />
                        {{ t('settings.openFolder') }}
                      </button>
                      <button
                        @click="copyToClipboard(group.source_path, 'source')"
                        class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium transition-all"
                        :class="copiedField === 'source'
                          ? 'text-emerald-600 bg-emerald-50'
                          : 'text-slate-400 hover:text-blue-600 hover:bg-blue-50'"
                      >
                        <Check v-if="copiedField === 'source'" class="w-3 h-3" />
                        <ClipboardCopy v-else class="w-3 h-3" />
                        {{ copiedField === 'source' ? t('console.copiedPath') : t('console.copyPath') }}
                      </button>
                    </div>
                  </div>
                  <div class="break-all font-mono text-[11px] leading-5 text-slate-600">
                    {{ group.source_path || '-' }}
                  </div>
                </div>
                <div class="rounded-lg border border-slate-200 bg-slate-50/70 p-3">
                  <div class="flex items-center justify-between text-[11px] font-semibold text-slate-500 mb-1.5">
                    <span class="flex items-center gap-1.5 uppercase tracking-wider">
                      <HardDrive class="w-3 h-3 text-blue-500" />
                      {{ t('console.localCopyPath') }}
                    </span>
                    <div v-if="group.local_target_path" class="flex items-center gap-1">
                      <button
                        @click="openFolder(group.local_target_path)"
                        class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium transition-all text-slate-400 hover:text-blue-600 hover:bg-blue-50"
                        :title="t('settings.openFolder')"
                      >
                        <ExternalLink class="w-3 h-3" />
                        {{ t('settings.openFolder') }}
                      </button>
                      <button
                        @click="copyToClipboard(group.local_target_path, 'local')"
                        class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium transition-all"
                        :class="copiedField === 'local'
                          ? 'text-emerald-600 bg-emerald-50'
                          : 'text-slate-400 hover:text-blue-600 hover:bg-blue-50'"
                      >
                        <Check v-if="copiedField === 'local'" class="w-3 h-3" />
                        <ClipboardCopy v-else class="w-3 h-3" />
                        {{ copiedField === 'local' ? t('console.copiedPath') : t('console.copyPath') }}
                      </button>
                    </div>
                  </div>
                  <div class="break-all font-mono text-[11px] leading-5 text-slate-600">
                    {{ group.local_target_path || '-' }}
                  </div>
                </div>
              </div>

              <!-- Local Exec & Deploy Status Cards (copy status removed — already shown in header badge) -->
              <div
                v-if="group.local_exec_status !== 'not_started' || group.deploy_status !== 'not_started'"
                class="grid gap-3"
                :class="group.local_exec_status !== 'not_started' ? 'sm:grid-cols-2' : 'sm:grid-cols-1'"
              >
                <div v-if="group.local_exec_status !== 'not_started'" class="rounded-lg border border-slate-200 p-3">
                  <div class="text-[11px] font-semibold text-slate-400 uppercase tracking-wider mb-1.5">{{ t('console.phaseLocalScripts') }}</div>
                  <div class="text-sm font-medium text-slate-700">{{ localExecStatusLabel(group.local_exec_status) }}</div>
                </div>
                <div v-if="group.deploy_status !== 'not_started'" class="rounded-lg border border-slate-200 p-3">
                  <div class="text-[11px] font-semibold text-slate-400 uppercase tracking-wider mb-1.5">{{ t('console.deployStatus') }}</div>
                  <div class="text-sm font-medium text-slate-700">{{ deployStatusLabel(group.deploy_status) }}</div>
                </div>
              </div>

              <!-- Server Rollups -->
              <div v-if="group.server_rollups.length > 0" class="rounded-lg border border-slate-200 p-4">
                <div class="flex items-center gap-2 text-sm font-semibold text-slate-700 mb-3">
                  <Server class="h-4 w-4 text-indigo-500" />
                  {{ t('console.serverStatus') }}
                </div>
                <div class="flex flex-col gap-2">
                  <div
                    v-for="rollup in group.server_rollups"
                    :key="rollup.server_id"
                    class="flex items-center gap-3 rounded-lg border px-3 py-2 text-[12px]"
                    :class="rollup.latest_status === 'failed'
                      ? 'border-rose-200 bg-rose-50'
                      : rollup.latest_status === 'success'
                        ? 'border-emerald-200 bg-emerald-50'
                        : 'border-blue-200 bg-blue-50'"
                  >
                    <span class="w-2.5 h-2.5 rounded-full shrink-0" :class="serverStatusDotClass(rollup.latest_status)"></span>
                    <span class="flex-1 font-mono text-slate-700 truncate">{{ rollup.server_name }}</span>
                    <span class="text-[11px] text-slate-500">
                      {{ rollup.success_count }} / {{ rollup.success_count + rollup.failure_count }}
                    </span>
                    <span
                      class="inline-flex items-center px-2 py-0.5 rounded text-[10px] font-bold ring-1 ring-inset whitespace-nowrap"
                      :class="rollup.latest_status === 'failed'
                        ? 'bg-rose-100 text-rose-700 ring-rose-300'
                        : rollup.latest_status === 'success'
                          ? 'bg-emerald-100 text-emerald-700 ring-emerald-300'
                          : 'bg-blue-100 text-blue-700 ring-blue-300'"
                    >
                      {{ rollup.latest_status }}
                    </span>
                  </div>
                </div>
              </div>

              <!-- Server Failures -->
              <div v-if="sections && sections.serverFailures.length > 0" class="rounded-lg border border-amber-200 bg-amber-50 p-4">
                <div class="flex items-center gap-2 text-sm font-semibold text-amber-800 mb-3">
                  <AlertTriangle class="h-4 w-4" />
                  {{ t('console.serverFailures') }}
                </div>
                <div class="space-y-2">
                  <div
                    v-for="failure in sections.serverFailures"
                    :key="failure.serverId"
                    class="rounded border border-amber-200 bg-white px-3 py-2 text-[12px]"
                  >
                    <span class="font-semibold text-slate-700">{{ failure.serverName }}:</span>
                    <span class="text-rose-600 ml-1">{{ failure.message }}</span>
                  </div>
                </div>
              </div>

              <!-- Action buttons -->
              <div v-if="group.had_failures || isActive" class="flex gap-2 flex-wrap">
                <button
                  v-if="group.had_failures"
                  @click="emit('retryDeploy', group.task_group_id)"
                  class="px-4 py-2 rounded-lg font-bold bg-amber-50 text-amber-700 border border-amber-200 hover:bg-amber-100 transition-all flex items-center gap-2 text-sm active:scale-95"
                >
                  <RotateCcw class="w-4 h-4" />
                  {{ t('console.retryDeploy') }}
                </button>
                <template v-if="isActive && latestRun">
                  <button
                    @click="emit('pauseRun', group.task_group_id, latestRun.run_id)"
                    class="px-3 py-2 rounded-lg font-bold border border-amber-200 bg-amber-50 text-amber-700 hover:bg-amber-100 transition-all flex items-center gap-1.5 text-sm active:scale-95"
                  >
                    <Pause class="w-4 h-4" />
                    {{ t('console.pause') }}
                  </button>
                  <button
                    @click="emit('resumeRun', group.task_group_id, latestRun.run_id)"
                    class="px-3 py-2 rounded-lg font-bold border border-emerald-200 bg-emerald-50 text-emerald-700 hover:bg-emerald-100 transition-all flex items-center gap-1.5 text-sm active:scale-95"
                  >
                    <PlayCircle class="w-4 h-4" />
                    {{ t('console.resume') }}
                  </button>
                  <button
                    @click="emit('cancelRun', group.task_group_id, latestRun.run_id)"
                    class="px-3 py-2 rounded-lg font-bold border border-red-200 bg-red-50 text-red-600 hover:bg-red-100 transition-all flex items-center gap-1.5 text-sm active:scale-95"
                  >
                    <XCircle class="w-4 h-4" />
                    {{ t('console.cancel') }}
                  </button>
                </template>
              </div>

              <!-- Run History -->
              <div v-if="sections && sections.runs.length > 0" class="rounded-lg border border-slate-200 p-4">
                <div class="flex items-center gap-2 text-sm font-semibold text-slate-700 mb-3">
                  <Clock class="h-4 w-4 text-slate-500" />
                  {{ t('console.runHistory') }}
                </div>
                <div class="overflow-x-auto">
                  <table class="w-full text-xs">
                    <thead>
                      <tr class="text-slate-500 border-b border-slate-200">
                        <th class="text-left py-2 px-2 font-semibold">{{ t('console.runType') }}</th>
                        <th class="text-left py-2 px-2 font-semibold">{{ t('console.copyStatus') }}</th>
                        <th class="text-left py-2 px-2 font-semibold">{{ t('console.phaseLocalScripts') }}</th>
                        <th class="text-left py-2 px-2 font-semibold">{{ t('console.deployStatus') }}</th>
                        <th class="text-left py-2 px-2 font-semibold">{{ t('console.startTime') }}</th>
                        <th class="text-left py-2 px-2 font-semibold">{{ t('console.endTime') }}</th>
                      </tr>
                    </thead>
                    <tbody class="divide-y divide-slate-100">
                      <tr v-for="run in sections.runs" :key="run.run_id" class="hover:bg-slate-50">
                        <td class="py-2 px-2 text-slate-700 font-mono">{{ runTypeLabel(run.run_type) }}</td>
                        <td class="py-2 px-2 text-slate-600">{{ copyStatusLabel(run.copy_phase) }}</td>
                        <td class="py-2 px-2 text-slate-600">{{ localExecStatusLabel(run.local_exec_phase) }}</td>
                        <td class="py-2 px-2 text-slate-600">{{ deployStatusLabel(run.deploy_phase) }}</td>
                        <td class="py-2 px-2 text-slate-500 font-mono tabular-nums">{{ formatFullTime(run.started_at) }}</td>
                        <td class="py-2 px-2 text-slate-500 font-mono tabular-nums">{{ run.finished_at ? formatFullTime(run.finished_at) : '-' }}</td>
                      </tr>
                    </tbody>
                  </table>
                </div>
              </div>

              <!-- Task Logs -->
              <div v-if="filteredLogs.length > 0" class="rounded-lg border border-slate-200 p-4">
                <div class="text-sm font-semibold text-slate-700 mb-3">{{ t('console.taskLogs') }}</div>
                <div class="max-h-60 overflow-y-auto space-y-1 font-mono text-[12px]">
                  <div
                    v-for="(log, idx) in filteredLogs"
                    :key="idx"
                    class="flex gap-2 leading-5"
                  >
                    <span class="text-slate-400 shrink-0 tabular-nums">{{ formatTime(log.timestamp) }}</span>
                    <span v-if="log.server_name" class="text-indigo-500 shrink-0">[{{ log.server_name }}]</span>
                    <span :class="logLevelClass(log.level)">{{ log.message }}</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
