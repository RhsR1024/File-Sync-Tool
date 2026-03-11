<script setup lang="ts">
import { onMounted, onActivated, ref, watch, nextTick } from 'vue';
import { Trash2, Activity, AlertCircle, CheckCircle2 } from 'lucide-vue-next';
import Empty from '@/components/Empty.vue';
import { getConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { appStore } from '@/lib/store';

defineOptions({ name: 'MainConsole' });

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
  // If user scrolled up more than 60px from bottom, disable auto-scroll
  autoScroll.value = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
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
          <div class="flex gap-1.5 ml-2">
            <div class="w-2.5 h-2.5 rounded-full bg-red-500/20 border border-red-500/50"></div>
            <div class="w-2.5 h-2.5 rounded-full bg-yellow-500/20 border border-yellow-500/50"></div>
            <div class="w-2.5 h-2.5 rounded-full bg-green-500/20 border border-green-500/50"></div>
          </div>
          <h3 class="ml-3 text-slate-400 font-mono text-xs uppercase tracking-widest">{{ t('console.logs') }}</h3>
          <span class="text-slate-600 font-mono text-xs">({{ appStore.logs.length }} / {{ appStore.maxLogLines }})</span>
        </div>
        <div class="flex items-center gap-2">
          <button v-if="!autoScroll" @click="autoScroll = true; scrollToBottom()"
                  class="text-slate-500 hover:text-blue-400 px-2 py-1 rounded-md hover:bg-slate-800 transition-colors text-xs font-mono">
            ↓ {{ t('console.scrollToBottom') }}
          </button>
          <button @click="clearLogs"
                  class="text-slate-500 hover:text-white p-1.5 rounded-md hover:bg-slate-800 transition-colors group"
                  title="Clear logs">
            <Trash2 class="w-4 h-4 group-hover:text-red-400 transition-colors" />
          </button>
        </div>
      </div>
      <div ref="logContainer" @scroll="onScroll"
           class="flex-1 overflow-auto p-4 font-mono text-xs md:text-sm space-y-1.5 custom-scrollbar">
        <Empty
          v-if="appStore.logs.length === 0"
          :icon="Activity"
          :title="t('console.noLogs')"
          :dashed="false"
          class="h-full text-slate-700"
        />
        <div v-for="(log, i) in appStore.logs" :key="i"
             class="flex gap-3 hover:bg-white/5 p-0.5 rounded px-2 transition-colors">
          <span class="text-slate-600 shrink-0 select-none">{{ log.time }}</span>
          <div class="flex items-start gap-2 break-all w-full">
            <CheckCircle2 v-if="log.type === 'success'" class="w-4 h-4 text-emerald-500 shrink-0 mt-0.5" />
            <AlertCircle v-else-if="log.type === 'error'" class="w-4 h-4 text-red-500 shrink-0 mt-0.5" />
            <div v-else-if="log.type === 'command'" class="w-4 h-4 shrink-0 flex items-center justify-center mt-0.5">
              <div class="w-1.5 h-1.5 rounded-full bg-sky-400"></div>
            </div>
            <div v-else class="w-4 h-4 shrink-0 flex items-center justify-center mt-0.5">
              <div class="w-1.5 h-1.5 rounded-full bg-blue-500"></div>
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

<style scoped>
.custom-scrollbar::-webkit-scrollbar { width: 10px; }
.custom-scrollbar::-webkit-scrollbar-track { background: #0f172a; }
.custom-scrollbar::-webkit-scrollbar-thumb { background: #334155; border-radius: 5px; border: 2px solid #0f172a; }
.custom-scrollbar::-webkit-scrollbar-thumb:hover { background: #475569; }
</style>
