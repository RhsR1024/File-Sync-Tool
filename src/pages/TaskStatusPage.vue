<script setup lang="ts">
import { ref, computed, onMounted, onActivated } from 'vue';
import { Play, Square, RefreshCw, Clock, Activity, Copy, AlertTriangle, FilePlus2, Trash2 } from 'lucide-vue-next';
import Empty from '@/components/Empty.vue';
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
import { startScheduler, stopScheduler, executeScan } from '@/lib/scheduler';

defineOptions({
  name: 'TaskStatusPage',
});

const { t } = useI18n();
const config = ref<AppConfig | null>(null);
const isManualCopyModalOpen = ref(false);

const toastMessage = ref('');
const toastTone = ref<'success' | 'error' | 'info'>('info');
let toastTimer: ReturnType<typeof setTimeout> | null = null;

// For retry run with target existence check
const retryTargetPreview = ref<ManualCopyPreview | null>(null);
const pendingRetryRequest = ref<{ taskGroupId: string; source: string; target: string } | null>(null);

function showToast(message: string, tone: 'success' | 'error' | 'info' = 'info') {
  toastMessage.value = message;
  toastTone.value = tone;
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toastMessage.value = '';
    toastTimer = null;
  }, 2400);
}

const rows = computed(() => buildTaskRows(taskStateStore.groups));

const hasAnyTerminal = computed(() => rows.value.some(r => {
  const s = r.summary_status;
  return s === 'completed' || s === 'failed' || s === 'cancelled'
    || s === 'interrupted' || s === 'partial_failed';
}));

async function handleSelect(taskGroupId: string) {
  await taskStateStore.selectTaskGroup(taskGroupId);
}

async function handleClearGroup(taskGroupId: string) {
  try {
    await clearTaskGroup(taskGroupId);
    showToast(t('console.clearGroup'), 'success');
  } catch (e) {
    addLog(`Clear failed: ${e}`, 'error');
    showToast(`${t('console.clearGroup')} - ${e}`, 'error');
  }
}

async function handleClearAll() {
  try {
    await clearTaskGroups();
    showToast(t('console.clearAllGroups'), 'success');
  } catch (e) {
    addLog(`Clear all failed: ${e}`, 'error');
    showToast(`${t('console.clearAllGroups')} - ${e}`, 'error');
  }
}

async function handlePause(taskGroupId: string, runId: string) {
  console.log('[TaskStatusPage] pause clicked', { taskGroupId, runId });
  try {
    await pauseTaskRun(taskGroupId, runId);
    addLog(t('console.paused'), 'info');
    showToast(t('console.paused'), 'success');
  } catch (e) {
    addLog(`Pause failed: ${e}`, 'error');
    showToast(`${t('console.pause')} - ${e}`, 'error');
  }
}

async function handleResume(taskGroupId: string, runId: string) {
  console.log('[TaskStatusPage] resume clicked', { taskGroupId, runId });
  try {
    await resumeTaskRun(taskGroupId, runId);
    addLog(t('console.resumed'), 'info');
    showToast(t('console.resumed'), 'success');
  } catch (e) {
    addLog(`Resume failed: ${e}`, 'error');
    showToast(`${t('console.resume')} - ${e}`, 'error');
  }
}

async function handleCancel(taskGroupId: string, runId: string) {
  console.log('[TaskStatusPage] cancel clicked', { taskGroupId, runId });
  try {
    await cancelTaskRun(taskGroupId, runId);
    addLog(t('console.cancelling'), 'info');
    showToast(t('console.cancelling'), 'info');
  } catch (e) {
    addLog(`Cancel failed: ${e}`, 'error');
    showToast(`${t('console.cancel')} - ${e}`, 'error');
  }
}

async function handleRetryDeploy(taskGroupId: string) {
  try {
    await retryTaskGroupDeploy(taskGroupId);
    addLog(t('console.retryDeploy'), 'info');
    showToast(t('console.retryDeploy'), 'success');
  } catch (e) {
    addLog(`Retry deploy failed: ${e}`, 'error');
    showToast(`${t('console.retryDeploy')} - ${e}`, 'error');
  }
}

async function handleRetryRun(taskGroupId: string) {
  // Find the task group from the rows
  const taskRow = rows.value.find(r => r.task_group_id === taskGroupId);
  if (!taskRow) {
    showToast('Task not found', 'error');
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
    showToast(`Failed to preview: ${e}`, 'error');
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
    showToast(
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
    showToast(`Failed to queue: ${e}`, 'error');
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
    await taskStateStore.hydrateTaskState();
  }
});
</script>

