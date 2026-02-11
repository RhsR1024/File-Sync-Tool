<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { getHistory, clearHistory, type HistoryEntry } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { Trash2, Folder, FileText, ChevronDown, ChevronRight, HardDrive, Play, Pause, Save, Settings, XCircle, CheckCircle } from 'lucide-vue-next';

const { t } = useI18n();
const history = ref<HistoryEntry[]>([]);
const expandedIds = ref<Set<string>>(new Set());

async function load() {
  const store = await getHistory();
  history.value = store.entries;
}

async function clear() {
  if (confirm(t('history.clearConfirm') || 'Are you sure you want to clear all history records? This action cannot be undone.')) {
    await clearHistory();
    history.value = [];
  }
}

function toggleExpand(id: string) {
  if (expandedIds.value.has(id)) {
    expandedIds.value.delete(id);
  } else {
    expandedIds.value.add(id);
  }
}

function formatBytes(bytes: number) {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

function getIcon(action: string) {
    if (action === 'COPY_STARTED') return Play;
    if (action === 'COPY_COMPLETED') return CheckCircle;
    if (action === 'COPY_CANCELLED') return XCircle;
    if (action === 'PAUSE') return Pause;
    if (action === 'RESUME') return Play;
    if (action === 'CONFIG_CHANGE') return Settings;
    if (action === 'SCHEDULER_START') return Play;
    if (action === 'SCHEDULER_STOP') return XCircle;
    return Folder;
}

function getIconColor(action: string) {
    if (action === 'COPY_COMPLETED') return 'text-emerald-500';
    if (action === 'COPY_CANCELLED') return 'text-red-500';
    if (action === 'COPY_STARTED') return 'text-blue-500';
    if (action === 'PAUSE') return 'text-amber-500';
    return 'text-slate-500';
}

onMounted(load);
</script>

<template>
  <div class="p-6 h-full flex flex-col gap-6 bg-slate-50">
    <div class="flex justify-between items-center">
      <h2 class="text-2xl font-bold text-slate-800">{{ t('history.title') }}</h2>
      <button 
        v-if="history.length > 0"
        @click="clear"
        class="text-red-500 hover:text-red-700 hover:bg-red-50 px-3 py-2 rounded-lg font-medium flex items-center gap-2 transition-colors"
      >
        <Trash2 class="w-4 h-4" />
        {{ t('history.clear') }}
      </button>
    </div>

    <div class="flex-1 overflow-auto bg-white rounded-xl border border-slate-200 shadow-sm custom-scrollbar">
      <div v-if="history.length === 0" class="h-full flex flex-col items-center justify-center text-slate-400">
        <Folder class="w-16 h-16 mb-3 opacity-20" />
        <span>{{ t('history.noHistory') }}</span>
      </div>
      
      <div v-else class="divide-y divide-slate-100">
        <div v-for="entry in history" :key="entry.id" class="p-4 hover:bg-slate-50 transition-colors">
          <div class="flex items-start gap-3 cursor-pointer" @click="toggleExpand(entry.id)">
            <button class="mt-1 text-slate-400 hover:text-blue-500 transition-colors">
              <component :is="expandedIds.has(entry.id) ? ChevronDown : ChevronRight" class="w-5 h-5" />
            </button>
            
            <div class="flex-1">
              <div class="flex justify-between items-start mb-1">
                <h3 class="font-bold text-slate-700 flex items-center gap-2">
                  <component :is="getIcon(entry.action_type || '')" class="w-4 h-4" :class="getIconColor(entry.action_type || '')" />
                  {{ entry.description || entry.folder_name }}
                </h3>
                <span class="text-xs text-slate-400 font-mono">{{ new Date(entry.timestamp).toLocaleString() }}</span>
              </div>
              
              <div v-if="entry.action_type && entry.action_type.startsWith('COPY')" class="grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-1 text-sm text-slate-500 mt-2">
                <div class="flex items-center gap-2 truncate" :title="entry.source_path">
                  <span class="w-12 text-xs font-semibold uppercase text-slate-400">{{ t('history.source') }}:</span>
                  <span class="font-mono text-xs truncate">{{ entry.source_path }}</span>
                </div>
                <div class="flex items-center gap-2 truncate" :title="entry.target_path">
                  <span class="w-12 text-xs font-semibold uppercase text-slate-400">{{ t('history.target') }}:</span>
                  <span class="font-mono text-xs truncate">{{ entry.target_path }}</span>
                </div>
                <div class="flex items-center gap-2">
                   <span class="w-12 text-xs font-semibold uppercase text-slate-400">{{ t('history.size') }}:</span>
                   <span class="font-mono text-xs">{{ formatBytes(entry.total_size) }}</span>
                </div>
                <div class="flex items-center gap-2">
                   <span class="w-12 text-xs font-semibold uppercase text-slate-400">{{ t('history.count') }}:</span>
                   <span class="font-mono text-xs">{{ entry.copied_files_count }} files</span>
                </div>
              </div>
              <div v-else class="text-sm text-slate-500 mt-1 italic">
                  {{ entry.action_type }} event
              </div>
            </div>
          </div>
          
          <!-- File List -->
          <div v-if="expandedIds.has(entry.id) && entry.files && entry.files.length > 0" class="ml-8 mt-3 pl-4 border-l-2 border-slate-100">
             <h4 class="text-xs font-semibold text-slate-400 uppercase mb-2 flex items-center gap-2">
               <FileText class="w-3 h-3" />
               {{ t('history.files') }}
             </h4>
             <ul class="space-y-1">
               <li v-for="file in entry.files" :key="file" class="text-xs font-mono text-slate-600 truncate hover:text-blue-600">
                 {{ file }}
               </li>
             </ul>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.custom-scrollbar::-webkit-scrollbar {
  width: 8px;
}
.custom-scrollbar::-webkit-scrollbar-track {
  background: transparent;
}
.custom-scrollbar::-webkit-scrollbar-thumb {
  background: #cbd5e1;
  border-radius: 4px;
}
.custom-scrollbar::-webkit-scrollbar-thumb:hover {
  background: #94a3b8;
}
</style>
