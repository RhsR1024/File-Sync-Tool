<script setup lang="ts">
import { onMounted, onActivated } from 'vue';
import { Trash2, Activity, AlertCircle, CheckCircle2, Copy, UploadCloud } from 'lucide-vue-next';
import { getConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { appStore } from '@/lib/store';

defineOptions({ name: 'MainConsole' });

const { t } = useI18n();

function clearLogs() {
  appStore.logs.splice(0, appStore.logs.length);
}

function clearRecords() {
  appStore.taskRecords.splice(0, appStore.taskRecords.length);
}

function formatRecordSpeed(bytesPerSec: number): string {
  if (!bytesPerSec || bytesPerSec <= 0) return '';
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  const i = Math.floor(Math.log(Math.max(bytesPerSec, 1)) / Math.log(1024));
  return `${(bytesPerSec / Math.pow(1024, i)).toFixed(1)} ${units[Math.min(i, 3)]}`;
}

onActivated(() => { getConfig(); });
onMounted(() => { getConfig(); });
</script>

<template>
  <div class="p-6 h-full flex flex-col gap-4 bg-slate-50">

    <!-- ── Task Records Panel ──────────────────────────────────────── -->
    <div class="bg-[#0f172a] rounded-xl overflow-hidden flex flex-col shadow-xl border border-slate-800 shrink-0"
         style="font-family:'JetBrains Mono','IBM Plex Mono','Fira Code',ui-monospace,monospace;">

      <!-- Header -->
      <div class="px-4 py-2.5 border-b border-slate-800 flex items-center justify-between"
           style="background:linear-gradient(90deg,#0c1629 0%,#111827 100%);">
        <div class="flex items-center gap-2.5">
          <svg width="13" height="13" viewBox="0 0 13 13" fill="none">
            <rect y="3"  width="7"  height="1.5" rx=".75" fill="#60a5fa"/>
            <rect y="6"  width="11" height="1.5" rx=".75" fill="#60a5fa" opacity=".5"/>
            <rect y="9"  width="5"  height="1.5" rx=".75" fill="#60a5fa" opacity=".25"/>
          </svg>
          <span class="text-[11px] tracking-[.15em] uppercase text-slate-500">{{ t('console.taskRecords') }}</span>
          <span v-if="appStore.taskRecords.length"
                class="text-[10px] px-1.5 py-px rounded font-bold tabular-nums"
                style="background:rgba(59,130,246,.12);color:#60a5fa;border:1px solid rgba(59,130,246,.25);">
            {{ appStore.taskRecords.length }}
          </span>
        </div>
        <button v-if="appStore.taskRecords.length"
                @click="clearRecords"
                class="flex items-center gap-1 text-[11px] text-slate-600 hover:text-red-400 transition-colors px-2 py-0.5 rounded clear-btn">
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
            <path d="M1 1l8 8M9 1L1 9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
          </svg>
          {{ t('console.clearRecords') }}
        </button>
      </div>

      <!-- Records scroll area -->
      <div class="overflow-y-auto task-records-scroll" style="max-height:280px;min-height:56px;">

        <!-- Empty state -->
        <div v-if="!appStore.taskRecords.length"
             class="flex flex-col items-center justify-center py-7 opacity-25 select-none">
          <svg width="28" height="28" viewBox="0 0 28 28" fill="none" class="mb-2">
            <rect x="3"  y="7"  width="22" height="2.5" rx="1.25" fill="#475569"/>
            <rect x="3"  y="13" width="16" height="2.5" rx="1.25" fill="#475569" opacity=".55"/>
            <rect x="3"  y="19" width="10" height="2.5" rx="1.25" fill="#475569" opacity=".25"/>
          </svg>
          <span class="text-slate-600 text-[11px] tracking-widest uppercase">{{ t('console.noRecords') }}</span>
        </div>

        <!-- Record Card -->
        <div v-for="rec in appStore.taskRecords" :key="rec.id"
             class="mx-2 my-1 record-card relative overflow-hidden"
             :style="{
               borderRadius:'4px',
               background:'linear-gradient(135deg,#1e293b 0%,#172033 100%)',
               borderLeft:`2px solid ${rec.phase==='completed'?'#10b981':rec.phase==='deploying'?'#a855f7':'#3b82f6'}`,
               boxShadow:'0 1px 6px rgba(0,0,0,.45),inset 0 0 0 1px rgba(255,255,255,.03)'
             }">

          <!-- Shimmer overlay (active only) -->
          <div v-if="rec.phase!=='completed'" class="absolute inset-0 pointer-events-none shimmer-bar" />

          <div class="px-3 py-2.5 relative">
            <!-- Top row: timestamp + name + badge -->
            <div class="flex items-start justify-between gap-2 mb-2">
              <div class="flex-1 min-w-0">
                <div class="text-[11px] text-slate-400 mb-0.5 tabular-nums">{{ rec.startTime }}</div>
                <div class="text-sm font-bold text-slate-200 truncate leading-snug" :title="rec.folder">
                  {{ rec.folder }}
                </div>
              </div>
              <!-- Phase badge -->
              <div class="shrink-0 mt-3">
                <span v-if="rec.phase==='copying'"
                      class="inline-flex items-center gap-1 text-xs font-bold px-1.5 py-px rounded-sm"
                      style="background:rgba(59,130,246,.12);color:#60a5fa;border:1px solid rgba(59,130,246,.28);">
                  <span class="w-1.5 h-1.5 rounded-full bg-blue-400 animate-pulse inline-block"/>COPYING
                </span>
                <span v-else-if="rec.phase==='deploying'"
                      class="inline-flex items-center gap-1 text-xs font-bold px-1.5 py-px rounded-sm"
                      style="background:rgba(168,85,247,.12);color:#c084fc;border:1px solid rgba(168,85,247,.28);">
                  <span class="w-1.5 h-1.5 rounded-full bg-purple-400 animate-pulse inline-block"/>DEPLOYING
                </span>
                <span v-else
                      class="inline-flex items-center gap-1 text-xs font-bold px-1.5 py-px rounded-sm"
                      style="background:rgba(16,185,129,.1);color:#34d399;border:1px solid rgba(16,185,129,.25);">
                  <CheckCircle2 class="w-3 h-3"/>DONE
                </span>
              </div>
            </div>

            <!-- Phase 1 — Local Copy -->
            <div class="space-y-1 mb-0.5">
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-1.5">
                  <Copy class="w-3 h-3"
                        :class="rec.copyCompleted?'text-emerald-500 opacity-70':'text-blue-400 opacity-60'"/>
                  <span class="text-xs uppercase tracking-wider"
                        :class="rec.copyCompleted?'text-emerald-400':'text-slate-400'">
                    {{ t('console.localCopy') }}
                  </span>
                </div>
                <div class="flex items-center gap-1.5">
                  <span v-if="rec.phase==='copying' && rec.speed>0"
                        class="text-[11px] text-blue-400 tabular-nums animate-pulse">
                    {{ formatRecordSpeed(rec.speed) }}
                  </span>
                  <span class="text-xs font-bold tabular-nums"
                        :class="rec.copyCompleted?'text-emerald-400':'text-blue-400'">
                    {{ rec.copyPercentage.toFixed(0) }}%
                  </span>
                  <CheckCircle2 v-if="rec.copyCompleted" class="w-3.5 h-3.5 text-emerald-500"/>
                </div>
              </div>
              <!-- Progress bar -->
              <div class="h-[3px] rounded-full overflow-hidden" style="background:rgba(255,255,255,.05);">
                <div class="h-full rounded-full transition-all duration-300 ease-out"
                     :style="{
                       width:`${rec.copyPercentage}%`,
                       background:rec.copyCompleted
                         ?'linear-gradient(90deg,#059669,#10b981)'
                         :'linear-gradient(90deg,#1d4ed8,#3b82f6,#60a5fa)',
                       boxShadow:rec.copyCompleted?'0 0 5px rgba(16,185,129,.5)':'0 0 5px rgba(59,130,246,.6)'
                     }"/>
              </div>
              <div v-if="rec.localPath" class="text-[11px] text-slate-400 truncate" :title="rec.localPath">
                → {{ rec.localPath }}
              </div>
            </div>

            <!-- Connector -->
            <div v-if="rec.hasRemote || (rec.copyCompleted && rec.phase==='deploying')"
                 class="flex items-center gap-1.5 my-1.5 pl-1">
              <div class="w-px" style="height:8px;background:linear-gradient(180deg,rgba(168,85,247,.35) 0%,rgba(168,85,247,.1) 100%);"/>
            </div>

            <!-- Phase 2 — Remote Deploy -->
            <div v-if="rec.hasRemote" class="space-y-1">
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-1.5">
                  <UploadCloud class="w-3 h-3"
                               :class="rec.deployCompleted?'text-emerald-500 opacity-70':'text-purple-400 opacity-70'"/>
                  <span class="text-xs uppercase tracking-wider"
                        :class="rec.deployCompleted?'text-emerald-400':'text-purple-400'">
                    {{ t('console.remotePush') }}
                  </span>
                  <span class="text-[10px] text-slate-600 tabular-nums">
                    {{ rec.remoteServers.filter(s=>s.completed).length }}/{{ rec.remoteServers.length }}
                  </span>
                </div>
                <div class="flex items-center gap-1.5">
                  <span v-if="rec.phase==='deploying' && rec.speed>0"
                        class="text-[11px] text-purple-400 tabular-nums animate-pulse">
                    {{ formatRecordSpeed(rec.speed) }}
                  </span>
                  <span class="text-xs font-bold tabular-nums"
                        :class="rec.deployCompleted?'text-emerald-400':'text-purple-400'">
                    {{ rec.deployPercentage.toFixed(0) }}%
                  </span>
                  <CheckCircle2 v-if="rec.deployCompleted" class="w-3.5 h-3.5 text-emerald-500"/>
                </div>
              </div>
              <div class="h-[3px] rounded-full overflow-hidden" style="background:rgba(255,255,255,.05);">
                <div class="h-full rounded-full transition-all duration-300 ease-out"
                     :style="{
                       width:`${rec.deployPercentage}%`,
                       background:rec.deployCompleted
                         ?'linear-gradient(90deg,#059669,#10b981)'
                         :'linear-gradient(90deg,#6d28d9,#a855f7,#c084fc)',
                       boxShadow:rec.deployCompleted?'0 0 5px rgba(16,185,129,.5)':'0 0 5px rgba(168,85,247,.6)'
                     }"/>
              </div>
              <!-- Server list -->
              <div class="space-y-0.5 mt-0.5">
                <div v-for="srv in (rec.remoteExpanded ? rec.remoteServers : rec.remoteServers.slice(0,3))"
                     :key="srv.key"
                     class="flex items-center gap-1 text-[11px] leading-snug">
                  <span class="shrink-0" :class="srv.completed?'text-emerald-400':'text-purple-400'">→</span>
                  <span class="truncate text-slate-400 flex-1" :title="srv.label">{{ srv.label }}</span>
                  <CheckCircle2 v-if="srv.completed" class="w-3 h-3 text-emerald-500 shrink-0"/>
                  <span v-else class="text-[10px] text-purple-400 tabular-nums shrink-0">
                    {{ srv.percentage.toFixed(0) }}%
                  </span>
                </div>
                <button v-if="rec.remoteServers.length > 3"
                        @click.stop="rec.remoteExpanded = !rec.remoteExpanded"
                        class="text-[10px] text-slate-500 hover:text-slate-300 transition-colors mt-0.5 pl-2">
                  {{ rec.remoteExpanded ? '▲ 收起' : `▼ 还有 ${rec.remoteServers.length - 3} 台...` }}
                </button>
              </div>
            </div>

            <!-- Awaiting deploy placeholder -->
            <div v-else-if="rec.copyCompleted && rec.phase==='deploying'"
                 class="flex items-center gap-1.5 opacity-20 mt-0.5">
              <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
                <circle cx="5" cy="5" r="4" stroke="#94a3b8" stroke-width="1" stroke-dasharray="2 1.5"/>
              </svg>
              <span class="text-xs text-slate-500 uppercase tracking-wider">
                {{ t('console.remotePush') }} · {{ t('console.waiting') }}
              </span>
            </div>

          </div>
        </div>
      </div>
    </div>

    <!-- ── Execution Logs Panel ───────────────────────────────────── -->
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
        <button @click="clearLogs"
                class="text-slate-500 hover:text-white p-1.5 rounded-md hover:bg-slate-800 transition-colors group"
                title="Clear logs">
          <Trash2 class="w-4 h-4 group-hover:text-red-400 transition-colors" />
        </button>
      </div>
      <div class="flex-1 overflow-auto p-4 font-mono text-xs md:text-sm space-y-1.5 custom-scrollbar">
        <div v-if="appStore.logs.length === 0"
             class="h-full flex flex-col items-center justify-center text-slate-700">
          <Activity class="w-12 h-12 mb-2 opacity-20" />
          <span class="italic">{{ t('console.noLogs') }}</span>
        </div>
        <div v-for="(log, i) in appStore.logs" :key="i"
             class="flex gap-3 hover:bg-white/5 p-0.5 rounded px-2 transition-colors">
          <span class="text-slate-600 shrink-0 select-none">{{ log.time }}</span>
          <div class="flex items-start gap-2 break-all w-full">
            <CheckCircle2 v-if="log.type === 'success'" class="w-4 h-4 text-emerald-500 shrink-0 mt-0.5" />
            <AlertCircle v-else-if="log.type === 'error'" class="w-4 h-4 text-red-500 shrink-0 mt-0.5" />
            <div v-else class="w-4 h-4 shrink-0 flex items-center justify-center mt-0.5">
              <div class="w-1.5 h-1.5 rounded-full bg-blue-500"></div>
            </div>
            <pre class="whitespace-pre-wrap font-mono" :class="{
              'text-slate-300': log.type === 'info',
              'text-red-400':   log.type === 'error',
              'text-emerald-400': log.type === 'success'
            }">{{ log.msg }}</pre>
          </div>
        </div>
      </div>
    </div>

  </div>