<template>
  <div class="p-6 h-full flex flex-col gap-6 bg-slate-50">
    <h2 class="text-2xl font-bold text-slate-800">{{ t('sidebar.tasks') }}</h2>

    <!-- Status cards -->
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

    <!-- Main area: scheduler controls + task groups table -->
    <div class="bg-white rounded-xl border border-slate-200 shadow-sm flex flex-col relative overflow-hidden flex-1">
      <!-- Scheduler controls bar -->
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

          <button
            v-if="hasAnyTerminal"
            @click="handleClearAll"
            class="px-4 py-2 rounded-lg font-bold bg-white text-slate-500 border border-slate-200 hover:bg-red-50 hover:border-red-200 hover:text-red-600 transition-all flex items-center gap-2 shadow-sm active:scale-95"
          >
            <Trash2 class="w-4 h-4" />
            {{ t('console.clearAllGroups') }}
          </button>
        </div>
      </div>

      <!-- Task groups table area -->
      <div class="flex-1 min-h-0 bg-slate-50 p-4 overflow-auto">
        <Empty
          v-if="!rows.length"
          :icon="Activity"
          :title="t('console.noTaskGroups')"
          class="h-full min-h-[320px]"
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
    </div>

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
      @close="isManualCopyModalOpen = false"
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
              <!-- Overwrite -->
              <button
                @click="retryConfirmQueue(pendingRetryRequest!.source, pendingRetryRequest!.target, true)"
                class="w-full flex items-start gap-3.5 rounded-xl border border-blue-200 bg-blue-50 px-4 py-3.5 text-left transition-all hover:border-blue-300 hover:bg-blue-100 hover:shadow-sm active:scale-[0.99]"
              >
                <div class="flex-shrink-0 w-8 h-8 rounded-lg bg-blue-600 flex items-center justify-center mt-0.5">
                  <RefreshCw class="w-3.5 h-3.5 text-white" />
                </div>
                <div class="min-w-0 flex-1">
                  <div class="text-sm font-semibold text-blue-800">
                    {{ t('manualCopy.overwriteAndQueue') }}
                  </div>
                  <div class="text-xs text-slate-600 mt-1 leading-5 break-words">
                    {{ retryOverwriteHint(retryTargetPreview) }}
                  </div>
                </div>
              </button>

              <!-- Skip -->
              <button
                @click="retryConfirmQueue(pendingRetryRequest!.source, pendingRetryRequest!.target, false)"
                class="w-full flex items-start gap-3.5 rounded-xl border border-emerald-200 bg-emerald-50 px-4 py-3.5 text-left transition-all hover:border-emerald-300 hover:bg-emerald-100 hover:shadow-sm active:scale-[0.99]"
              >
                <div class="flex-shrink-0 w-8 h-8 rounded-lg bg-emerald-600 flex items-center justify-center mt-0.5">
                  <FilePlus2 class="w-3.5 h-3.5 text-white" />
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
                class="px-4 py-2 rounded-lg border border-slate-200 bg-white text-sm font-medium text-slate-600 hover:bg-slate-50 hover:border-slate-300 transition-colors"
              >
                {{ t('manualCopy.cancelConflictDecision') }}
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>

    <!-- Action feedback toast -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition-all duration-200 ease-out"
        leave-active-class="transition-all duration-200 ease-in"
        enter-from-class="opacity-0 translate-y-2"
        leave-to-class="opacity-0 translate-y-2"
      >
        <div
          v-if="toastMessage"
          class="fixed bottom-6 right-6 z-[200] px-4 py-2.5 rounded-lg shadow-lg font-medium text-sm border flex items-center gap-2 max-w-md"
          :class="toastTone === 'success'
            ? 'bg-emerald-50 text-emerald-700 border-emerald-200'
            : toastTone === 'error'
              ? 'bg-rose-50 text-rose-700 border-rose-200'
              : 'bg-blue-50 text-blue-700 border-blue-200'"
        >
          <span class="w-2 h-2 rounded-full shrink-0"
            :class="toastTone === 'success' ? 'bg-emerald-500' : toastTone === 'error' ? 'bg-rose-500' : 'bg-blue-500'">
          </span>
          <span class="break-all">{{ toastMessage }}</span>
        </div>
      </Transition>
    </Teleport>
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
</style>
