<script setup lang="ts">
import { ref, computed, onMounted, onActivated } from 'vue';
import { Play, Square, RefreshCw, Clock, Activity, Copy } from 'lucide-vue-next';
import Empty from '@/components/Empty.vue';
import ManualCopyModal from '@/components/ManualCopyModal.vue';
import TaskGroupsTable from '@/components/tasks/TaskGroupsTable.vue';
import TaskGroupDetailPanel from '@/components/tasks/TaskGroupDetailPanel.vue';
import { getConfig, type AppConfig } from '@/lib/tauri';
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
          @clear-all="handleClearAll"
          @pause-run="handlePause"
          @resume-run="handleResume"
          @cancel-run="handleCancel"
          @retry-deploy="handleRetryDeploy"
        />
      </div>
    </div>

    <!-- Detail panel (slide-in) -->
    <TaskGroupDetailPanel
      :group="taskStateStore.selectedGroupDetail"
      :task-logs="taskStateStore.taskLogs"
      :is-loading="taskStateStore.isLoadingDetail"
      @retry-deploy="handleRetryDeploy"
      @pause-run="handlePause"
      @resume-run="handleResume"
      @cancel-run="handleCancel"
      @close="handleCloseDetail"
    />

    <!-- Manual Copy Modal -->
    <ManualCopyModal
      :is-open="isManualCopyModalOpen"
      @close="isManualCopyModalOpen = false"
      @success="() => {}"
    />

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
