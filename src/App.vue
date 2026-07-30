<script setup lang="ts">
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isPermissionGranted, requestPermission } from '@tauri-apps/plugin-notification';
import { defineAsyncComponent, onMounted, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { RouterView, useRouter } from 'vue-router';

import { ensureUpdaterInitialized } from '@/composables/useUpdater';
import { configStore } from '@/lib/configStore';
import {
  DEVICE_SIMULATOR_EVENTS,
  describeSimulatorError,
  deviceSimulatorApi,
  isDeviceSimulatorRuntimeActive,
  type SimulatorStatus,
} from '@/lib/deviceSimulator';
import { startScheduler } from '@/lib/scheduler';
import { appStore, addLog, setToolRuntime, startLiveTicker, stopLiveTicker } from '@/lib/store';
import {
  createSyncTaskNotificationTracker,
  type SyncTaskNotificationEvent,
} from '@/lib/syncTaskNotifications';
import { taskStateStore } from '@/lib/taskStateStore';
import {
  cancelQuit,
  confirmQuit,
  fileShareGetStatus,
  fileShareStartSaved,
  getConfig,
  listTaskGroups,
  loadUiState,
  saveUiState,
  screenShareGetStatus,
  screenShareRespondControlRequest,
  showAppNotification,
  type FileShareStatus,
  type ScreenShareControlRequest,
  type ScreenShareStatus,
  type TaskGroupDetailSnapshot,
  type TaskGroupsSnapshot,
  type TaskLogEntry,
} from '@/lib/tauri';

// The application chrome belongs to the main window. Loading it lazily keeps it
// out of the entry chunk that borderless helper windows have to parse.
const Sidebar = defineAsyncComponent(() => import('@/components/Sidebar.vue'));
const AppTitleBar = defineAsyncComponent(() => import('@/components/AppTitleBar.vue'));
const ToastContainer = defineAsyncComponent(() => import('@/components/ToastContainer.vue'));
const UpdateDialog = defineAsyncComponent(() => import('@/components/UpdateDialog.vue'));
const ScreenShareControlRequestDialog = defineAsyncComponent(
  () => import('@/components/ScreenShareControlRequestDialog.vue'),
);
const QuitConfirmDialog = defineAsyncComponent(() => import('@/components/QuitConfirmDialog.vue'));

let unlistenLog: (() => void) | null = null;
let unlistenProgress: (() => void) | null = null;
let unlistenScanQueued: (() => void) | null = null;
let unlistenTaskGroups: (() => void) | null = null;
let unlistenTaskDetail: (() => void) | null = null;
let unlistenTaskLog: (() => void) | null = null;
let unlistenBeforeQuit: (() => void) | null = null;
let unlistenScreenShareStatus: (() => void) | null = null;
let unlistenScreenShareControlRequest: (() => void) | null = null;
let unlistenFileShareStatus: (() => void) | null = null;
let unlistenDeviceSimulatorStatus: (() => void) | null = null;
let unlistenOpenClipboardSettings: (() => void) | null = null;
let unlistenMainWindowResize: (() => void) | null = null;
let saveTimer: ReturnType<typeof setTimeout> | null = null;
let notificationPermissionPromise: Promise<boolean> | null = null;
let initialSyncTaskNotificationsEnabled = true;
let syncTaskNotificationTracker = createSyncTaskNotificationTracker();

const router = useRouter();
const { t } = useI18n();
const pendingScreenShareControlRequest = ref<ScreenShareControlRequest | null>(null);
const respondingToScreenShareControlRequest = ref(false);
const screenShareControlRequestError = ref('');
const quitConfirmOpen = ref(false);
const isMaximized = ref(false);
const quitConfirmTaskNames = ref<string[]>([]);
const quitConfirmSimulatorCleanup = ref(false);
const quitConfirmBusy = ref(false);
const quitConfirmError = ref('');
let quitFlowActive = false;

interface ScanQueuedEvent {
  folder: string;
  local_path: string;
  remote_path: string;
  task_group_id: string;
  run_id: string;
}

function syncTaskNotificationsEnabled(): boolean {
  return configStore.config?.sync_task_notifications_enabled
    ?? initialSyncTaskNotificationsEnabled;
}

async function ensureNotificationPermission(): Promise<boolean> {
  notificationPermissionPromise ??= (async () => {
    if (await isPermissionGranted()) return true;
    return (await requestPermission()) === 'granted';
  })();
  return notificationPermissionPromise;
}

async function showSyncTaskNotification(event: SyncTaskNotificationEvent): Promise<void> {
  if (!syncTaskNotificationsEnabled()) return;

  try {
    if (!await ensureNotificationPermission()) return;
    await showAppNotification(
      t(`sync.notifications.${event.kind}Title`),
      t(`sync.notifications.${event.kind}Body`, { task: event.taskName }),
    );
  } catch (error) {
    addLog(`System notification failed: ${error}`, 'error');
  }
}

function scheduleSave() {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    try {
      await saveUiState([...appStore.logs]);
    } catch {
      // silent
    }
  }, 3000);
}

