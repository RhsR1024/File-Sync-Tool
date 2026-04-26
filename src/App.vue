<script setup lang="ts">
import Sidebar from '@/components/Sidebar.vue';
import ToastContainer from '@/components/ToastContainer.vue';
import UpdateDialog from '@/components/UpdateDialog.vue';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { onMounted, onUnmounted, watch } from 'vue';
import { RouterView, useRouter } from 'vue-router';

import { ensureUpdaterInitialized } from '@/composables/useUpdater';
import { startScheduler } from '@/lib/scheduler';
import { appStore, addLog, setToolRuntime, startLiveTicker, stopLiveTicker } from '@/lib/store';
import { taskStateStore } from '@/lib/taskStateStore';
import {
  confirmQuit,
  fileShareGetStatus,
  fileShareStartSaved,
  getConfig,
  loadUiState,
  saveUiState,
  screenShareGetStatus,
  type FileShareStatus,
  type ScreenShareStatus,
  type TaskGroupDetailSnapshot,
  type TaskGroupsSnapshot,
  type TaskLogEntry,
} from '@/lib/tauri';

let unlistenLog: (() => void) | null = null;
let unlistenProgress: (() => void) | null = null;
let unlistenTaskGroups: (() => void) | null = null;
let unlistenTaskDetail: (() => void) | null = null;
let unlistenTaskLog: (() => void) | null = null;
let unlistenBeforeQuit: (() => void) | null = null;
let unlistenScreenShareStatus: (() => void) | null = null;
let unlistenFileShareStatus: (() => void) | null = null;
let unlistenOpenClipboardSettings: (() => void) | null = null;
let saveTimer: ReturnType<typeof setTimeout> | null = null;

const router = useRouter();

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

watch(() => appStore.logs.length, scheduleSave);

async function hydrateToolRuntime() {
  try {
    const [screenShareStatus, fileShareStatus] = await Promise.all([
      screenShareGetStatus(),
      fileShareGetStatus(),
    ]);
    setToolRuntime('screenShare', screenShareStatus.is_active);
    setToolRuntime('fileShare', fileShareStatus.is_active);
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

  try {
    await ensureUpdaterInitialized();
  } catch (error) {
    addLog(`Updater init failed: ${error}`, 'error');
  }

  startLiveTicker();
  let cfg = null;
  try {
    cfg = await getConfig();
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

  unlistenTaskGroups = await listen('task-groups-snapshot', (event) => {
    taskStateStore.applyGroupsSnapshot(event.payload as TaskGroupsSnapshot);
  });

  unlistenTaskDetail = await listen('task-group-detail-snapshot', (event) => {
    taskStateStore.applyDetailSnapshot(event.payload as TaskGroupDetailSnapshot);
  });

  unlistenTaskLog = await listen('task-log', (event) => {
    taskStateStore.appendTaskLog(event.payload as TaskLogEntry);
  });

  unlistenBeforeQuit = await listen('before-quit', async () => {
    if (saveTimer) {
      clearTimeout(saveTimer);
      saveTimer = null;
    }
    try {
      await saveUiState([...appStore.logs]);
    } catch {
      // silent
    }
    await confirmQuit();
  });

  await hydrateToolRuntime();

  unlistenScreenShareStatus = await listen<ScreenShareStatus>('screen-share-status', (event) => {
    setToolRuntime('screenShare', event.payload.is_active);
  });

  unlistenFileShareStatus = await listen<FileShareStatus>('file-share-status', (event) => {
    setToolRuntime('fileShare', event.payload.is_active);
  });

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
  if (unlistenTaskGroups) unlistenTaskGroups();
  if (unlistenTaskDetail) unlistenTaskDetail();
  if (unlistenTaskLog) unlistenTaskLog();
  if (unlistenBeforeQuit) unlistenBeforeQuit();
  if (unlistenScreenShareStatus) unlistenScreenShareStatus();
  if (unlistenFileShareStatus) unlistenFileShareStatus();
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
          - MainConsole: preserves the live log buffer / scroll position so
            users don't lose console state when bouncing between routes.
          - CodeStatisticsPage: large analysis results and form inputs are
            expensive to recompute; keep-alive preserves them across nav.
          - NetworkToolsPage: holds tab state (ping scan, port test, WOL,
            subnet calc, TCP table) — remounting would lose in-progress data.
          - SettingsPage: a 1900-line form with many nested mutable refs;
            keep-alive avoids the cost of reloading config + remounting fields.
          Other pages either reload cheaply or rely on Tauri events that
          re-hydrate state on mount, so they intentionally remount.
        -->
        <keep-alive include="MainConsole,CodeStatisticsPage,NetworkToolsPage,SettingsPage">
          <component :is="Component" />
        </keep-alive>
      </router-view>
    </main>
    <UpdateDialog />
    <ToastContainer />
  </div>
</template>
