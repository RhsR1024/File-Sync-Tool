<script setup lang="ts">
import { ref, onMounted, onUnmounted, onActivated } from 'vue';
import { Play, Square, RefreshCw, Trash2, Clock, Activity, AlertCircle, CheckCircle2, XCircle, Pause, PlayCircle } from 'lucide-vue-next';
import { getConfig, scanNow, cancelScan, pauseScan, resumeScan, addSystemEvent, type ScanResult, type AppConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { listen } from '@tauri-apps/api/event';

// Add name for KeepAlive
defineOptions({
  name: 'MainConsole'
});

const { t } = useI18n();
const isRunning = ref(false);
const logs = ref<{ time: string; msg: string; type: 'info' | 'error' | 'success' }[]>([]);
const nextRunTime = ref<string>('-');
const config = ref<AppConfig | null>(null);
const progress = ref<{ folder: string; percentage: number; copied: number; total: number; speed: number; eta: number } | null>(null);
const isCancelling = ref(false);
const isPaused = ref(false);
let timer: ReturnType<typeof setInterval> | null = null;
let unlistenLog: (() => void) | null = null;
let unlistenProgress: (() => void) | null = null;

async function handleCancel() {
  if (isCancelling.value) return;
  isCancelling.value = true;
  
  const targetName = progress.value?.folder || '';
  const msg = `${t('console.cancelling')} ${targetName ? '(' + targetName + ')' : ''}`;
  addLog(msg, 'info');
  // Log to history
  // Note: Backend cancelScan will trigger logic, but backend scanner might be busy.
  // Actually, scanner.rs already handles COPY_CANCELLED.
  // We don't need to add generic Cancel event if copy is running.
  // But if we want to log "User clicked cancel", we can.
  // Let's rely on scanner.rs for COPY_CANCELLED which is more accurate.
  
  try {
    await cancelScan();
  } catch (e) {
    addLog(`Cancel failed: ${e}`, 'error');
    isCancelling.value = false;
  }
}

async function togglePause() {
  if (!progress.value) return;
  
  const targetName = progress.value.folder || '';
  
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

function addLog(msg: string, type: 'info' | 'error' | 'success' = 'info') {
  const time = new Date().toLocaleTimeString();
  // Handle multiline messages (like tree view) by preserving whitespace
  logs.value.unshift({ time, msg, type });
  if (logs.value.length > 1000) logs.value.pop();
}

// Setup real-time listeners
async function setupListeners() {
    if (unlistenLog) unlistenLog();
    if (unlistenProgress) unlistenProgress();

    unlistenLog = await listen('log-message', (event: any) => {
        const payload = event.payload as { msg: string, level: string };
        let type: 'info' | 'error' | 'success' = 'info';
        if (payload.level === 'error') type = 'error';
        if (payload.level === 'success') type = 'success';
        addLog(payload.msg, type);
    });

    unlistenProgress = await listen('copy-progress', (event: any) => {
        const p = event.payload as { folder: string, total_bytes: number, copied_bytes: number, percentage: number, speed: number, eta_seconds: number };
        progress.value = {
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
                if (progress.value?.folder === p.folder) {
                    progress.value = null;
                    isPaused.value = false; // Reset pause state
                }
            }, 2000);
        }
    });
}

