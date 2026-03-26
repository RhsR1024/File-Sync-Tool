<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { ArrowRight, Copy, Settings, ShieldCheck, AlertCircle } from 'lucide-vue-next';
import ManualCopyModal from '@/components/ManualCopyModal.vue';
import { getConfig, type AppConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';
import { addLog } from '@/lib/store';

defineOptions({ name: 'ManualCopyPage' });

const { t } = useI18n();
const router = useRouter();

const isModalOpen = ref(false);
const config = ref<AppConfig | null>(null);
const isLoadingConfig = ref(false);
const configError = ref('');

const filterSummary = computed(() => {
  if (!config.value) return t('manualCopy.readingRules');

  const exts = config.value.file_extensions.filter(Boolean);
  const keywords = config.value.filename_includes.filter(Boolean);

  if (exts.length === 0 && keywords.length === 0) {
    return t('manualCopy.noGlobalFilters');
  }

  const parts: string[] = [];
  if (exts.length > 0) {
    parts.push(t('manualCopy.extFilter', { value: exts.join(', ') }));
  }
  if (keywords.length > 0) {
    parts.push(t('manualCopy.keywordFilter', { value: keywords.join(', ') }));
  }
  return parts.join(' | ');
});

const stabilitySummary = computed(() => {
  if (!config.value) return t('manualCopy.readingRules');
  return t('manualCopy.stabilityEnabled', {
    mins: config.value.recent_file_guard_mins,
    secs: config.value.stability_check_secs,
  });
});

async function loadConfig(): Promise<void> {
  isLoadingConfig.value = true;
  configError.value = '';
  try {
    const cfg = await getConfig();
    config.value = cfg;
  } catch (error) {
    const errorMsg = error instanceof Error ? error.message : String(error);
    configError.value = t('manualCopy.loadConfigFailed', { error: errorMsg });
    addLog(`Failed to load config: ${errorMsg}`, 'error');
    console.error('Failed to load config:', error);
  } finally {
    isLoadingConfig.value = false;
  }
}

function openManualCopyModal(): void {
  isModalOpen.value = true;
}

function closeManualCopyModal(): void {
  isModalOpen.value = false;
}

function handleCopySuccess(): void {
  // Optional: Any additional logic after successful copy
  // e.g., show a toast, refresh data, etc.
}

onMounted(loadConfig);
</script>

<template>
  <div class="p-6 bg-slate-50 min-h-full space-y-6">
    <!-- Page Header -->
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

    <!-- Main Content Area -->
    <div class="grid grid-cols-1 xl:grid-cols-[minmax(0,1.2fr)_minmax(320px,0.8fr)] gap-6">
      <!-- Start Copy Section -->
      <div class="bg-white rounded-2xl border border-slate-200 shadow-sm p-8 space-y-6 flex flex-col items-start justify-center min-h-80">
        <div>
          <p class="text-slate-600 mb-4">{{ t('manualCopy.subtitle') }}</p>
          <button
            @click="openManualCopyModal"
            class="inline-flex items-center gap-3 px-8 py-4 rounded-xl text-white font-semibold text-lg transition-all bg-blue-600 hover:bg-blue-700 active:scale-95 shadow-md hover:shadow-lg"
          >
            <Copy class="w-6 h-6" />
            {{ t('manualCopy.startCopy') }}
          </button>
        </div>
        <div class="text-sm text-slate-500 mt-4">
          {{ t('manualCopy.modalTip') }}
        </div>
      </div>

      <!-- Right Sidebar -->
      <div class="space-y-4">
        <!-- Error Alert -->
        <div v-if="configError" class="bg-red-50 rounded-2xl border border-red-200 shadow-sm p-5">
          <div class="flex items-start gap-3">
            <AlertCircle class="w-5 h-5 text-red-600 flex-shrink-0 mt-0.5" />
            <div>
              <div class="font-semibold text-red-800">{{ t('common.error') }}</div>
              <p class="text-sm text-red-700 mt-1">{{ configError }}</p>
            </div>
          </div>
        </div>

        <!-- Rules Card -->
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

        <!-- Progress Tracking Card -->
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

    <!-- Manual Copy Modal -->
    <ManualCopyModal
      :is-open="isModalOpen"
      @close="closeManualCopyModal"
      @success="handleCopySuccess"
    />
  </div>
</template>
