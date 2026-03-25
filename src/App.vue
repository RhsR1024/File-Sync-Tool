<script setup lang="ts">
import Sidebar from '@/components/Sidebar.vue';
import { RouterView } from 'vue-router';
import { onMounted, onUnmounted, watch } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { appStore, addLog, upsertTaskRecord, syncTaskRecordByLog, updateManualCopyTaskState, markStaleTasksInterrupted } from '@/lib/store';
import { getConfig, saveUiState, loadUiState, confirmQuit } from '@/lib/tauri';
import { startScheduler } from '@/lib/scheduler';

let unlistenLog: (() => void) | null = null;
let unlistenProgress: (() => void) | null = null;
let unlistenManualCopyState: (() => void) | null = null;
let unlistenBeforeQuit: (() => void) | null = null;
let saveTimer: ReturnType<typeof setTimeout> | null = null;

function scheduleSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
        try {
            await saveUiState([...appStore.logs], [...appStore.taskRecords]);
        } catch { /* silent */ }
    }, 3000);
}

// Watch for any changes that warrant a persistent save
watch(() => appStore.logs.length, scheduleSave);
watch(
    () => appStore.taskRecords.map(r => `${r.id}:${r.phase}:${r.copyPercentage}:${r.deployPercentage}`).join('|'),
    scheduleSave,
);

onMounted(async () => {
    // 1. Load config first to get limits
    let cfg = null;
    try {
        cfg = await getConfig();
        if (cfg.max_log_lines > 0) appStore.maxLogLines = cfg.max_log_lines;
        if (cfg.max_task_records > 0) appStore.maxTaskRecords = cfg.max_task_records;
    } catch (e) {
        addLog(`Config load failed: ${e}`, 'error');
    }

    // 2. Restore persisted logs and task records
    try {
        const persisted = await loadUiState();
        if (Array.isArray(persisted.logs) && persisted.logs.length > 0) {
            const capped = persisted.logs.slice(-appStore.maxLogLines);
            appStore.logs.push(...(capped as any[]));
        }
        if (Array.isArray(persisted.task_records) && persisted.task_records.length > 0) {
            const capped = persisted.task_records.slice(0, appStore.maxTaskRecords);
            appStore.taskRecords.push(...(capped as any[]));
        }
    } catch { /* silent – fresh start if file missing or corrupt */ }

    // 2.5 Mark any stale active tasks as interrupted (from previous session)
    markStaleTasksInterrupted();

    // 3. Set up event listeners (new events append after restored data)
    unlistenLog = await listen('log-message', (event: any) => {
        const payload = event.payload as { msg: string, level: string };
        let type: 'info' | 'error' | 'success' | 'command' = 'info';
        if (payload.level === 'error') type = 'error';
        if (payload.level === 'success') type = 'success';
        if (payload.level === 'command') type = 'command';
        addLog(payload.msg, type);
        syncTaskRecordByLog(payload.msg, payload.level);
    });

    unlistenProgress = await listen('copy-progress', (event: any) => {
        const p = event.payload as { folder: string, total_bytes: number, copied_bytes: number, percentage: number, speed: number, eta_seconds: number, elapsed_seconds: number, local_path: string, remote_path: string, source?: string };
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
        // Reset live progress when done (100%)
        if (p.percentage >= 100) {
            setTimeout(() => {
                if (appStore.progress?.folder === p.folder) {
                    appStore.progress = null;
                }
            }, 2000);
        }
        // Update persistent task records in console
        upsertTaskRecord({ ...p, source: p.source });
    });

    unlistenManualCopyState = await listen('manual-copy-task-state', (event: any) => {
        const payload = event.payload as {
            folder: string;
            source_path: string;
            local_path: string;
            state: 'started' | 'completed' | 'failed' | 'cancelled';
        };
        updateManualCopyTaskState({
            folder: payload.folder,
            sourcePath: payload.source_path,
            localPath: payload.local_path,
            state: payload.state,
        });
    });

    // Save state before app quits, then confirm the exit
    unlistenBeforeQuit = await listen('before-quit', async () => {
        if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; }
        try {
            await saveUiState([...appStore.logs], [...appStore.taskRecords]);
        } catch { /* silent */ }
        await confirmQuit();
    });

    // 4. Auto-start scheduler if configured
    if (cfg?.launch_and_auto_scan && !appStore.isRunning) {
        try {
            await startScheduler();
        } catch (e) {
            addLog(`Auto-start check failed: ${e}`, 'error');
        }
    }
});

onUnmounted(() => {
    if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; }
    if (unlistenLog) unlistenLog();
    if (unlistenProgress) unlistenProgress();
    if (unlistenManualCopyState) unlistenManualCopyState();
    if (unlistenBeforeQuit) unlistenBeforeQuit();
});
</script>

<template>
  <div class="flex h-screen bg-slate-50 font-sans text-slate-900">
    <Sidebar />
    <main class="flex-1 overflow-auto">
      <router-view v-slot="{ Component }">
        <keep-alive include="MainConsole,CodeStatisticsPage,NetworkToolsPage">
          <component :is="Component" />
        </keep-alive>
      </router-view>
    </main>
  </div>
</template>
