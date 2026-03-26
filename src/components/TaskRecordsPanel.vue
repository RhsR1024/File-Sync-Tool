<script setup lang="ts">
import { computed } from 'vue';
import { CheckCircle2, Copy, Trash2 } from 'lucide-vue-next';
import { appStore, type TaskRecordPhase } from '@/lib/store';
import { useI18n } from 'vue-i18n';

withDefaults(defineProps<{
  embedded?: boolean;
  showHeader?: boolean;
}>(), {
  embedded: false,
  showHeader: true,
});

const { t } = useI18n();

const orderedRecords = computed(() =>
  [...appStore.taskRecords].sort((a, b) => (b.updatedAt || 0) - (a.updatedAt || 0))
);

function clearRecords() {
  appStore.taskRecords.splice(0, appStore.taskRecords.length);
}

function formatRecordSpeed(bytesPerSec: number): string {
  if (!bytesPerSec || bytesPerSec <= 0) return '-';
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  const i = Math.floor(Math.log(Math.max(bytesPerSec, 1)) / Math.log(1024));
  return `${(bytesPerSec / Math.pow(1024, i)).toFixed(1)} ${units[Math.min(i, 3)]}`;
}

function statusText(phase: TaskRecordPhase): string {
  if (phase === 'queued') return t('console.phaseQueued');
  if (phase === 'paused') return t('console.phasePaused');
  if (phase === 'remote_pushing') return t('console.phaseRemotePushing');
  if (phase === 'remote_deploying') return t('console.phaseRemoteDeploying');
  if (phase === 'failed') return t('console.phaseFailed');
  if (phase === 'cancelled') return t('console.phaseCancelled');
  if (phase === 'completed') return t('console.phaseCompleted');
  if (phase === 'interrupted') return t('console.phaseInterrupted');
  return t('console.phaseCopying');
}

function statusClass(phase: TaskRecordPhase): string {
  if (phase === 'queued') return 'bg-slate-100 text-slate-600';
  if (phase === 'paused') return 'bg-amber-50 text-amber-700';
  if (phase === 'remote_pushing') return 'bg-purple-50 text-purple-700';
  if (phase === 'remote_deploying') return 'bg-fuchsia-50 text-fuchsia-700';
  if (phase === 'failed') return 'bg-rose-50 text-rose-700';
  if (phase === 'cancelled') return 'bg-red-50 text-red-700';
  if (phase === 'completed') return 'bg-emerald-50 text-emerald-700';
  if (phase === 'interrupted') return 'bg-orange-50 text-orange-700';
  return 'bg-blue-50 text-blue-700';
}

function progressBarClass(phase: TaskRecordPhase): string {
  if (phase === 'queued') return 'bg-slate-400';
  if (phase === 'paused') return 'bg-amber-500';
  if (phase === 'remote_pushing') return 'bg-purple-500';
  if (phase === 'remote_deploying') return 'bg-fuchsia-500';
  if (phase === 'failed') return 'bg-rose-500';
  if (phase === 'cancelled') return 'bg-red-500';
  if (phase === 'completed') return 'bg-emerald-500';
  if (phase === 'interrupted') return 'bg-orange-500';
  return 'bg-blue-500';
}

function progressValue(rec: { phase: TaskRecordPhase; copyPercentage: number; deployPercentage: number; copyCompleted: boolean }): number {
  if (rec.phase === 'remote_pushing' || rec.phase === 'remote_deploying') {
    return rec.deployPercentage > 0 ? rec.deployPercentage : (rec.copyCompleted ? 100 : rec.copyPercentage);
  }
  return rec.copyPercentage;
}
</script>

<template>
  <div :class="embedded ? '' : 'bg-white rounded-xl border border-slate-200 shadow-sm'">
    <div v-if="showHeader" class="p-4 border-b border-slate-100 flex items-center justify-between">
      <h3 class="text-lg font-semibold text-slate-700">{{ t('console.taskRecords') }}</h3>
      <button
        v-if="orderedRecords.length"
        @click="clearRecords"
        class="text-slate-500 hover:text-red-600 px-2 py-1 rounded-md hover:bg-red-50 transition-colors flex items-center gap-1"
      >
        <Trash2 class="w-4 h-4" />
        {{ t('console.clearRecords') }}
      </button>
    </div>

    <div v-else-if="orderedRecords.length" class="pb-2 flex justify-end">
      <button
        @click="clearRecords"
        class="text-slate-500 hover:text-red-600 px-2 py-1 rounded-md hover:bg-red-50 transition-colors flex items-center gap-1 text-sm"
      >
        <Trash2 class="w-4 h-4" />
        {{ t('console.clearRecords') }}
      </button>
    </div>

    <div v-if="!orderedRecords.length" :class="embedded ? 'p-4 text-slate-400 text-center' : 'p-8 text-slate-400 text-center'">
      {{ t('console.noRecords') }}
    </div>

    <div v-else :class="embedded ? 'max-h-80 overflow-auto space-y-3 pr-1' : 'max-h-80 overflow-auto divide-y divide-slate-100'">
      <div
        v-for="rec in orderedRecords"
        :key="rec.id"
        :class="embedded ? 'p-4 rounded-lg bg-white border border-slate-200 shadow-sm' : 'p-4'"
      >
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0">
            <div class="text-xs text-slate-400 font-mono">{{ rec.startTime }}</div>
            <div class="font-semibold text-slate-800 truncate mt-1" :title="rec.folder">{{ rec.folder }}</div>
            <div class="text-xs text-slate-500 truncate mt-1 flex items-center gap-1" :title="rec.localPath">
              <Copy class="w-3 h-3" />
              {{ rec.localPath || '-' }}
            </div>
          </div>
          <span
            class="text-xs font-bold px-2 py-1 rounded-full inline-flex items-center gap-1"
            :class="statusClass(rec.phase)"
          >
            <CheckCircle2 v-if="rec.phase === 'completed'" class="w-3.5 h-3.5" />
            {{ statusText(rec.phase) }}
          </span>
        </div>

        <div class="mt-3">
          <div class="h-2 bg-slate-100 rounded-full overflow-hidden">
            <div
              class="h-full transition-all duration-300"
              :class="progressBarClass(rec.phase)"
              :style="{ width: `${progressValue(rec)}%` }"
            />
          </div>
          <div class="mt-2 text-xs text-slate-500 flex items-center justify-between font-mono">
            <span>{{ progressValue(rec).toFixed(1) }}%</span>
            <span>{{ formatRecordSpeed(rec.speed) }}</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
