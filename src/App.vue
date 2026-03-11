<script setup lang="ts">
import Sidebar from '@/components/Sidebar.vue';
import { RouterView } from 'vue-router';
import { onMounted, onUnmounted } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { appStore, addLog, upsertTaskRecord, syncTaskRecordByLog } from '@/lib/store';
import { getConfig } from '@/lib/tauri';
import { startScheduler } from '@/lib/scheduler';

let unlistenLog: (() => void) | null = null;
let unlistenProgress: (() => void) | null = null;

onMounted(async () => {
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

    try {
        const cfg = await getConfig();
        if (cfg.max_log_lines > 0) appStore.maxLogLines = cfg.max_log_lines;
        if (cfg.launch_and_auto_scan && !appStore.isRunning) {
            await startScheduler();
        }
    } catch (e) {
        addLog(`Auto-start check failed: ${e}`, 'error');
    }
});

onUnmounted(() => {
    if (unlistenLog) unlistenLog();
    if (unlistenProgress) unlistenProgress();
});
</script>

<template>
  <div class="flex h-screen bg-slate-50 font-sans text-slate-900">
    <Sidebar />
    <main class="flex-1 overflow-auto">
      <router-view v-slot="{ Component }">
        <keep-alive include="MainConsole">
          <component :is="Component" />
        </keep-alive>
      </router-view>
    </main>
  </div>
</template>
