<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { AlertCircle, ArrowRight, Copy, Play, Settings, ShieldCheck } from 'lucide-vue-next';
import { addLog } from '@/lib/store';
import { addSystemEvent, getConfig, temporaryCopy, type AppConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';

defineOptions({ name: 'ManualCopyPage' });

const { t } = useI18n();
const router = useRouter();

const sourcePath = ref('');
const targetRootPath = ref('');
const statusMsg = ref('');
const statusTone = ref<'info' | 'success' | 'error'>('info');
const isSubmitting = ref(false);
const config = ref<AppConfig | null>(null);

const canSubmit = computed(() => sourcePath.value.trim().length > 0 && targetRootPath.value.trim().length > 0 && !isSubmitting.value);

const filterSummary = computed(() => {
  if (!config.value) return t('manualCopy.readingRules');

  const exts = config.value.file_extensions.filter(Boolean);
  const keywords = config.value.filename_includes.filter(Boolean);
  const parts: string[] = [];

  if (exts.length > 0) {
    parts.push(t('manualCopy.extFilter', { value: exts.join(', ') }));
  }
  if (keywords.length > 0) {
    parts.push(t('manualCopy.keywordFilter', { value: keywords.join(', ') }));
  }

  return parts.length > 0 ? parts.join(' | ') : t('manualCopy.noFilters');
});

const stabilitySummary = computed(() => {
  if (!config.value) return t('manualCopy.readingRules');
  return t('manualCopy.stabilityEnabled', {
    mins: config.value.recent_file_guard_mins,
    secs: config.value.stability_check_secs,
  });
});

async function loadConfig() {
  try {
    const cfg = await getConfig();
    config.value = cfg;
    if (!targetRootPath.value) {
      targetRootPath.value = cfg.local_path || '';
    }
  } catch (error) {
    statusTone.value = 'error';
    statusMsg.value = t('manualCopy.loadConfigFailed', { error: String(error) });
  }
}

async function submitCopy() {
  if (!canSubmit.value) {
    statusTone.value = 'error';
    statusMsg.value = t('manualCopy.fillRequired');
    return;
  }

  isSubmitting.value = true;
  statusMsg.value = '';
  statusTone.value = 'info';

  try {
    await temporaryCopy(sourcePath.value.trim(), targetRootPath.value.trim());
    statusTone.value = 'success';
    statusMsg.value = t('manualCopy.success');
    addLog(t('manualCopy.addedToQueue'), 'success');
    await addSystemEvent('MANUAL_COPY', t('manualCopy.addedToQueue'));
  } catch (error) {
    statusTone.value = 'error';
    statusMsg.value = t('manualCopy.failed', { error: String(error) });
  } finally {
    isSubmitting.value = false;
  }
}

onMounted(loadConfig);
</script>

<template>
  <div class="p-6 bg-slate-50 min-h-full space-y-6">
    <div class="flex items-start justify-between gap-4 flex-wrap">
      <div>
        <h2 class="text-2xl font-bold text-slate-800 flex items-center gap-3">
          <div class="w-10 h-10 rounded-xl bg-blue-100 text-blue-600 flex items-center justify-center">
            <Copy class="w-5 h-5" />
          </div>
          {{ t('manualCopy.title') }}
        </h2>
        <p class="text-sm text-slate-500 mt-2 max-w-3xl">
          {{ t('manualCopy.subtitle') }}
        </p>
      </div>

      <router-link
        to="/settings"
        class="inline-flex items-center gap-2 px-4 py-2 rounded-lg border border-slate-200 bg-white text-slate-600 hover:text-blue-600 hover:border-blue-200 transition-colors"
      >
        <Settings class="w-4 h-4" />
        {{ t('manualCopy.viewRules') }}
      </router-link>
    </div>

    <div class="grid grid-cols-1 xl:grid-cols-[minmax(0,1.2fr)_minmax(320px,0.8fr)] gap-6">
      <!-- Input Form -->
      <div class="bg-white rounded-2xl border border-slate-200 shadow-sm p-6 space-y-5">
        <div class="grid grid-cols-1 gap-5">
          <div>
            <label class="block text-sm font-medium text-slate-700 mb-2">{{ t('manualCopy.sourcePath') }}</label>
            <input
              v-model="sourcePath"
              type="text"
              class="w-full p-3 border border-slate-300 rounded-xl focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all"
              :placeholder="t('manualCopy.sourcePlaceholder')"
            />
          </div>

          <div>
            <label class="block text-sm font-medium text-slate-700 mb-2">{{ t('manualCopy.targetRootPath') }}</label>
            <input
              v-model="targetRootPath"
              type="text"
              class="w-full p-3 border border-slate-300 rounded-xl focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all"
              :placeholder="t('manualCopy.targetPlaceholder')"
            />
            <p class="text-xs text-slate-400 mt-2">{{ t('manualCopy.targetHint') }}</p>
          </div>
        </div>

        <div class="flex flex-wrap gap-3 pt-2">
          <button
            @click="submitCopy"
            :disabled="!canSubmit"
            class="inline-flex items-center gap-2 px-5 py-2.5 rounded-xl text-white font-medium transition-colors disabled:opacity-60 disabled:cursor-not-allowed bg-blue-600 hover:bg-blue-700"
          >
            <Play class="w-4 h-4" />
            {{ isSubmitting ? t('manualCopy.copying') : t('manualCopy.startCopy') }}
          </button>
        </div>

        <!-- Status message -->
        <div v-if="statusMsg" class="rounded-xl px-4 py-3 text-sm border" :class="statusTone === 'error' ? 'bg-red-50 text-red-600 border-red-100' : statusTone === 'success' ? 'bg-emerald-50 text-emerald-600 border-emerald-100' : 'bg-slate-50 text-slate-600 border-slate-200'">
          <div class="flex items-center justify-between gap-3">
            <span>{{ statusMsg }}</span>
            <button
              v-if="statusTone === 'success'"
              @click="router.push('/')"
              class="inline-flex items-center gap-1 text-xs font-medium text-emerald-700 hover:text-emerald-900 shrink-0"
            >
              {{ t('manualCopy.viewInConsole') }}
              <ArrowRight class="w-3 h-3" />
            </button>
          </div>
        </div>
      </div>

      <!-- Rules Card -->
      <div class="space-y-4">
        <div class="bg-white rounded-2xl border border-slate-200 shadow-sm p-5 space-y-4">
          <div class="flex items-center gap-2 text-slate-800 font-semibold">
            <ShieldCheck class="w-4 h-4 text-blue-600" />
            {{ t('manualCopy.ruleCard') }}
          </div>

          <div class="rounded-xl bg-slate-50 border border-slate-200 p-4 space-y-3 text-sm text-slate-600">
            <div>
              <div class="font-medium text-slate-700 mb-1">{{ t('manualCopy.filterTitle') }}</div>
              <div>{{ filterSummary }}</div>
            </div>
            <div>
              <div class="font-medium text-slate-700 mb-1">{{ t('manualCopy.stabilityTitle') }}</div>
              <div>{{ stabilitySummary }}</div>
            </div>
            <div>
              <div class="font-medium text-slate-700 mb-1">{{ t('manualCopy.modeTitle') }}</div>
              <div>{{ t('manualCopy.modeDesc') }}</div>
            </div>
          </div>
        </div>

        <!-- Hint card -->
        <div class="bg-white rounded-2xl border border-slate-200 shadow-sm p-5 space-y-3">
          <div class="flex items-center gap-2 text-slate-800 font-semibold">
            <AlertCircle class="w-4 h-4 text-amber-500" />
            {{ t('manualCopy.queueHintTitle') }}
          </div>
          <p class="text-sm text-slate-500">{{ t('manualCopy.queueHintDesc') }}</p>
          <button
            @click="router.push('/')"
            class="inline-flex items-center gap-2 text-sm text-blue-600 hover:text-blue-800 font-medium"
          >
            {{ t('manualCopy.goToConsole') }}
            <ArrowRight class="w-3.5 h-3.5" />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
