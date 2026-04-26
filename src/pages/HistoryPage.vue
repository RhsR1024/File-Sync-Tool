<script setup lang="ts">
import { computed, onUnmounted, ref, onMounted } from 'vue';
import { getHistory, clearHistory, type HistoryEntry } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import {
  Trash2,
  Folder,
  FileText,
  ChevronDown,
  ChevronRight,
  Play,
  Pause,
  Settings,
  XCircle,
  CheckCircle,
  HelpCircle,
  RefreshCw,
} from 'lucide-vue-next';
import Empty from '@/components/Empty.vue';
import { formatDateTime } from '@/lib/formatters';

const { t, locale } = useI18n();
const history = ref<HistoryEntry[]>([]);
const expandedIds = ref<Set<string>>(new Set());

// Locale-aware wrapper so templates can render timestamps without reaching
// for `.value` (which Vue's template auto-unwrap already does, but keeping a
// helper makes the intent obvious).
function formatTimestamp(value: string): string {
  return formatDateTime(value, locale.value);
}

// `confirmingClear` flips to true on the first Clear click and reverts after
// CLEAR_CONFIRM_TIMEOUT_MS so a second click within the window commits.  This
// avoids a destructive accidental click without introducing a modal.
const CLEAR_CONFIRM_TIMEOUT_MS = 3000;
const confirmingClear = ref(false);
let confirmResetTimer: ReturnType<typeof setTimeout> | null = null;

const recentLabel = computed(() =>
  t('history.recentN', { n: history.value.length }),
);

async function load() {
  const store = await getHistory();
  history.value = store.entries;
}

function cancelConfirmReset() {
  if (confirmResetTimer) {
    clearTimeout(confirmResetTimer);
    confirmResetTimer = null;
  }
}

async function clear() {
  if (!confirmingClear.value) {
    // First click — arm the confirmation window and bail.
    confirmingClear.value = true;
    cancelConfirmReset();
    confirmResetTimer = setTimeout(() => {
      confirmingClear.value = false;
      confirmResetTimer = null;
    }, CLEAR_CONFIRM_TIMEOUT_MS);
    return;
  }

  cancelConfirmReset();
  confirmingClear.value = false;
  await clearHistory();
  history.value = [];
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
  // Fallback so new event types stay visible in the timeline instead of
  // silently rendering with the same generic folder icon.
  return HelpCircle;
}

function getIconColor(action: string) {
  if (action === 'COPY_COMPLETED') return 'text-emerald-500';
  if (action === 'COPY_CANCELLED') return 'text-red-500';
  if (action === 'COPY_STARTED') return 'text-blue-500';
  if (action === 'PAUSE') return 'text-amber-500';
  if (action === 'CONFIG_CHANGE') return 'text-slate-500';
  if (action === 'SCHEDULER_START') return 'text-blue-500';
  if (action === 'SCHEDULER_STOP') return 'text-slate-500';
  if (action === 'RESUME') return 'text-blue-500';
  // Mute unknown kinds so they still render but don't compete visually.
  return 'text-slate-400';
}

onMounted(load);
onUnmounted(cancelConfirmReset);
</script>

<template>
  <div class="p-6 h-full flex flex-col gap-6 bg-slate-50">
    <div class="flex justify-between items-center">
      <h2 class="text-2xl font-bold text-slate-800">{{ t('history.title') }}</h2>
      <button
        v-if="history.length > 0"
        type="button"
        :aria-label="confirmingClear ? t('history.clearConfirm') : t('history.clear')"
        :title="confirmingClear ? t('history.clearConfirm') : t('history.clear')"
        @click="clear"
        class="px-3 py-2 rounded-lg font-medium flex items-center gap-2 transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500/40 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50"
        :class="confirmingClear
          ? 'bg-red-500 text-white hover:bg-red-600'
          : 'text-red-500 hover:text-red-700 hover:bg-red-50'"
      >
        <Trash2 class="w-4 h-4" aria-hidden="true" />
        <span>{{ confirmingClear ? t('history.clearConfirm') : t('history.clear') }}</span>
      </button>
    </div>

    <div class="flex-1 overflow-auto bg-white rounded-xl border border-slate-200 shadow-sm scrollbar-light">
      <Empty
        v-if="history.length === 0"
        :icon="Folder"
        :title="t('history.noHistory')"
        :action-label="t('history.empty.actionLabel')"
        action-tone="subtle"
        class="h-full"
        @action="load"
      />

      <div v-else class="divide-y divide-slate-100">
        <div v-for="entry in history" :key="entry.id" class="px-4">
          <div
            class="flex items-start gap-3 py-4 cursor-pointer rounded-lg -mx-2 px-2 transition-colors motion-reduce:transition-none hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/40 focus-visible:ring-offset-2 focus-visible:ring-offset-white"
            role="button"
            tabindex="0"
            :aria-expanded="expandedIds.has(entry.id)"
            @click="toggleExpand(entry.id)"
            @keydown.enter.prevent="toggleExpand(entry.id)"
            @keydown.space.prevent="toggleExpand(entry.id)"
          >
            <span class="mt-1 text-slate-400" aria-hidden="true">
              <component :is="expandedIds.has(entry.id) ? ChevronDown : ChevronRight" class="w-5 h-5" />
            </span>

            <div class="flex-1 min-w-0">
              <div class="flex justify-between items-start mb-1 gap-3">
                <h3 class="font-bold text-slate-700 flex items-center gap-2 min-w-0">
                  <component :is="getIcon(entry.action_type || '')" class="w-4 h-4 shrink-0" :class="getIconColor(entry.action_type || '')" aria-hidden="true" />
                  <span class="truncate">{{ entry.description || entry.folder_name }}</span>
                </h3>
                <span class="text-xs text-slate-400 font-mono shrink-0">{{ formatTimestamp(entry.timestamp) }}</span>
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
          <div v-if="expandedIds.has(entry.id) && entry.files && entry.files.length > 0" class="ml-8 mb-4 pl-4 border-l-2 border-slate-100">
            <h4 class="text-xs font-semibold text-slate-400 uppercase mb-2 flex items-center gap-2">
              <FileText class="w-3 h-3" aria-hidden="true" />
              {{ t('history.files') }}
            </h4>
            <ul class="space-y-1 max-h-80 overflow-y-auto scrollbar-light">
              <li v-for="file in entry.files" :key="file" class="text-xs font-mono text-slate-600 truncate hover:text-blue-600">
                {{ file }}
              </li>
            </ul>
          </div>
        </div>
      </div>
    </div>

    <div v-if="history.length > 0" class="flex items-center justify-between text-xs text-slate-400">
      <span>{{ recentLabel }}</span>
      <button
        type="button"
        class="inline-flex items-center gap-1.5 rounded-md px-2 py-1 hover:text-slate-600 hover:bg-slate-100 transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/40 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50"
        :aria-label="t('history.empty.actionLabel')"
        @click="load"
      >
        <RefreshCw class="w-3 h-3" aria-hidden="true" />
        <span>{{ t('history.empty.actionLabel') }}</span>
      </button>
    </div>
  </div>
</template>
