<script setup lang="ts">
import { ref, onMounted, onUnmounted, onActivated } from 'vue';
import { Play, Square, RefreshCw, Clock, Activity, Pause, PlayCircle, XCircle } from 'lucide-vue-next';
import { getConfig, scanNow, cancelScan, pauseScan, resumeScan, addSystemEvent, type ScanResult, type AppConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { appStore, addLog } from '@/lib/store';

defineOptions({
  name: 'TaskStatusPage'
});

const { t } = useI18n();
const isRunning = ref(false);
const nextRunTime = ref<string>('-');
const config = ref<AppConfig | null>(null);
const isCancelling = ref(false);
const isPaused = ref(false);
let timer: ReturnType<typeof setInterval> | null = null;

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

async function handleScan() {
  addLog(t('console.running'), 'info'); 
  try {
    const result: ScanResult = await scanNow();
    addLog(t('console.scanComplete', { scanned: result.scanned_paths, found: result.found_folders.length, copied: result.copied_folders.length }), 'success');
    
    if (result.found_folders.length > 0) {
      result.found_folders.forEach(f => addLog(`Found candidate: ${f}`, 'info'));
    }
    if (result.copied_folders.length > 0) {
      result.copied_folders.forEach(f => addLog(`Successfully copied: ${f}`, 'success'));
    }
    if (result.errors.length > 0) {
      result.errors.forEach(e => addLog(`Error: ${e}`, 'error'));
    }
  } catch (e) {
    addLog(t('console.scanFailed', { error: e }), 'error');
  } finally {
    appStore.progress = null; // Ensure progress is cleared when scan finishes
    isCancelling.value = false; // Reset cancel state
    isPaused.value = false;
  }
}

function startScheduler(isRestart = false) {
  if (!config.value) return;
  isRunning.value = true;
  if (!isRestart) {
      const msg = t('console.schedulerStarted', { interval: config.value.interval_minutes });
      addLog(msg, 'info');
      addSystemEvent('SCHEDULER_START', msg);
  }
  
  if (!isRestart) {
      handleScan();
  }
  
  const intervalMs = config.value.interval_minutes * 60 * 1000;
  updateNextRunTime(intervalMs);
  
  timer = setInterval(() => {
    handleScan();
    updateNextRunTime(intervalMs);
  }, intervalMs);
}

function stopScheduler() {
  isRunning.value = false;
  if (timer) {
    clearInterval(timer);
    timer = null;
  }
  nextRunTime.value = '-';
  const msg = t('console.schedulerStopped');
  addLog(msg, 'info');
  addSystemEvent('SCHEDULER_STOP', msg);
}

function updateNextRunTime(delayMs: number) {
  const next = new Date(Date.now() + delayMs);
  nextRunTime.value = next.toLocaleTimeString();
}

onActivated(() => {
  loadConfig();
  if (isRunning.value && config.value) {
       if (timer) clearInterval(timer);
       startScheduler(true); 
   }
});

onMounted(() => {
  loadConfig();
});

onUnmounted(() => {
  if (timer) clearInterval(timer);
});
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
        <div class="flex items-center gap-3 font-bold text-2xl" :class="isRunning ? 'text-emerald-600' : 'text-slate-700'">
          <div class="relative flex h-3 w-3">
             <span v-if="isRunning" class="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
             <span class="relative inline-flex rounded-full h-3 w-3" :class="isRunning ? 'bg-emerald-500' : 'bg-slate-400'"></span>
          </div>
          {{ isRunning ? t('console.running') : t('console.stopped') }}
        </div>
      </div>
      
      <!-- Next Run Card -->
      <div class="bg-white p-5 rounded-xl border border-slate-200 shadow-sm relative overflow-hidden group hover:shadow-md transition-shadow">
        <div class="absolute top-0 right-0 p-4 opacity-10 group-hover:opacity-20 transition-opacity">
          <Clock class="w-16 h-16 text-blue-600" />
        </div>
        <div class="text-slate-500 text-sm font-medium uppercase tracking-wider mb-2">{{ t('console.nextRun') }}</div>
        <div class="flex items-center gap-2 font-bold text-2xl text-slate-800 font-mono">
          {{ nextRunTime }}
        </div>
      </div>
    </div>
    
    <!-- Controls & Task List -->
    <div class="bg-white rounded-xl border border-slate-200 shadow-sm flex flex-col relative overflow-hidden flex-1">
        <div class="p-4 flex gap-3 border-b border-slate-100 items-center justify-between">
            <h3 class="text-lg font-semibold text-slate-700">Scheduler Controls</h3>
            <div class="flex gap-3">
                <button 
                    @click="isRunning ? stopScheduler() : startScheduler()"
                    class="px-6 py-2 rounded-lg font-bold transition-all flex items-center justify-center gap-2 shadow-sm active:scale-95"
                    :class="isRunning 
                    ? 'bg-red-50 text-red-600 hover:bg-red-100 border border-red-200' 
                    : 'bg-emerald-600 text-white hover:bg-emerald-700 shadow-emerald-200'"
                >
                    <component :is="isRunning ? Square : Play" class="w-4 h-4 fill-current" />
                    {{ isRunning ? t('console.stop') : t('console.start') }}
                </button>
                
                <button 
                    @click="handleScan"
                    class="px-4 py-2 rounded-lg font-bold bg-white text-blue-600 border border-blue-200 hover:bg-blue-50 hover:border-blue-300 transition-all flex items-center gap-2 shadow-sm active:scale-95"
                    :disabled="isRunning"
                    :class="{ 'opacity-50 cursor-not-allowed': isRunning }"
                >
                    <RefreshCw class="w-4 h-4" :class="{ 'animate-spin': isRunning }" />
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
                <div class="grid grid-cols-[2fr_1fr_2fr_1.5fr_2fr_0.5fr_2fr_1.5fr_1.5fr_1fr] gap-4 p-3 bg-slate-100 text-slate-600 font-bold border-b border-slate-200 text-sm">
                    <div>{{ t('console.name') }}</div>
                    <div>{{ t('console.status') }}</div>
                    <div>{{ t('console.progress') }}</div>
                    <div>{{ t('console.size') }}</div>
                    <div>{{ t('console.localPath') }}</div>
                    <div class="text-center">&lt;-&gt;</div>
                    <div>{{ t('console.remotePath') }}</div>
                    <div>{{ t('console.speed') }}</div>
                    <div>{{ t('console.eta') }}</div>
                    <div>{{ t('console.elapsed') }}</div>
                </div>

                <!-- Table Row -->
                <div class="grid grid-cols-[2fr_1fr_2fr_1.5fr_2fr_0.5fr_2fr_1.5fr_1.5fr_1fr] gap-4 p-4 bg-white items-center text-sm">
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

                    <!-- Arrow -->
                    <div class="text-center flex justify-center">
                        <div class="w-6 h-6 rounded-full bg-slate-100 flex items-center justify-center text-red-500 font-bold">↑</div>
                    </div>

                    <!-- Remote Path -->
                    <div class="truncate text-slate-500 text-xs" :title="appStore.progress.remotePath || '-'">
                        {{ appStore.progress.remotePath || '-' }}
                    </div>

                    <!-- Speed -->
                    <div class="truncate font-mono font-medium text-blue-600">
                        {{ formatSpeed(appStore.progress.speed) }}
                    </div>

                    <!-- ETA -->
                    <div class="truncate font-mono text-slate-600">
                        {{ formatDuration(appStore.progress.eta) }}
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
