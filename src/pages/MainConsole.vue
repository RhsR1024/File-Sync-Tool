<script setup lang="ts">
import { ref, onMounted, onUnmounted, onActivated } from 'vue';
import { Play, Square, RefreshCw, Trash2, Clock, Activity, AlertCircle, CheckCircle2, XCircle, Pause, PlayCircle } from 'lucide-vue-next';
import { getConfig, scanNow, cancelScan, pauseScan, resumeScan, addSystemEvent, type ScanResult, type AppConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { appStore, addLog } from '@/lib/store';

// Add name for KeepAlive
defineOptions({
  name: 'MainConsole'
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

async function loadConfig(silentIfSame = false) {
  try {
    const newConfig = await getConfig();
    const isChanged = JSON.stringify(config.value) !== JSON.stringify(newConfig);

    if (isChanged) {
        config.value = newConfig;
        addLog(t('console.configLoaded'), 'info');
    } else {
        if (!silentIfSame) {
             addLog(t('console.configLoaded'), 'info');
        }
    }
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

function clearLogs() {
  appStore.logs.splice(0, appStore.logs.length);
}

// Reload config when page is activated (switched back to)
onActivated(() => {
  loadConfig(true); 
  
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
    <!-- Status Cards -->
    <div class="grid grid-cols-1 md:grid-cols-3 gap-6">
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
      
      <!-- Controls -->
      <div class="bg-white rounded-xl border border-slate-200 shadow-sm flex flex-col relative overflow-hidden">
        <!-- Header / Main Controls -->
        <div class="p-4 flex gap-3 relative z-10 border-b border-slate-100">
          <button 
            @click="isRunning ? stopScheduler() : startScheduler()"
            class="flex-1 px-4 py-3 rounded-lg font-bold transition-all flex items-center justify-center gap-2 shadow-sm active:scale-95"
            :class="isRunning 
              ? 'bg-red-50 text-red-600 hover:bg-red-100 border border-red-200' 
              : 'bg-emerald-600 text-white hover:bg-emerald-700 shadow-emerald-200'"
          >
            <component :is="isRunning ? Square : Play" class="w-4 h-4 fill-current" />
            {{ isRunning ? t('console.stop') : t('console.start') }}
          </button>
          
          <button 
            @click="handleScan"
            class="px-4 py-3 rounded-lg font-bold bg-white text-blue-600 border border-blue-200 hover:bg-blue-50 hover:border-blue-300 transition-all flex items-center gap-2 shadow-sm active:scale-95"
            :disabled="isRunning"
            :class="{ 'opacity-50 cursor-not-allowed': isRunning }"
          >
            <RefreshCw class="w-4 h-4" :class="{ 'animate-spin': isRunning }" />
          </button>
        </div>

        <!-- Detailed Progress Table Style View -->
        <div v-if="appStore.progress" class="bg-slate-50 border-t border-slate-100 text-xs font-mono">
           <!-- Table Header -->
           <div class="grid grid-cols-[2fr_1fr_2fr_1.5fr_2fr_0.5fr_2fr_1.5fr_1.5fr_1fr] gap-2 p-2 bg-slate-100 text-slate-600 font-bold border-b border-slate-200 items-center">
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
           <div class="grid grid-cols-[2fr_1fr_2fr_1.5fr_2fr_0.5fr_2fr_1.5fr_1.5fr_1fr] gap-2 p-3 bg-white items-center border-b border-slate-200 relative group hover:bg-slate-50 transition-colors">
               <!-- Name -->
               <div class="flex items-center gap-2 truncate" :title="appStore.progress.folder">
                   <div class="w-4 h-4 bg-slate-200 rounded shrink-0"></div>
                   <span class="truncate">{{ appStore.progress.folder }}</span>
               </div>
               
               <!-- Status -->
               <div :class="isPaused ? 'text-amber-600' : 'text-emerald-600'">
                   {{ isPaused ? t('console.paused') : t('console.running') }}
               </div>

               <!-- Progress Bar -->
               <div class="relative h-4 bg-slate-200 rounded overflow-hidden">
                   <div class="absolute inset-0 bg-blue-500 transition-all duration-300" :style="{ width: `${appStore.progress.percentage}%` }"></div>
                   <div class="absolute inset-0 flex items-center justify-center text-[10px] text-white font-bold drop-shadow-md">
                       {{ appStore.progress.percentage.toFixed(0) }}%
                   </div>
                   <!-- Left Blue Indicator -->
                   <div class="absolute left-0 top-0 bottom-0 w-1 bg-blue-600"></div>
               </div>

               <!-- Size -->
               <div class="truncate" :title="`${(appStore.progress.copied / 1024 / 1024).toFixed(2)}MB / ${(appStore.progress.total / 1024 / 1024).toFixed(2)}MB`">
                   {{ (appStore.progress.copied / 1024 / 1024).toFixed(2) }}MB / {{ (appStore.progress.total / 1024 / 1024).toFixed(2) }}MB
               </div>

               <!-- Local Path -->
               <div class="truncate text-slate-500" :title="appStore.progress.localPath || '-'">
                   {{ appStore.progress.localPath || '-' }}
               </div>

               <!-- Arrow -->
               <div class="text-center flex justify-center">
                   <div class="w-4 h-4 text-red-500 font-bold">↑</div>
               </div>

               <!-- Remote Path -->
               <div class="truncate text-slate-500" :title="appStore.progress.remotePath || '-'">
                   {{ appStore.progress.remotePath || '-' }}
               </div>

               <!-- Speed -->
               <div class="truncate">
                   {{ formatSpeed(appStore.progress.speed) }}
               </div>

               <!-- ETA -->
               <div class="truncate">
                   {{ formatDuration(appStore.progress.eta) }}
               </div>

               <!-- Elapsed -->
               <div class="truncate">
                   {{ formatDuration(appStore.progress.elapsed) }}
               </div>
           </div>
           
           <!-- Actions Row -->
           <div class="p-2 flex justify-end gap-2 bg-white border-t border-slate-100">
               <button 
                 @click="togglePause"
                 class="text-xs px-3 py-1.5 rounded border flex items-center gap-1 transition-colors"
                 :class="isPaused ? 'text-emerald-600 border-emerald-200 hover:bg-emerald-50' : 'text-amber-600 border-amber-200 hover:bg-amber-50'"
               >
                 <component :is="isPaused ? PlayCircle : Pause" class="w-3 h-3" />
                 {{ isPaused ? t('console.resume') : t('console.pause') }}
               </button>

               <button 
                 @click="handleCancel"
                 class="text-xs text-red-500 hover:text-red-700 hover:bg-red-50 px-3 py-1.5 rounded border border-red-200 flex items-center gap-1 transition-colors"
                 :disabled="isCancelling"
               >
                 <XCircle class="w-3 h-3" />
                 {{ isCancelling ? t('console.cancelling') : t('console.cancel') }}
               </button>
           </div>
        </div>
      </div>
    </div>

    <!-- Logs -->
    <div class="flex-1 bg-[#0f172a] rounded-xl overflow-hidden flex flex-col shadow-xl border border-slate-800">
      <div class="p-3 border-b border-slate-800 flex justify-between items-center bg-slate-900/80 backdrop-blur">
        <div class="flex items-center gap-2">
           <div class="flex gap-1.5 ml-2">
             <div class="w-2.5 h-2.5 rounded-full bg-red-500/20 border border-red-500/50"></div>
             <div class="w-2.5 h-2.5 rounded-full bg-yellow-500/20 border border-yellow-500/50"></div>
             <div class="w-2.5 h-2.5 rounded-full bg-green-500/20 border border-green-500/50"></div>
           </div>
           <h3 class="ml-3 text-slate-400 font-mono text-xs uppercase tracking-widest">{{ t('console.logs') }}</h3>
        </div>
        <button @click="clearLogs" class="text-slate-500 hover:text-white p-1.5 rounded-md hover:bg-slate-800 transition-colors group" title="Clear logs">
          <Trash2 class="w-4 h-4 group-hover:text-red-400 transition-colors" />
        </button>
      </div>
      <div class="flex-1 overflow-auto p-4 font-mono text-xs md:text-sm space-y-1.5 custom-scrollbar">
        <div v-if="appStore.logs.length === 0" class="h-full flex flex-col items-center justify-center text-slate-700">
           <Activity class="w-12 h-12 mb-2 opacity-20" />
           <span class="italic">{{ t('console.noLogs') }}</span>
        </div>
        <div v-for="(log, i) in appStore.logs" :key="i" class="flex gap-3 hover:bg-white/5 p-0.5 rounded px-2 transition-colors">
          <span class="text-slate-600 shrink-0 select-none">{{ log.time }}</span>
          <div class="flex items-start gap-2 break-all w-full">
             <CheckCircle2 v-if="log.type === 'success'" class="w-4 h-4 text-emerald-500 shrink-0 mt-0.5" />
             <AlertCircle v-else-if="log.type === 'error'" class="w-4 h-4 text-red-500 shrink-0 mt-0.5" />
             <div v-else class="w-4 h-4 shrink-0 flex items-center justify-center mt-0.5">
               <div class="w-1.5 h-1.5 rounded-full bg-blue-500"></div>
             </div>
             <pre class="whitespace-pre-wrap font-mono" :class="{
               'text-slate-300': log.type === 'info',
               'text-red-400': log.type === 'error',
               'text-emerald-400': log.type === 'success'
             }">{{ log.msg }}</pre>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.custom-scrollbar::-webkit-scrollbar {
  width: 10px;
}
.custom-scrollbar::-webkit-scrollbar-track {
  background: #0f172a;
}
.custom-scrollbar::-webkit-scrollbar-thumb {
  background: #334155;
  border-radius: 5px;
  border: 2px solid #0f172a;
}
.custom-scrollbar::-webkit-scrollbar-thumb:hover {
  background: #475569;
}
</style>
