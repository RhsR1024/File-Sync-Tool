<script setup lang="ts">
import { ref, computed, onMounted, onActivated } from 'vue';
import { RefreshCw, Clock, Activity, Copy, AlertTriangle, FilePlus2, Gauge, ListChecks, Trash2 } from 'lucide-vue-next';
import Empty from '@/components/Empty.vue';
import LoadingSkeleton from '@/components/LoadingSkeleton.vue';
import ManualCopyModal from '@/components/ManualCopyModal.vue';
import TaskGroupsTable from '@/components/tasks/TaskGroupsTable.vue';
import TaskGroupDetailPanel from '@/components/tasks/TaskGroupDetailPanel.vue';
import { getConfig, type AppConfig, previewTemporaryCopy, queueTemporaryCopy, type ManualCopyPreview } from '@/lib/tauri';
import {
  clearTaskGroup,
  clearTaskGroups,
  cancelTaskRun,
  pauseTaskRun,
  resumeTaskRun,
  retryTaskGroupDeploy,
} from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { appStore, addLog } from '@/lib/store';
import { taskStateStore } from '@/lib/taskStateStore';
import { buildTaskRows } from '@/lib/taskStatusView';
import { executeScan, startScheduler } from '@/lib/scheduler';
import { pushToast, type ToastTone } from '@/composables/useToast';

defineOptions({
  name: 'SyncOverviewPage',
});

const { t } = useI18n();
const config = ref<AppConfig | null>(null);
const isManualCopyModalOpen = ref(false);
const manualCopyTriggerRef = ref<HTMLButtonElement | null>(null);

// Tracks whether the first hydrate of taskStateStore has resolved during this
// page lifecycle so we can show a skeleton instead of a blank panel on cold
// boot.
const isInitialLoading = ref(false);

// Pushes a toast through the M01 shared queue. Kept as a thin wrapper so the
// migration from the old local timer is grep-able and the call sites stay
// short.
function notify(message: string, tone: ToastTone = 'info') {
  pushToast(message, tone);
}

// For retry run with target existence check
const retryTargetPreview = ref<ManualCopyPreview | null>(null);
const pendingRetryRequest = ref<{ taskGroupId: string; source: string; target: string } | null>(null);

const rows = computed(() => buildTaskRows(taskStateStore.groups));

const currentSpeed = computed(() => {
  const bytesPerSecond = appStore.progress?.speed ?? 0;
  if (bytesPerSecond <= 0) return '—';
  if (bytesPerSecond < 1024) return `${bytesPerSecond.toFixed(0)} B/s`;
  if (bytesPerSecond < 1024 * 1024) return `${(bytesPerSecond / 1024).toFixed(1)} KB/s`;
  if (bytesPerSecond < 1024 * 1024 * 1024) return `${(bytesPerSecond / (1024 * 1024)).toFixed(1)} MB/s`;
  return `${(bytesPerSecond / (1024 * 1024 * 1024)).toFixed(2)} GB/s`;
});

const terminalTaskCount = computed(() => rows.value.filter(r => {
  const s = r.summary_status;
  return s === 'completed' || s === 'failed' || s === 'cancelled'
    || s === 'interrupted' || s === 'partial_failed';
}).length);

const hasAnyTerminal = computed(() => terminalTaskCount.value > 0);

async function handleSelect(taskGroupId: string) {
  await taskStateStore.selectTaskGroup(taskGroupId);
}

async function handleClearGroup(taskGroupId: string) {
  try {
    await clearTaskGroup(taskGroupId);
    notify(t('console.clearGroup'), 'success');
  } catch (e) {
    addLog(`Clear failed: ${e}`, 'error');
    notify(`${t('console.clearGroup')} - ${e}`, 'error');
  }
}

async function handleClearAll() {
  try {
    await clearTaskGroups();
    notify(t('console.clearAllGroups'), 'success');
  } catch (e) {
    addLog(`Clear all failed: ${e}`, 'error');
    notify(`${t('console.clearAllGroups')} - ${e}`, 'error');
  }
}

async function handlePause(taskGroupId: string, runId: string) {
  try {
    await pauseTaskRun(taskGroupId, runId);
    addLog(t('console.paused'), 'info');
    notify(t('console.paused'), 'success');
  } catch (e) {
    addLog(`Pause failed: ${e}`, 'error');
    notify(`${t('console.pause')} - ${e}`, 'error');
  }
}