function activeCopyTaskNames(groups: Awaited<ReturnType<typeof listTaskGroups>>): string[] {
  return groups
    .filter((group) => group.copy_status === 'running')
    .map((group) => group.display_name || group.folder_name);
}

async function revealMainWindowForPrompt() {
  try {
    const mainWindow = getCurrentWindow();
    await mainWindow.show();
    await mainWindow.unminimize();
    await mainWindow.setFocus();
  } catch {
    // The Rust tray handler also restores and foregrounds the main window.
  }
}

async function cancelQuitConfirmation() {
  if (quitConfirmBusy.value) return;
  quitConfirmOpen.value = false;
  quitConfirmTaskNames.value = [];
  quitConfirmSimulatorCleanup.value = false;
  quitConfirmError.value = '';
  quitFlowActive = false;
  try {
    await cancelQuit();
  } catch (error) {
    addLog(`Cancel quit failed: ${error}`, 'error');
  }
}

async function persistAndConfirmQuit() {
  if (quitConfirmBusy.value) return;
  quitConfirmBusy.value = true;
  quitConfirmError.value = '';

  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  try {
    await saveUiState([...appStore.logs]);
  } catch {
    // UI state persistence must not strand the process during exit.
  }

  try {
    await confirmQuit();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    addLog(`Simulator cleanup blocked exit: ${message}`, 'error');
    quitConfirmSimulatorCleanup.value = true;
    quitConfirmError.value = message;
    quitConfirmOpen.value = true;
    await revealMainWindowForPrompt();
  } finally {
    // On success the process exits. On failure the same dialog stays open and
    // becomes the retry/cleanup entry instead of delegating to window.alert.
    quitConfirmBusy.value = false;
    quitFlowActive = false;
  }
}

function applyScreenShareStatus(status: ScreenShareStatus) {
  setToolRuntime('screenShare', status.is_active);
  const request = status.control_state === 'requested'
    ? status.pending_control_request
    : null;
  if (request) {
    pendingScreenShareControlRequest.value = request;
  } else if (!respondingToScreenShareControlRequest.value) {
    pendingScreenShareControlRequest.value = null;
    screenShareControlRequestError.value = '';
  }
}

async function respondToScreenShareControlRequest(allow: boolean) {
  const request = pendingScreenShareControlRequest.value;
  if (!request || respondingToScreenShareControlRequest.value) return;
  respondingToScreenShareControlRequest.value = true;
  screenShareControlRequestError.value = '';
  try {
    await screenShareRespondControlRequest(request.request_id, allow);
    if (pendingScreenShareControlRequest.value?.request_id === request.request_id) {
      pendingScreenShareControlRequest.value = null;
    }
  } catch (error) {
    screenShareControlRequestError.value = t('tools.screenShare.errControlResponseFailed', {
      error: String(error),
    });
  } finally {
    respondingToScreenShareControlRequest.value = false;
  }
}

watch(() => appStore.logs.length, scheduleSave);

