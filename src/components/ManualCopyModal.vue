<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { X, Play, FolderOpen, ShieldCheck } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';
import { addLog, updateManualCopyForm, getManualCopyForm } from '@/lib/store';
import { getConfig, temporaryCopy, openDirectory, type AppConfig } from '@/lib/tauri';

defineOptions({ name: 'ManualCopyModal' });

interface Props {
  isOpen: boolean;
}

interface Emits {
  close: [];
  success: [];
}

defineProps<Props>();
defineEmits<Emits>();

const emit = defineEmits<Emits>();

const { t } = useI18n();

const sourcePath = ref('');
const targetRootPath = ref('');
const statusMsg = ref('');
const statusTone = ref<'info' | 'success' | 'error'>('info');
const isSubmitting = ref(false);
const config = ref<AppConfig | null>(null);
const isSelectingTarget = ref(false);

const canSubmit = computed(
  () => sourcePath.value.trim().length > 0 && targetRootPath.value.trim().length > 0 && !isSubmitting.value
);

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
  } catch (error) {
    statusTone.value = 'error';
    statusMsg.value = t('manualCopy.loadConfigFailed', { error: String(error) });
  }
}

function restoreFormData() {
  const saved = getManualCopyForm();
  sourcePath.value = saved.sourcePath;
  targetRootPath.value = saved.targetRootPath;
}

async function selectTargetDirectory() {
  isSelectingTarget.value = true;
  try {
    const selected = await openDirectory();
    if (selected) {
      targetRootPath.value = selected;
      updateManualCopyForm({ targetRootPath: selected });
    }
  } catch (error) {
    statusTone.value = 'error';
    statusMsg.value = t('manualCopy.failed', { error: String(error) });
  } finally {
    isSelectingTarget.value = false;
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

    // Save form data
    updateManualCopyForm({
      sourcePath: sourcePath.value.trim(),
      targetRootPath: targetRootPath.value.trim(),
    });

    // Emit success event and close after delay
    emit('success');
    setTimeout(() => {
      emit('close');
    }, 2000);
  } catch (error) {
    statusTone.value = 'error';
    statusMsg.value = t('manualCopy.failed', { error: String(error) });
  } finally {
    isSubmitting.value = false;
  }
}

function closeModal() {
  emit('close');
}

// Save form data on input
watch([sourcePath, targetRootPath], () => {
  updateManualCopyForm({
    sourcePath: sourcePath.value,
    targetRootPath: targetRootPath.value,
  });
});

onMounted(() => {
  loadConfig();
  restoreFormData();
});
</script>

<template>
  <Transition name="modal-fade">
    <div v-if="isOpen" class="fixed inset-0 z-50 flex items-center justify-center bg-black/50" @click="closeModal">
      <!-- Modal Container -->
      <div
        class="bg-white rounded-2xl shadow-xl max-w-2xl w-full mx-4 max-h-[90vh] overflow-y-auto"
        @click.stop
      >
        <!-- Modal Header -->
        <div class="sticky top-0 z-10 flex items-center justify-between border-b border-slate-200 bg-white px-6 py-4">
          <div>
            <h3 class="text-lg font-bold text-slate-800">{{ t('manualCopy.title') }}</h3>
            <p class="text-sm text-slate-500 mt-1">{{ t('manualCopy.subtitle') }}</p>
          </div>
          <button
            @click="closeModal"
            class="p-2 rounded-lg hover:bg-slate-100 transition-colors text-slate-600 hover:text-slate-800"
            :aria-label="t('settings.close')"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        <!-- Modal Content -->
        <div class="p-6 space-y-6">
          <!-- Form Section -->
          <div class="space-y-4">
            <!-- Source Path Input -->
            <div>
              <label class="block text-sm font-medium text-slate-700 mb-2">
                {{ t('manualCopy.sourcePath') }}
              </label>
              <input
                v-model="sourcePath"
                type="text"
                class="w-full p-3 border border-slate-300 rounded-xl focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all"
                :placeholder="t('manualCopy.sourcePlaceholder')"
              />
            </div>

            <!-- Target Path Input -->
            <div>
              <label class="block text-sm font-medium text-slate-700 mb-2">
                {{ t('manualCopy.targetRootPath') }}
              </label>
              <div class="flex gap-2">
                <input
                  v-model="targetRootPath"
                  type="text"
                  class="flex-1 p-3 border border-slate-300 rounded-xl focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all"
                  :placeholder="t('manualCopy.targetPlaceholder')"
                />
                <button
                  @click="selectTargetDirectory"
                  :disabled="isSelectingTarget"
                  class="px-4 py-3 rounded-xl border border-slate-300 bg-slate-50 hover:bg-slate-100 transition-colors disabled:opacity-60 disabled:cursor-not-allowed inline-flex items-center gap-2 text-slate-600 font-medium"
                >
                  <FolderOpen class="w-4 h-4" />
                  {{ t('settings.openFolder') }}
                </button>
              </div>
              <p class="text-xs text-slate-400 mt-2">{{ t('manualCopy.targetHint') }}</p>
            </div>
          </div>

          <!-- Status Message -->
          <div
            v-if="statusMsg"
            class="rounded-xl px-4 py-3 text-sm border"
            :class="
              statusTone === 'error'
                ? 'bg-red-50 text-red-600 border-red-100'
                : statusTone === 'success'
                  ? 'bg-emerald-50 text-emerald-600 border-emerald-100'
                  : 'bg-slate-50 text-slate-600 border-slate-200'
            "
          >
            {{ statusMsg }}
          </div>

          <!-- Rules Card -->
          <div class="bg-slate-50 rounded-xl border border-slate-200 p-5 space-y-3">
            <div class="flex items-center gap-2 text-slate-800 font-semibold">
              <ShieldCheck class="w-4 h-4 text-blue-600" />
              {{ t('manualCopy.ruleCard') }}
            </div>

            <div class="space-y-3 text-sm text-slate-600">
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
        </div>

        <!-- Modal Footer -->
        <div class="sticky bottom-0 z-10 border-t border-slate-200 bg-white px-6 py-4 flex items-center justify-end gap-3">
          <button
            @click="closeModal"
            class="px-5 py-2.5 rounded-xl border border-slate-300 bg-white text-slate-600 hover:bg-slate-50 transition-colors font-medium"
          >
            {{ t('settings.close') }}
          </button>
          <button
            @click="submitCopy"
            :disabled="!canSubmit"
            class="inline-flex items-center gap-2 px-5 py-2.5 rounded-xl text-white font-medium transition-colors disabled:opacity-60 disabled:cursor-not-allowed bg-blue-600 hover:bg-blue-700"
          >
            <Play class="w-4 h-4" />
            {{ isSubmitting ? t('manualCopy.copying') : t('manualCopy.startCopy') }}
          </button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.modal-fade-enter-active,
.modal-fade-leave-active {
  transition: opacity 0.3s ease;
}

.modal-fade-enter-from,
.modal-fade-leave-to {
  opacity: 0;
}

.modal-fade-enter-to,
.modal-fade-leave-from {
  opacity: 1;
}
</style>
