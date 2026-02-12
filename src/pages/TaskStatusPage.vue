<script setup lang="ts">
import { ref, onMounted, onActivated } from 'vue';
import { Play, Square, RefreshCw, Clock, Activity, Pause, PlayCircle, XCircle } from 'lucide-vue-next';
import { getConfig, cancelScan, pauseScan, resumeScan, addSystemEvent, type AppConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { appStore, addLog } from '@/lib/store';
import { startScheduler, stopScheduler, executeScan } from '@/lib/scheduler';

defineOptions({
  name: 'TaskStatusPage'
});

const { t } = useI18n();
// Removed local state: isRunning, nextRunTime, timer
const config = ref<AppConfig | null>(null);
const isCancelling = ref(false);
const isPaused = ref(false);

async function handleCancel() {
  if (isCancelling.value) return;
  isCancelling.value = true;
  
  const targetName = appStore.progress?.folder || '';
  const msg = `${t('console.cancelling')} ${targetName ? '(' + targetName + ')' : ''}`;
  addLog(msg, 'info');
  
  try {
    await cancelScan();
  } catch (e) {
    addLog(`Cancel failed: ${e}`, 'error');
    isCancelling.value = false;
  }
}

async function togglePause() {
  if (!appStore.progress) return;
  
  const targetName = appStore.progress.folder || '';
  
  if (isPaused.value) {
    await resumeScan();
    isPaused.value = false;
    const msg = `${t('console.resumed')} ${targetName ? '(' + targetName + ')' : ''}`;
    addLog(msg, 'info');
    await addSystemEvent('RESUME', msg);
  } else {
    await pauseScan();
    isPaused.value = true;
    const msg = `${t('console.paused')} ${targetName ? '(' + targetName + ')' : ''}`;
    addLog(msg, 'info');
    await addSystemEvent('PAUSE', msg);
  }
}

function formatSpeed(bytesPerSec: number) {
  if (bytesPerSec === 0) return '0 B/s';
  const k = 1024;
  const sizes = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  const i = Math.floor(Math.log(bytesPerSec) / Math.log(k));
  return parseFloat((bytesPerSec / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

function formatDuration(seconds: number) {
  if (seconds === 0) return '-';
  if (!isFinite(seconds)) return 'Calculaing...';
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return `${m}m ${s}s`;
}

async function loadConfig() {
  try {
    const newConfig = await getConfig();
    config.value = newConfig;
  } catch (e) {
    addLog(t('console.failedLoadConfig', { error: e }), 'error');
  }
}

// Replaced local handleScan with scheduler's executeScan
async function handleScanClick() {
    // Only allow manual scan if scheduler is not running to avoid conflicts, 
    // or if we want to allow it, we should ensure scheduler logic handles concurrent calls.
    // The previous logic disabled the button if isRunning.
    if (appStore.isRunning) return;
    await executeScan();
}

onActivated(() => {
  loadConfig();
});

onMounted(() => {
  loadConfig();
});

// Removed onUnmounted cleanup
</script>

<template>
  <div class="p-6 h-full flex flex-col gap-6 bg-slate-50">
    <h2 class="text-2xl font-bold text-slate-800">{{ t('sidebar.tasks') }}</h2>
    
    <!-- Status Cards Row -->
    <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
      <!-- Status Card -->
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
      
      <!-- Next Run Card -->
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
    
    <!-- Controls & Task List -->
    <div class="bg-white rounded-xl border border-slate-200 shadow-sm flex flex-col relative overflow-hidden flex-1">
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
            </div>
        </div>
        
        <!-- Active Tasks (Progress Table) -->
        <div class="flex-1 bg-slate-50 p-4">
             <div v-if="!appStore.progress" class="h-full flex flex-col items-center justify-center text-slate-400 border-2 border-dashed border-slate-200 rounded-lg">
                 <Activity class="w-12 h-12 mb-2 opacity-20" />
                 <span>No active tasks running</span>
             </div>

            <div v-else class="bg-white border border-slate-200 rounded-lg overflow-hidden shadow-sm">
                <!-- Table Header -->
                <div class="grid grid-cols-[2fr_1fr_2fr_1.5fr_2fr_2fr_1.5fr_1.5fr_1fr] gap-4 p-3 bg-slate-100 text-slate-600 font-bold border-b border-slate-200 text-sm">
                    <div>{{ t('console.name') }}</div>
                    <div>{{ t('console.status') }}</div>
                    <div>{{ t('console.progress') }}</div>
                    <div>{{ t('console.size') }}</div>
                    <div>{{ t('console.localPath') }}</div>
                    <div>{{ t('console.remotePath') }}</div>
                    <div>{{ t('console.speed') }}</div>
                    <div>{{ t('console.eta') }}</div>
                    <div>{{ t('console.elapsed') }}</div>
                </div>

                <!-- Table Row -->
                <div class="grid grid-cols-[2fr_1fr_2fr_1.5fr_2fr_2fr_1.5fr_1.5fr_1fr] gap-4 p-4 bg-white items-center text-sm">
                    <!-- Name -->
                    <div class="flex items-center gap-2 truncate font-medium text-slate-800" :title="appStore.progress.folder">
                        <div class="w-8 h-8 bg-blue-100 text-blue-600 rounded flex items-center justify-center shrink-0">
                            <Activity class="w-4 h-4" />
                        </div>
                        <span class="truncate">{{ appStore.progress.folder }}</span>
                    </div>
                    
                    <!-- Status -->
                    <div class="font-bold" :class="isPaused ? 'text-amber-600' : 'text-emerald-600'">
                        {{ isPaused ? t('console.paused') : t('console.running') }}
                    </div>

                    <!-- Progress Bar -->
                    <div class="relative h-6 bg-slate-100 rounded-full overflow-hidden border border-slate-200">
                        <div class="absolute inset-0 bg-blue-500 transition-all duration-300" :style="{ width: `${appStore.progress.percentage}%` }"></div>
                        <div class="absolute inset-0 flex items-center justify-center text-xs text-white font-bold drop-shadow-md z-10">
                            {{ appStore.progress.percentage.toFixed(1) }}%
                        </div>
                    </div>

                    <!-- Size -->
                    <div class="truncate font-mono text-slate-600" :title="`${(appStore.progress.copied / 1024 / 1024).toFixed(2)}MB / ${(appStore.progress.total / 1024 / 1024).toFixed(2)}MB`">
                        {{ (appStore.progress.copied / 1024 / 1024).toFixed(2) }}MB / {{ (appStore.progress.total / 1024 / 1024).toFixed(2) }}MB
                    </div>

                    <!-- Local Path -->
                    <div class="truncate text-slate-500 text-xs" :title="appStore.progress.localPath || '-'">
                        {{ appStore.progress.localPath || '-' }}
                    </div>

                    <!-- Remote Path -->
                    <div class="truncate text-slate-500 text-xs" :title="appStore.progress.remotePath || '-'">
                        {{ appStore.progress.remotePath || '-' }}
                    </div>

                    <!-- Speed -->
                    <div class="truncate font-mono font-medium" :class="isPaused ? 'text-slate-400' : 'text-blue-600'">
                        {{ isPaused ? '-' : formatSpeed(appStore.progress.speed) }}
                    </div>

                    <!-- ETA -->
                    <div class="truncate font-mono text-slate-600">
                        {{ isPaused ? '-' : formatDuration(appStore.progress.eta) }}
                    </div>

                    <!-- Elapsed -->
                    <div class="truncate font-mono text-slate-600">
                        {{ formatDuration(appStore.progress.elapsed) }}
                    </div>
                </div>
                
                <!-- Actions Footer -->
                <div class="p-3 bg-slate-50 border-t border-slate-100 flex justify-end gap-3">
                    <button 
                        @click="togglePause"
                        class="px-4 py-2 rounded-lg font-medium border flex items-center gap-2 transition-colors shadow-sm active:scale-95"
                        :class="isPaused ? 'bg-emerald-50 text-emerald-700 border-emerald-200 hover:bg-emerald-100' : 'bg-amber-50 text-amber-700 border-amber-200 hover:bg-amber-100'"
                    >
                        <component :is="isPaused ? PlayCircle : Pause" class="w-4 h-4" />
                        {{ isPaused ? t('console.resume') : t('console.pause') }}
                    </button>

                    <button 
                        @click="handleCancel"
                        class="px-4 py-2 rounded-lg font-medium bg-white text-red-600 border border-red-200 hover:bg-red-50 hover:border-red-300 transition-colors shadow-sm active:scale-95 flex items-center gap-2"
                        :disabled="isCancelling"
                    >
                        <XCircle class="w-4 h-4" />
                        {{ isCancelling ? t('console.cancelling') : t('console.cancel') }}
                    </button>
                </div>
            </div>
        </div>
    </div>
  </div>
</template>