// One failing tool must not blank out the runtime flags of the others, so each
// query is settled on its own.
async function hydrateToolRuntime() {
  const [screenShareResult, fileShareResult, deviceSimulatorResult] = await Promise.allSettled([
    screenShareGetStatus(),
    fileShareGetStatus(),
    deviceSimulatorApi.getStatus(),
  ]);
  if (screenShareResult.status === 'fulfilled') {
    setToolRuntime('screenShare', screenShareResult.value.is_active);
  }
  if (fileShareResult.status === 'fulfilled') {
    setToolRuntime('fileShare', fileShareResult.value.is_active);
  }
  if (deviceSimulatorResult.status === 'fulfilled') {
    setToolRuntime(
      'deviceSimulator',
      isDeviceSimulatorRuntimeActive(deviceSimulatorResult.value.state),
    );
  }
  const failures: string[] = [];
  if (screenShareResult.status === 'rejected') {
    failures.push(`screenShare: ${describeSimulatorError(screenShareResult.reason)}`);
  }
  if (fileShareResult.status === 'rejected') {
    failures.push(`fileShare: ${describeSimulatorError(fileShareResult.reason)}`);
  }
  if (deviceSimulatorResult.status === 'rejected') {
    failures.push(`deviceSimulator: ${describeSimulatorError(deviceSimulatorResult.reason)}`);
  }
  if (failures.length > 0) {
    addLog(`Tool runtime status load failed: ${failures.join('; ')}`, 'error');
  }
}

