<script setup lang="ts">
import Sidebar from '@/components/Sidebar.vue';
import { RouterView } from 'vue-router';
import { onMounted, onUnmounted } from 'vue';
import { listen } from '@tauri-apps/api/event';
import { appStore, addLog } from '@/lib/store';

let unlistenLog: (() => void) | null = null;
let unlistenProgress: (() => void) | null = null;

onMounted(async () => {
    unlistenLog = await listen('log-message', (event: any) => {
        const payload = event.payload as { msg: string, level: string };
        let type: 'info' | 'error' | 'success' = 'info';
        if (payload.level === 'error') type = 'error';
        if (payload.level === 'success') type = 'success';
        addLog(payload.msg, type);
    });

    unlistenProgress = await listen('copy-progress', (event: any) => {
        const p = event.payload as { folder: string, total_bytes: number, copied_bytes: number, percentage: number, speed: number, eta_seconds: number };
        appStore.progress = {
            folder: p.folder,
            percentage: p.percentage,
            copied: p.copied_bytes,
            total: p.total_bytes,
            speed: p.speed,
            eta: p.eta_seconds,
        };
        // Reset progress when done (100%)
        if (p.percentage >= 100) {
            setTimeout(() => {
                if (appStore.progress?.folder === p.folder) {
                    appStore.progress = null;
                }
            }, 2000);
        }
    });
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
