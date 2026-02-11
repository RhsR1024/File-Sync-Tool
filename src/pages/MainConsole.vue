<script setup lang="ts">
import { ref, onMounted, onUnmounted, onActivated } from 'vue';
import { Play, Square, RefreshCw, Trash2, Clock, Activity, AlertCircle, CheckCircle2 } from 'lucide-vue-next';
import { getConfig, scanNow, type ScanResult, type AppConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';

// Add name for KeepAlive
defineOptions({
  name: 'MainConsole'
});

const { t } = useI18n();
const isRunning = ref(false);
const logs = ref<{ time: string; msg: string; type: 'info' | 'error' | 'success' }[]>([]);
const nextRunTime = ref<string>('-');
const config = ref<AppConfig | null>(null);
let timer: ReturnType<typeof setInterval> | null = null;

function addLog(msg: string, type: 'info' | 'error' | 'success' = 'info') {
  const time = new Date().toLocaleTimeString();
  logs.value.unshift({ time, msg, type });
  if (logs.value.length > 1000) logs.value.pop();
}

async function loadConfig() {
  try {
    config.value = await getConfig();
    addLog(t('console.configLoaded'), 'info');
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
  }
}

function startScheduler(isRestart = false) {
  if (!config.value) return;
  isRunning.value = true;
  if (!isRestart) {
      addLog(t('console.schedulerStarted', { interval: config.value.interval_minutes }), 'info');
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
  addLog(t('console.schedulerStopped'), 'info');
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
  loadConfig();
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
      <div class="bg-white p-5 rounded-xl border border-slate-200 shadow-sm flex flex-col justify-center gap-3">
        <div class="flex gap-3">
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
          <div class="flex items-start gap-2 break-all">
             <CheckCircle2 v-if="log.type === 'success'" class="w-4 h-4 text-emerald-500 shrink-0 mt-0.5" />
             <AlertCircle v-else-if="log.type === 'error'" class="w-4 h-4 text-red-500 shrink-0 mt-0.5" />
             <div v-else class="w-4 h-4 shrink-0 flex items-center justify-center mt-0.5">
               <div class="w-1.5 h-1.5 rounded-full bg-blue-500"></div>
             </div>
             <span :class="{
               'text-slate-300': log.type === 'info',
               'text-red-400': log.type === 'error',
               'text-emerald-400': log.type === 'success'
             }">{{ log.msg }}</span>
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
