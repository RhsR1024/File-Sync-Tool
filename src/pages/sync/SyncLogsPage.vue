<script setup lang="ts">
import { onMounted, onActivated, ref, watch, nextTick } from 'vue';
import { Activity, AlertCircle, CheckCircle2, Eraser, Terminal } from 'lucide-vue-next';
import Empty from '@/components/Empty.vue';
import { getConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { appStore } from '@/lib/store';

defineOptions({ name: 'SyncLogsPage' });

// Pixel distance from the bottom that still counts as "user is at the tail";
// scrolling further up disables auto-scroll until the user explicitly opts back in.
const AUTO_SCROLL_THRESHOLD_PX = 60;

const { t } = useI18n();
const logContainer = ref<HTMLElement | null>(null);
const autoScroll = ref(true);

function clearLogs() {
  appStore.logs.splice(0, appStore.logs.length);
}

function scrollToBottom() {
  if (logContainer.value && autoScroll.value) {
    nextTick(() => {
      logContainer.value!.scrollTop = logContainer.value!.scrollHeight;
    });
  }
}

function onScroll() {
  if (!logContainer.value) return;
  const el = logContainer.value;
  autoScroll.value =
    el.scrollHeight - el.scrollTop - el.clientHeight < AUTO_SCROLL_THRESHOLD_PX;
}

function logKindLabel(type: string): string {
  switch (type) {
    case 'success':
      return t('console.logKind.success');
    case 'error':
      return t('console.logKind.error');
    case 'command':
      return t('console.logKind.command');
    default:
      return t('console.logKind.info');
  }
}

watch(() => appStore.logs.length, () => {
  scrollToBottom();
});

onActivated(() => { getConfig(); scrollToBottom(); });
onMounted(() => { getConfig(); scrollToBottom(); });
</script>

<template>
  <div class="p-6 h-full flex flex-col gap-4 bg-slate-50">
    <div class="flex-1 bg-[#0f172a] rounded-xl overflow-hidden flex flex-col shadow-xl border border-slate-800">
      <div class="p-3 border-b border-slate-800 flex justify-between items-center bg-slate-900/80 backdrop-blur">
        <div class="flex items-center gap-2">
          <div class="flex gap-1.5 ml-2" aria-hidden="true">
            <div class="w-2.5 h-2.5 rounded-full bg-red-500/20 border border-red-500/50"></div>
            <div class="w-2.5 h-2.5 rounded-full bg-yellow-500/20 border border-yellow-500/50"></div>
            <div class="w-2.5 h-2.5 rounded-full bg-green-500/20 border border-green-500/50"></div>
          </div>
          <h3 class="ml-3 text-slate-400 font-mono text-xs uppercase tracking-widest">{{ t('console.logs') }}</h3>
          <span class="text-slate-600 font-mono text-xs">({{ appStore.logs.length }} / {{ appStore.maxLogLines }})</span>
        </div>
        <div class="flex items-center gap-2">
          <button v-if="!autoScroll" @click="autoScroll = true; scrollToBottom()"
                  class="text-slate-500 hover:text-blue-400 px-2 py-1 rounded-md hover:bg-slate-800 transition-colors motion-reduce:transition-none text-xs font-mono focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400/60 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-900">
            ↓ {{ t('console.scrollToBottom') }}
          </button>
          <button @click="clearLogs"
                  :aria-label="t('console.clear')"
                  :title="t('console.clear')"
                  :disabled="appStore.logs.length === 0"
                  class="text-slate-500 hover:text-white p-1.5 rounded-md hover:bg-slate-800 transition-colors motion-reduce:transition-none group disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-slate-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400/60 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-900">
            <Eraser class="w-4 h-4 group-hover:text-red-400 transition-colors motion-reduce:transition-none" aria-hidden="true" />
          </button>
        </div>
      </div>
      <div ref="logContainer" @scroll="onScroll"
           class="flex-1 overflow-auto p-4 font-mono text-xs md:text-sm space-y-1.5 scrollbar-terminal">
        <Empty
          v-if="appStore.logs.length === 0"
          :icon="Terminal"
          :title="t('console.empty.title')"
          :dashed="false"
          class="h-full text-slate-300"
        />
        <div v-for="(log, i) in appStore.logs" :key="i"
             class="flex gap-3 hover:bg-white/5 p-0.5 rounded px-2 transition-colors motion-reduce:transition-none">
          <span class="text-slate-600 shrink-0 select-none">{{ log.time }}</span>
          <div class="flex items-start gap-2 break-all w-full">
            <span class="sr-only">{{ logKindLabel(log.type) }}:</span>
            <CheckCircle2
              v-if="log.type === 'success'"
              class="w-4 h-4 text-emerald-500 shrink-0 mt-0.5"
              :aria-label="t('console.logKind.success')"
              role="img"
            />
            <AlertCircle
              v-else-if="log.type === 'error'"
              class="w-4 h-4 text-red-500 shrink-0 mt-0.5"
              :aria-label="t('console.logKind.error')"
              role="img"
            />
            <div
              v-else-if="log.type === 'command'"
              class="w-4 h-4 shrink-0 flex items-center justify-center mt-0.5"
              :aria-label="t('console.logKind.command')"
              role="img"
            >
              <div class="w-1.5 h-1.5 rounded-full bg-sky-400" aria-hidden="true"></div>
            </div>
            <div
              v-else
              class="w-4 h-4 shrink-0 flex items-center justify-center mt-0.5"
              :aria-label="t('console.logKind.info')"
              role="img"
            >
              <Activity class="w-3 h-3 text-blue-400" aria-hidden="true" />
            </div>
            <pre class="whitespace-pre-wrap font-mono" :class="{
              'text-slate-300': log.type === 'info',
              'text-red-400':   log.type === 'error',
              'text-emerald-400': log.type === 'success',
              'text-sky-400 font-semibold': log.type === 'command'
            }">{{ log.msg }}</pre>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