async function loadConfig(silentIfSame = false) {
  try {
    const newConfig = await getConfig();
    const isFirstLoad = config.value === null;
    const isChanged = JSON.stringify(config.value) !== JSON.stringify(newConfig);

    if (isChanged) {
        config.value = newConfig;
        // Only log if it's NOT a silent reload (or if it's first load)
        // Actually, user wants "Configuration loaded" ONLY if changed (or first load).
        // If silentIfSame is true, and it IS changed, we SHOULD log "Updated".
        // If silentIfSame is true, and NOT changed, we do nothing.
        
        // Let's refine:
        // If first load -> Log "Loaded"
        // If changed -> Log "Updated" (or Loaded)
        // If not changed -> Do nothing (if silentIfSame=true)
        
        addLog(t('console.configLoaded'), 'info');
    } else {
        // Not changed
        if (!silentIfSame) {
            // If not silent mode (manual call?), log it?
            // User complained about "switching back to console" triggering log.
            // onActivated calls loadConfig().
            // So we should pass silentIfSame=true from onActivated.
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
    progress.value = null; // Ensure progress is cleared when scan finishes
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
  
  // Initial run only if not a restart (to avoid double scan on tab switch)
  if (!isRestart) {
      handleScan();
  }
  
  // Schedule
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
  logs.value = [];
}

// Reload config when page is activated (switched back to)
onActivated(() => {
  loadConfig(true); // Pass true to silent if same
  // If timer is running, we might want to restart it to pick up new interval?
  // But user said: "real-time hot update config... next scheduled run use new config"
  // Our startScheduler sets up a timer with FIXED interval. 
  // To support dynamic interval change without stopping, we need more complex logic.
  // For now, let's at least reload the config object so the next scan uses new paths/versions.
  
  if (isRunning.value && config.value) {
       // Silent restart to apply potential interval changes
       if (timer) clearInterval(timer);
       startScheduler(true); 
   }
 });

onMounted(() => {
  loadConfig();
  setupListeners();
});

onUnmounted(() => {
  if (timer) clearInterval(timer);
  if (unlistenLog) unlistenLog();
  if (unlistenProgress) unlistenProgress();
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
      <div class="bg-white p-5 rounded-xl border border-slate-200 shadow-sm flex flex-col justify-center gap-3 relative overflow-hidden">
        <div class="flex gap-3 relative z-10">
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

        <!-- Progress Bar Overlay -->
        <div v-if="progress" class="absolute bottom-0 left-0 w-full h-1.5 bg-slate-100">
           <div class="h-full bg-blue-500 transition-all duration-300" :style="{ width: `${progress.percentage}%` }"></div>
        </div>
        <div v-if="progress" class="absolute inset-0 bg-white/95 z-20 flex flex-col items-center justify-center backdrop-blur-sm p-4 text-center">
           <div class="text-xs font-bold text-blue-600 mb-1 uppercase tracking-wider">{{ isPaused ? t('console.pausedStatus') : t('console.copying') }}</div>
           <div class="text-sm font-mono text-slate-700 mb-2 truncate max-w-full" :title="progress.folder">{{ progress.folder }}</div>
           <div class="w-full h-2 bg-slate-200 rounded-full overflow-hidden mb-1">
              <div class="h-full bg-blue-500 transition-all duration-300" :style="{ width: `${progress.percentage}%` }"></div>
           </div>
           
           <!-- Stats -->
           <div class="flex justify-between w-full text-xs text-slate-500 font-mono mb-3 px-1">
               <span>{{ progress.percentage.toFixed(1) }}%</span>
               <span v-if="!isPaused">{{ formatSpeed(progress.speed) }} - ETA: {{ formatDuration(progress.eta) }}</span>
               <span v-else class="text-amber-500">{{ t('console.paused') }}</span>
           </div>
           
           <div class="flex gap-3">
               <button 
                 @click="togglePause"
                 class="text-xs px-3 py-1.5 rounded-full font-medium border flex items-center gap-1 transition-colors"
                 :class="isPaused ? 'text-emerald-600 border-emerald-200 hover:bg-emerald-50' : 'text-amber-600 border-amber-200 hover:bg-amber-50'"
               >
                 <component :is="isPaused ? PlayCircle : Pause" class="w-3 h-3" />
                 {{ isPaused ? t('console.resume') : t('console.pause') }}
               </button>

               <button 
                 @click="handleCancel"
                 class="text-xs text-red-500 hover:text-red-700 hover:bg-red-50 px-3 py-1.5 rounded-full font-medium border border-red-200 flex items-center gap-1 transition-colors"
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
        <div v-if="logs.length === 0" class="h-full flex flex-col items-center justify-center text-slate-700">
           <Activity class="w-12 h-12 mb-2 opacity-20" />
           <span class="italic">{{ t('console.noLogs') }}</span>
        </div>
        <div v-for="(log, i) in logs" :key="i" class="flex gap-3 hover:bg-white/5 p-0.5 rounded px-2 transition-colors">
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