async function handleResume(taskGroupId: string, runId: string) {
  try {
    await resumeTaskRun(taskGroupId, runId);
    addLog(t('console.resumed'), 'info');
    notify(t('console.resumed'), 'success');
  } catch (e) {
    addLog(`Resume failed: ${e}`, 'error');
    notify(`${t('console.resume')} - ${e}`, 'error');
  }
}

async function handleCancel(taskGroupId: string, runId: string) {
  try {
    await cancelTaskRun(taskGroupId, runId);
    addLog(t('console.cancelling'), 'info');
    notify(t('console.cancelling'), 'info');
  } catch (e) {
    addLog(`Cancel failed: ${e}`, 'error');
    notify(`${t('console.cancel')} - ${e}`, 'error');
  }
}

async function handleRetryDeploy(taskGroupId: string) {
  try {
    await retryTaskGroupDeploy(taskGroupId);
    addLog(t('console.retryDeploy'), 'info');
    notify(t('console.retryDeploy'), 'success');
  } catch (e) {
    addLog(`Retry deploy failed: ${e}`, 'error');
    notify(`${t('console.retryDeploy')} - ${e}`, 'error');
  }
}

async function handleRetryRun(taskGroupId: string) {
  // Find the task group from the rows
  const taskRow = rows.value.find(r => r.task_group_id === taskGroupId);
  if (!taskRow) {
    notify('Task not found', 'error');
    return;
  }

  // Derive original target_root_path from the persisted local_target_path.
  // local_target_path is always target_root + folder_name, so its parent is the
  // original target_root — passing that back lets the backend merge the retry
  // into the existing task group (via matching merge_key) instead of nesting
  // folder_name again and creating a fresh group each time.
  const targetRoot = deriveTargetRootPath(taskRow.local_target_path);

  try {
    // Preview the copy to check if target exists
    const preview = await previewTemporaryCopy(taskRow.source_path, targetRoot);

    if (preview.target_exists) {
      // Target exists, show dialog for user to choose
      retryTargetPreview.value = preview;
      pendingRetryRequest.value = {
        taskGroupId,
        source: taskRow.source_path,
        target: targetRoot,
      };
    } else {
      // Target doesn't exist, queue directly
      await retryConfirmQueue(taskRow.source_path, targetRoot, false);
    }
  } catch (e) {
    addLog(`Retry run preview failed: ${e}`, 'error');
    notify(`Failed to preview: ${e}`, 'error');
  }
}

function deriveTargetRootPath(localTargetPath: string): string {
  const trimmed = localTargetPath.trim().replace(/[\\/]+$/, '');
  const lastSep = Math.max(trimmed.lastIndexOf('\\'), trimmed.lastIndexOf('/'));
  if (lastSep <= 0) return trimmed;
  // Preserve drive root form like "C:\" instead of collapsing to "C:"
  if (lastSep === 2 && trimmed.charAt(1) === ':') {
    return trimmed.substring(0, 3);
  }
  return trimmed.substring(0, lastSep);
}

function clearRetryTargetDecision() {
  retryTargetPreview.value = null;
  pendingRetryRequest.value = null;
}

function retryDialogSummary(preview: ManualCopyPreview): string {
  if (preview.source_kind === 'file') {
    return t('manualCopy.targetExistsFileDecision', { path: preview.resolved_target_path });
  }
  return t('manualCopy.targetExistsDirectoryDecision', { path: preview.resolved_target_path });
}

function retryOverwriteHint(preview: ManualCopyPreview): string {
  return preview.source_kind === 'file'
    ? t('manualCopy.overwriteFileHint')
    : t('manualCopy.overwriteDirectoryHint');
}

function retrySkipHint(preview: ManualCopyPreview): string {
  return preview.source_kind === 'file'
    ? t('manualCopy.skipFileHint')
    : t('manualCopy.skipDirectoryHint');
}

async function retryConfirmQueue(source: string, target: string, overwriteExisting: boolean) {
  try {
    const ack = await queueTemporaryCopy(source, target, overwriteExisting);

    addLog(t('manualCopy.addedToQueue'), 'success');
    notify(
      ack.queued_ahead > 0
        ? t('manualCopy.addedToQueueWithAhead', { count: ack.queued_ahead })
        : t('manualCopy.addedToQueue'),
      'success'
    );

    clearRetryTargetDecision();
    // Refresh task list
    await taskStateStore.hydrateTaskState();
  } catch (e) {
    addLog(`Queue copy failed: ${e}`, 'error');
    notify(`Failed to queue: ${e}`, 'error');
  }
}