onMounted(async () => {
  // Skip main-window init in auxiliary windows (e.g. clipboard-panel):
  // those windows only render their own page and must not duplicate event
  // listeners, scheduler auto-start, or before-quit handling.
  const label = getCurrentWindow().label;
  if (label !== 'main') return;

  const mainWindow = getCurrentWindow();
  const syncMaximizedState = async () => {
    isMaximized.value = await mainWindow.isMaximized();
  };
  await syncMaximizedState();
  unlistenMainWindowResize = await mainWindow.onResized(() => {
    void syncMaximizedState();
  });

  // Register these listeners before slower app hydration so a control request
  // can never depend on the screen-share page being mounted.
  unlistenScreenShareControlRequest = await listen<ScreenShareControlRequest>(
    'screen-share-control-request',
    (event) => {
      pendingScreenShareControlRequest.value = event.payload;
      screenShareControlRequestError.value = '';
    },
  );
  unlistenScreenShareStatus = await listen<ScreenShareStatus>('screen-share-status', (event) => {
    applyScreenShareStatus(event.payload);
  });
  try {
    applyScreenShareStatus(await screenShareGetStatus());
  } catch (error) {
    addLog(`Screen share status load failed: ${error}`, 'error');
  }

  try {
    await ensureUpdaterInitialized();
  } catch (error) {
    addLog(`Updater init failed: ${error}`, 'error');
  }

  startLiveTicker();
  let cfg = null;
  try {
    cfg = await getConfig();
    initialSyncTaskNotificationsEnabled = cfg.sync_task_notifications_enabled;
    if (cfg.max_log_lines > 0) appStore.maxLogLines = cfg.max_log_lines;
  } catch (e) {
    addLog(`Config load failed: ${e}`, 'error');
  }

  try {
    const persisted = await loadUiState();
    if (Array.isArray(persisted.logs) && persisted.logs.length > 0) {
      appStore.logs.push(...persisted.logs.slice(-appStore.maxLogLines) as typeof appStore.logs);
    }
  } catch {
    // silent
  }

  unlistenLog = await listen('log-message', (event: any) => {
    const payload = event.payload as { msg: string; level: string };
    let type: 'info' | 'error' | 'success' | 'command' = 'info';
    if (payload.level === 'error') type = 'error';
    if (payload.level === 'success') type = 'success';
    if (payload.level === 'command') type = 'command';
    addLog(payload.msg, type);
  });

  unlistenProgress = await listen('copy-progress', (event: any) => {
    const p = event.payload as {
      folder: string;
      total_bytes: number;
      copied_bytes: number;
      percentage: number;
      speed: number;
      eta_seconds: number;
      elapsed_seconds: number;
      local_path: string;
      remote_path: string;
      source?: string;
    };
    appStore.progress = {
      folder: p.folder,
      percentage: p.percentage,
      copied: p.copied_bytes,
      total: p.total_bytes,
      speed: p.speed,
      eta: p.eta_seconds,
      elapsed: p.elapsed_seconds || 0,
      localPath: p.local_path,
      remotePath: p.remote_path,
      source: p.source === 'manual' ? 'manual' : 'scheduled',
    };
    if (p.percentage >= 100) {
      setTimeout(() => {
        if (appStore.progress?.folder === p.folder) {
          appStore.progress = null;
        }
      }, 2000);
    }
  });

  await taskStateStore.hydrateTaskState();
  syncTaskNotificationTracker = createSyncTaskNotificationTracker(taskStateStore.groups);

  unlistenScanQueued = await listen<ScanQueuedEvent>('scan-queued', (event) => {
    syncTaskNotificationTracker.markQueued(event.payload.run_id);
    void showSyncTaskNotification({
      kind: 'queued',
      taskName: event.payload.folder,
    });
  });

  unlistenTaskGroups = await listen('task-groups-snapshot', (event) => {
    const snapshot = event.payload as TaskGroupsSnapshot;
    for (const notification of syncTaskNotificationTracker.collect(snapshot.groups)) {
      void showSyncTaskNotification(notification);
    }
    taskStateStore.applyGroupsSnapshot(snapshot);
  });

  unlistenTaskDetail = await listen('task-group-detail-snapshot', (event) => {
    taskStateStore.applyDetailSnapshot(event.payload as TaskGroupDetailSnapshot);
  });

  unlistenTaskLog = await listen('task-log', (event) => {
    taskStateStore.appendTaskLog(event.payload as TaskLogEntry);
  });

  unlistenBeforeQuit = await listen('before-quit', async () => {
    if (quitFlowActive || quitConfirmOpen.value || quitConfirmBusy.value) {
      await revealMainWindowForPrompt();
      return;
    }
    quitFlowActive = true;

    let runningCopyTasks = activeCopyTaskNames(taskStateStore.groups);
    let simulatorCleanupRequired = false;
    const [taskGroupsResult, simulatorStatusResult] = await Promise.allSettled([
      listTaskGroups(),
      deviceSimulatorApi.getStatus(),
    ]);

    if (taskGroupsResult.status === 'fulfilled') {
      // Refresh from Rust so a close request cannot race a task snapshot event.
      runningCopyTasks = activeCopyTaskNames(taskGroupsResult.value);
    }
    if (simulatorStatusResult.status === 'fulfilled') {
      simulatorCleanupRequired = isDeviceSimulatorRuntimeActive(simulatorStatusResult.value.state);
    } else {
      // An unreadable residual journal is itself a cleanup blocker. Surface an
      // actionable in-app confirmation; confirm_quit will retry the real cleanup.
      simulatorCleanupRequired = true;
      addLog(
        `Simulator exit status check failed: ${describeSimulatorError(simulatorStatusResult.reason)}`,
        'error',
      );
    }

    if (runningCopyTasks.length > 0 || simulatorCleanupRequired) {
      await revealMainWindowForPrompt();

      // Surface the task list behind the dialog so the runs being abandoned are
      // visible while the decision is made.
      if (runningCopyTasks.length > 0 && router.currentRoute.value.path !== '/sync') {
        try {
          await router.push('/sync');
        } catch {
          // Navigation is context, not a precondition for the confirmation.
        }
      }
      quitConfirmTaskNames.value = runningCopyTasks;
      quitConfirmSimulatorCleanup.value = simulatorCleanupRequired;
      quitConfirmError.value = '';
      quitConfirmOpen.value = true;
      quitFlowActive = false;
      return;
    }

    await persistAndConfirmQuit();
  });

  await hydrateToolRuntime();

  unlistenFileShareStatus = await listen<FileShareStatus>('file-share-status', (event) => {
    setToolRuntime('fileShare', event.payload.is_active);
  });

  unlistenDeviceSimulatorStatus = await listen<SimulatorStatus>(
    DEVICE_SIMULATOR_EVENTS.status,
    (event) => {
      setToolRuntime(
        'deviceSimulator',
        isDeviceSimulatorRuntimeActive(event.payload.state),
      );
    },
  );

  unlistenOpenClipboardSettings = await listen('clipboard-open-settings', () => {
    if (router.currentRoute.value.path !== '/tools/clipboard') {
      void router.push('/tools/clipboard');
    }
  });

  if (cfg?.launch_and_auto_scan && !appStore.isRunning) {
    try {
      await startScheduler();
    } catch (e) {
      addLog(`Auto-start check failed: ${e}`, 'error');
    }
  }

  if (cfg?.launch_and_auto_start_file_share) {
    try {
      await fileShareStartSaved();
    } catch (e) {
      addLog(`Auto file share start failed: ${e}`, 'error');
    }
  }
});