</template>

<style scoped>
@keyframes shimmer-move {
  0%   { transform: translateX(-100%); }
  100% { transform: translateX(300%); }
}
.shimmer-bar::after {
  content: '';
  position: absolute;
  inset: 0;
  width: 33%;
  background: linear-gradient(90deg, transparent, rgba(255,255,255,.022), transparent);
  animation: shimmer-move 2.8s ease-in-out infinite;
}
.record-card { transition: box-shadow .15s ease; }
.record-card:hover {
  box-shadow: 0 2px 14px rgba(0,0,0,.55), inset 0 0 0 1px rgba(255,255,255,.06) !important;
}
.clear-btn { border: 1px solid transparent; transition: border-color .15s, color .15s; }
.clear-btn:hover { border-color: rgba(239,68,68,.25); }

.task-records-scroll::-webkit-scrollbar { width: 4px; }
.task-records-scroll::-webkit-scrollbar-track { background: transparent; }
.task-records-scroll::-webkit-scrollbar-thumb { background: #1e293b; border-radius: 2px; }
.task-records-scroll::-webkit-scrollbar-thumb:hover { background: #334155; }

.custom-scrollbar::-webkit-scrollbar { width: 10px; }
.custom-scrollbar::-webkit-scrollbar-track { background: #0f172a; }
.custom-scrollbar::-webkit-scrollbar-thumb { background: #334155; border-radius: 5px; border: 2px solid #0f172a; }
.custom-scrollbar::-webkit-scrollbar-thumb:hover { background: #475569; }
</style>
