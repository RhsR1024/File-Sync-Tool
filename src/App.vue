<script setup lang="ts">
import Sidebar from '@/components/Sidebar.vue';
import ScreenShareControlRequestDialog from '@/components/ScreenShareControlRequestDialog.vue';
import ToastContainer from '@/components/ToastContainer.vue';
import UpdateDialog from '@/components/UpdateDialog.vue';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isPermissionGranted, requestPermission } from '@tauri-apps/plugin-notification';
import { onMounted, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { RouterView, useRouter } from 'vue-router';

import { ensureUpdaterInitialized } from '@/composables/useUpdater';
import { configStore } from '@/lib/configStore';
import {
  DEVICE_SIMULATOR_EVENTS,
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
let saveTimer: ReturnType<typeof setTimeout> | null = null;
let notificationPermissionPromise: Promise<boolean> | null = null;
let initialSyncTaskNotificationsEnabled = true;
let syncTaskNotificationTracker = createSyncTaskNotificationTracker();

const router = useRouter();
const { t } = useI18n();
const pendingScreenShareControlRequest = ref<ScreenShareControlRequest | null>(null);
const respondingToScreenShareControlRequest = ref(false);
const screenShareControlRequestError = ref('');

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

function hasActiveCopyTask(groups: Awaited<ReturnType<typeof listTaskGroups>>): boolean {
  return groups.some((group) => group.copy_status === 'running');
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

async function hydrateToolRuntime() {
  try {
    const [screenShareStatus, fileShareStatus, deviceSimulatorStatus] = await Promise.all([
      screenShareGetStatus(),
      fileShareGetStatus(),
      deviceSimulatorApi.getStatus(),
    ]);
    setToolRuntime('screenShare', screenShareStatus.is_active);
    setToolRuntime('fileShare', fileShareStatus.is_active);
    setToolRuntime(
      'deviceSimulator',
      isDeviceSimulatorRuntimeActive(deviceSimulatorStatus.state),
    );
  } catch (error) {
    addLog(`Tool runtime status load failed: ${error}`, 'error');
  }
}

onMounted(async () => {
  // Skip main-window init in auxiliary windows (e.g. clipboard-panel):
  // those windows only render their own page and must not duplicate event
  // listeners, scheduler auto-start, or before-quit handling.
  const label = getCurrentWindow().label;
  if (label !== 'main') return;

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
    let activeCopyTask = hasActiveCopyTask(taskStateStore.groups);
    try {
      // Refresh from Rust so a close request cannot race a task snapshot event.
      activeCopyTask = hasActiveCopyTask(await listTaskGroups());
    } catch {
      // Keep the last event-backed state if the refresh fails during shutdown.
    }

    if (activeCopyTask) {
      // A tray exit can arrive while the main window is hidden. Restore it so
      // the browser confirmation is visible and can be answered.
      try {
        const mainWindow = getCurrentWindow();
        await mainWindow.show();
        await mainWindow.setFocus();
      } catch {
        // The confirmation still falls back to the current window state.
      }

      if (!window.confirm(t('common.quitWhileCopyingConfirm'))) {
        try {
          await cancelQuit();
        } catch (error) {
          addLog(`Cancel quit failed: ${error}`, 'error');
        }
        return;
      }
    }

    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = null;
    }
    try {
      await saveUiState([...appStore.logs]);
    } catch {
      // silent
    }
    try {
      await confirmQuit();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      addLog(`Simulator cleanup blocked exit: ${message}`, 'error');
      window.alert(t('deviceSimulator.exit.blocked', { error: message }));
    }
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
});
</script>

<template>
  <router-view v-if="$route.meta?.noLayout" />
  <div v-else class="flex h-screen bg-slate-50 font-sans text-slate-900">
    <a
      href="#main-content"
      class="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-[100] focus:bg-white focus:px-3 focus:py-1 focus:rounded-md focus:shadow-lg focus:text-slate-900 focus:outline focus:outline-2 focus:outline-sky-500"
    >
      {{ $t('common.skipToMain') }}
    </a>
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
    <UpdateDialog />
    <ToastContainer />
  </div>
  <ScreenShareControlRequestDialog
    :request="pendingScreenShareControlRequest"
    :busy="respondingToScreenShareControlRequest"
    :error="screenShareControlRequestError"
    @allow="respondToScreenShareControlRequest(true)"
    @deny="respondToScreenShareControlRequest(false)"
  />
</template>