onUnmounted(() => {
  stopLiveTicker();
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  if (unlistenLog) unlistenLog();
  if (unlistenProgress) unlistenProgress();
  if (unlistenScanQueued) unlistenScanQueued();
  if (unlistenTaskGroups) unlistenTaskGroups();
  if (unlistenTaskDetail) unlistenTaskDetail();
  if (unlistenTaskLog) unlistenTaskLog();
  if (unlistenBeforeQuit) unlistenBeforeQuit();
  if (unlistenScreenShareStatus) unlistenScreenShareStatus();
  if (unlistenScreenShareControlRequest) unlistenScreenShareControlRequest();
  if (unlistenFileShareStatus) unlistenFileShareStatus();
  if (unlistenDeviceSimulatorStatus) unlistenDeviceSimulatorStatus();
  if (unlistenOpenClipboardSettings) unlistenOpenClipboardSettings();
  if (unlistenMainWindowResize) unlistenMainWindowResize();
});
</script>

<template>
  <router-view v-if="$route.meta?.noLayout" />
  <div
    v-else
    class="app-window-shell relative flex h-screen flex-col overflow-hidden bg-slate-50 font-sans text-slate-900"
    :class="{ 'app-window-shell--maximized': isMaximized }"
  >
    <a
      href="#main-content"
      class="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-[100] focus:bg-white focus:px-3 focus:py-1 focus:rounded-md focus:shadow-lg focus:text-slate-900 focus:outline focus:outline-2 focus:outline-sky-500"
    >
      {{ $t('common.skipToMain') }}
    </a>
    <AppTitleBar />
    <div class="flex min-h-0 flex-1">
      <Sidebar />
      <main
        id="main-content"
        role="main"
        class="flex flex-1 flex-col overflow-hidden"
      >
        <router-view v-slot="{ Component }">
        <!--
          Keep-alive list intentionally narrow:
          - SyncConsolePage: owns the nested sync tabs and keeps their forms,
            overview state, and log scroll position alive across main routes.
          - CodeStatisticsPage: large analysis results and form inputs are
            expensive to recompute; keep-alive preserves them across nav.
          - NetworkToolsPage: holds tab state (ping scan, port test, WOL,
            subnet calc, TCP table) — remounting would lose in-progress data.
          - SettingsPage: keeps application preferences stable across routes.
          Other pages either reload cheaply or rely on Tauri events that
          re-hydrate state on mount, so they intentionally remount.
        -->
        <keep-alive include="SyncConsolePage,CodeStatisticsPage,NetworkToolsPage,RemotePackagePatchPage,SettingsPage">
          <component :is="Component" />
        </keep-alive>
        </router-view>
      </main>
    </div>
    <UpdateDialog />
    <ToastContainer />
  </div>
  <ScreenShareControlRequestDialog
    v-if="pendingScreenShareControlRequest"
    :request="pendingScreenShareControlRequest"
    :busy="respondingToScreenShareControlRequest"
    :error="screenShareControlRequestError"
    @allow="respondToScreenShareControlRequest(true)"
    @deny="respondToScreenShareControlRequest(false)"
  />
  <QuitConfirmDialog
    v-if="quitConfirmOpen"
    :open="quitConfirmOpen"
    :task-names="quitConfirmTaskNames"
    :simulator-cleanup-required="quitConfirmSimulatorCleanup"
    :busy="quitConfirmBusy"
    :error="quitConfirmError"
    @confirm="persistAndConfirmQuit"
    @cancel="cancelQuitConfirmation"
  />
</template>