function handleCloseDetail() {
  taskStateStore.selectedTaskGroupId = null;
  taskStateStore.selectedGroupDetail = null;
}

async function handleScanClick() {
  if (appStore.isRunning) return;
  await executeScan();
}

async function loadConfig() {
  try {
    config.value = await getConfig();
  } catch (e) {
    addLog(t('console.failedLoadConfig', { error: e }), 'error');
  }
}

onActivated(() => {
  loadConfig();
});

onMounted(async () => {
  loadConfig();
  if (!taskStateStore.isHydrated) {
    isInitialLoading.value = true;
    try {
      await taskStateStore.hydrateTaskState();
    } finally {
      isInitialLoading.value = false;
    }
  }
});

// Returns focus to the Manual Copy trigger after the modal closes so that
// keyboard users land back on a sensible anchor element.
function handleManualCopyClose() {
  isManualCopyModalOpen.value = false;
  // Wait one tick so Vue has actually unmounted the modal contents before we
  // try to refocus — otherwise the trigger could lose focus again.
  requestAnimationFrame(() => {
    manualCopyTriggerRef.value?.focus();
  });
}
</script>

<template>
  <div class="sync-console-workspace h-full min-h-0 w-full p-6 flex flex-col gap-4 bg-slate-50">
    <section class="sync-overview-summary grid shrink-0 grid-cols-1 overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm sm:grid-cols-2 xl:grid-cols-4">
      <div class="flex min-h-[78px] items-center gap-3 border-b border-slate-100 px-5 py-3 sm:border-r xl:border-b-0">
        <Activity class="h-5 w-5 shrink-0 text-blue-600" aria-hidden="true" />
        <div class="min-w-0">
          <div class="text-[11px] font-semibold uppercase text-slate-500">{{ t('console.status') }}</div>
          <div class="mt-1 flex items-center gap-2 text-xl font-bold" :class="appStore.isRunning ? 'text-emerald-600' : 'text-slate-700'">
            <div class="relative flex h-2.5 w-2.5 shrink-0">
              <span v-if="appStore.isRunning" class="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-75 motion-reduce:animate-none"></span>
              <span class="relative inline-flex h-2.5 w-2.5 rounded-full" :class="appStore.isRunning ? 'bg-emerald-500' : 'bg-slate-400'"></span>
            </div>
            <span class="truncate">{{ appStore.isRunning ? t('console.running') : t('console.stopped') }}</span>
          </div>
        </div>
      </div>

      <div class="flex min-h-[78px] items-center gap-3 border-b border-slate-100 px-5 py-3 xl:border-b-0 xl:border-r">
        <Clock class="h-5 w-5 shrink-0 text-slate-500" aria-hidden="true" />
        <div class="min-w-0">
          <div class="text-[11px] font-semibold uppercase text-slate-500">{{ t('console.nextRun') }}</div>
          <div class="mt-1 truncate font-mono text-xl font-bold tabular-nums text-slate-900">
            {{ appStore.nextRunTime }}
          </div>
        </div>
      </div>

      <div class="flex min-h-[78px] items-center gap-3 border-b border-slate-100 px-5 py-3 sm:border-b-0 sm:border-r">
        <Gauge class="h-5 w-5 shrink-0 text-emerald-600" aria-hidden="true" />
        <div class="min-w-0">
          <div class="text-[11px] font-semibold uppercase text-slate-500">{{ t('console.speed') }}</div>
          <div class="mt-1 truncate font-mono text-xl font-bold tabular-nums text-slate-900">{{ currentSpeed }}</div>
        </div>
      </div>

      <div class="flex min-h-[78px] items-center gap-3 px-5 py-3">
        <ListChecks class="h-5 w-5 shrink-0 text-indigo-600" aria-hidden="true" />
        <div class="min-w-0">
          <div class="text-[11px] font-semibold uppercase text-slate-500">{{ t('console.taskRecords') }}</div>
          <div class="mt-1 text-xl font-bold tabular-nums text-slate-900">{{ rows.length }}</div>
        </div>
      </div>
    </section>

    <section class="sync-overview-panel flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-slate-200 bg-white shadow-sm">
      <div class="flex flex-col gap-3 border-b border-slate-200 bg-white px-4 py-3 lg:flex-row lg:items-center lg:justify-between">
        <div class="min-w-0">
          <h2 class="text-lg font-bold text-slate-900">{{ t('console.taskRecords') }}</h2>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <button
            @click="handleScanClick"
            class="flex min-h-11 items-center gap-2 rounded-lg border border-blue-200 bg-white px-3.5 py-2 text-sm font-semibold text-blue-700 shadow-sm transition-colors hover:border-blue-300 hover:bg-blue-50 motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50 focus-visible:ring-offset-2 disabled:hover:border-blue-200 disabled:hover:bg-white"
            :disabled="appStore.isRunning"
            :class="{ 'opacity-50 cursor-not-allowed': appStore.isRunning }"
            :aria-label="t('console.scanNow')"
            :title="t('console.scanNow')"
          >
            <RefreshCw class="h-4 w-4 motion-reduce:animate-none" :class="{ 'animate-spin': appStore.isRunning }" aria-hidden="true" />
            {{ t('console.scanNow') }}
          </button>

          <button
            ref="manualCopyTriggerRef"
            @click="isManualCopyModalOpen = true"
            class="flex min-h-11 items-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2 text-sm font-semibold text-slate-700 shadow-sm transition-colors hover:border-blue-200 hover:bg-slate-50 hover:text-blue-700 motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50 focus-visible:ring-offset-2"
            :aria-label="t('manualCopy.title')"
            :title="t('manualCopy.title')"
          >
            <Copy class="h-4 w-4 text-blue-500" aria-hidden="true" />
            {{ t('manualCopy.title') }}
          </button>

          <button
            v-if="hasAnyTerminal"
            @click="handleClearAll"
            class="flex min-h-11 items-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2 text-sm font-semibold text-slate-600 shadow-sm transition-colors hover:border-red-200 hover:bg-red-50 hover:text-red-600 motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500/50 focus-visible:ring-offset-2"
            :aria-label="t('console.clearAllGroups')"
            :title="t('console.clearAllGroups')"
          >
            <Trash2 class="h-4 w-4" aria-hidden="true" />
            {{ t('console.clearAllGroups') }}
          </button>
        </div>
      </div>

      <div class="min-h-0 flex-1 overflow-auto bg-slate-50/70 p-3">
        <div
          v-if="isInitialLoading && !rows.length"
          class="rounded-lg border border-slate-200 bg-white p-4"
          role="status"
          aria-live="polite"
          :aria-label="t('tasks.loading.tasks')"
        >
          <LoadingSkeleton variant="list-row" :count="3" />
        </div>

        <Empty
          v-else-if="!rows.length"
          :icon="Activity"
          :title="appStore.isRunning ? t('console.noTaskGroups') : t('tasks.empty.notRunning')"
          :description="appStore.isRunning ? '' : t('tasks.empty.notRunningHint')"
          :action-label="appStore.isRunning ? undefined : t('tasks.empty.actionStart')"
          class="h-full min-h-[320px]"
          @action="startScheduler"
        />

        <TaskGroupsTable
          v-else
          :rows="rows"
          :selected-task-group-id="taskStateStore.selectedTaskGroupId"
          @select="handleSelect"
          @clear="handleClearGroup"
          @pause-run="handlePause"
          @resume-run="handleResume"
          @cancel-run="handleCancel"
          @retry-deploy="handleRetryDeploy"
          @retry-run="handleRetryRun"
        />
      </div>
    </section>

    <!-- Detail panel (slide-in) -->
    <TaskGroupDetailPanel
      :group="taskStateStore.selectedGroupDetail"
      :task-logs="taskStateStore.taskLogs"
      :is-loading="taskStateStore.isLoadingDetail"
      @retry-deploy="handleRetryDeploy"
      @close="handleCloseDetail"
    />

    <!-- Manual Copy Modal -->
    <ManualCopyModal
      :is-open="isManualCopyModalOpen"
      @close="handleManualCopyClose"
      @success="() => {}"
    />

    <!-- Retry target exists dialog -->
    <Teleport to="body">
      <Transition name="retry-confirm-fade">
        <div
          v-if="retryTargetPreview"
          class="fixed inset-0 z-[100] flex items-center justify-center bg-slate-950/50 backdrop-blur-sm px-4"
          role="dialog"
          aria-modal="true"
          aria-labelledby="retry-dialog-title"
          @click="clearRetryTargetDecision"
        >
          <div
            class="w-full max-w-xl rounded-2xl border border-slate-200 bg-white shadow-2xl shadow-slate-900/20 overflow-hidden"
            @click.stop
          >
            <!-- Header -->
            <div class="flex items-start gap-4 px-6 pt-6 pb-5 border-b border-slate-100">
              <div class="flex-shrink-0 w-10 h-10 rounded-xl bg-amber-50 border border-amber-200 flex items-center justify-center">
                <AlertTriangle class="w-5 h-5 text-amber-600" />
              </div>
              <div class="min-w-0 pt-0.5 flex-1">
                <div id="retry-dialog-title" class="text-base font-semibold text-slate-800">
                  {{ t('manualCopy.targetExistsDecisionTitle') }}
                </div>
                <p class="text-sm leading-6 text-slate-500 mt-1.5 break-all">
                  {{ retryDialogSummary(retryTargetPreview) }}
                </p>
              </div>
            </div>

            <!-- Option cards -->
            <div class="p-4 space-y-2.5">
              <!-- Overwrite (destructive) -->
              <button
                @click="retryConfirmQueue(pendingRetryRequest!.source, pendingRetryRequest!.target, true)"
                class="w-full flex items-start gap-3.5 rounded-xl border border-amber-300 bg-amber-50 px-4 py-3.5 text-left transition-all motion-reduce:transition-none hover:border-amber-400 hover:bg-amber-100 hover:shadow-sm active:scale-[0.99] motion-reduce:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500/60 focus-visible:ring-offset-2"
                :aria-label="t('manualCopy.overwriteAndQueue')"
              >
                <div class="flex-shrink-0 w-8 h-8 rounded-lg bg-amber-600 flex items-center justify-center mt-0.5">
                  <RefreshCw class="w-3.5 h-3.5 text-white" aria-hidden="true" />
                </div>
                <div class="min-w-0 flex-1">
                  <div class="text-sm font-semibold text-amber-800">
                    {{ t('manualCopy.overwriteAndQueue') }}
                  </div>
                  <div class="text-xs text-slate-600 mt-1 leading-5 break-words">
                    {{ retryOverwriteHint(retryTargetPreview) }}
                  </div>
                </div>
              </button>

              <!-- Skip (safe option) -->
              <button
                @click="retryConfirmQueue(pendingRetryRequest!.source, pendingRetryRequest!.target, false)"
                class="w-full flex items-start gap-3.5 rounded-xl border border-emerald-200 bg-emerald-50 px-4 py-3.5 text-left transition-all motion-reduce:transition-none hover:border-emerald-300 hover:bg-emerald-100 hover:shadow-sm active:scale-[0.99] motion-reduce:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/60 focus-visible:ring-offset-2"
                :aria-label="t('manualCopy.skipAndQueue')"
              >
                <div class="flex-shrink-0 w-8 h-8 rounded-lg bg-emerald-600 flex items-center justify-center mt-0.5">
                  <FilePlus2 class="w-3.5 h-3.5 text-white" aria-hidden="true" />
                </div>
                <div class="min-w-0 flex-1">
                  <div class="text-sm font-semibold text-emerald-800">
                    {{ t('manualCopy.skipAndQueue') }}
                  </div>
                  <div class="text-xs text-slate-600 mt-1 leading-5 break-words">
                    {{ retrySkipHint(retryTargetPreview) }}
                  </div>
                </div>
              </button>
            </div>

            <!-- Footer cancel -->
            <div class="px-4 pb-4 flex justify-end border-t border-slate-100 pt-3">
              <button
                @click="clearRetryTargetDecision"
                class="px-4 py-2 rounded-lg border border-slate-200 bg-white text-sm font-medium text-slate-600 hover:bg-slate-50 hover:border-slate-300 transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2"
              >
                {{ t('manualCopy.cancelConflictDecision') }}
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>

    <!-- Toast container is mounted globally in App.vue via M01 primitives. -->
  </div>
</template>

<style scoped>
.retry-confirm-fade-enter-active {
  transition: opacity 0.2s ease;
}
.retry-confirm-fade-leave-active {
  transition: opacity 0.15s ease;
}
.retry-confirm-fade-enter-from,
.retry-confirm-fade-leave-to {
  opacity: 0;
}
.retry-confirm-fade-enter-active > div,
.retry-confirm-fade-leave-active > div {
  transition: transform 0.2s ease, opacity 0.2s ease;
}
.retry-confirm-fade-enter-from > div,
.retry-confirm-fade-leave-to > div {
  opacity: 0;
  transform: scale(0.96);
}

/* Drop the scale/translate when the OS reports prefers-reduced-motion. */
@media (prefers-reduced-motion: reduce) {
  .retry-confirm-fade-enter-from > div,
  .retry-confirm-fade-leave-to > div {
    transform: none;
  }
}
</style>
