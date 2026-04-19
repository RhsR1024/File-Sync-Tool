<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { X, Play, FolderOpen, ShieldCheck, AlertTriangle, RefreshCw, FilePlus2 } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';
import { updateManualCopyForm, getManualCopyForm } from '@/lib/store';
import {
  getConfig,
  previewTemporaryCopy,
  queueTemporaryCopy,
  openDirectory,
  type AppConfig,
  type ManualCopyPreview,
} from '@/lib/tauri';

defineOptions({ name: 'ManualCopyModal' });

interface Props {
  isOpen: boolean;
}

interface Emits {
  close: [];
  success: [];
}

const props = defineProps<Props>();

const emit = defineEmits<Emits>();

const { t } = useI18n();

const sourcePath = ref('');
const targetRootPath = ref('');
const statusMsg = ref('');
const statusTone = ref<'info' | 'success' | 'error'>('info');
const isSubmitting = ref(false);
const config = ref<AppConfig | null>(null);
const isSelectingTarget = ref(false);
const sourceInputRef = ref<HTMLInputElement | null>(null);
const existingTargetPreview = ref<ManualCopyPreview | null>(null);
const pendingSubmitRequest = ref<{ source: string; target: string } | null>(null);

// Filter selections: user picks which global extensions/keywords to apply (default: none selected = copy all)
const selectedExtensions = ref<string[]>([]);
const selectedKeywords = ref<string[]>([]);

const canSubmit = computed(
  () =>
    sourcePath.value.trim().length > 0
    && targetRootPath.value.trim().length > 0
    && !isSubmitting.value
    && !existingTargetPreview.value
);

const globalExtensions = computed(() => config.value?.file_extensions.filter(Boolean) ?? []);
const globalKeywords = computed(() => config.value?.filename_includes.filter(Boolean) ?? []);
const hasAnyGlobalFilter = computed(() => globalExtensions.value.length > 0 || globalKeywords.value.length > 0);

function toggleExtension(ext: string) {
  const idx = selectedExtensions.value.indexOf(ext);
  if (idx >= 0) selectedExtensions.value.splice(idx, 1);
  else selectedExtensions.value.push(ext);
}

function toggleKeyword(kw: string) {
  const idx = selectedKeywords.value.indexOf(kw);
  if (idx >= 0) selectedKeywords.value.splice(idx, 1);
  else selectedKeywords.value.push(kw);
}

const filterSummary = computed(() => {
  const parts: string[] = [];
  if (selectedExtensions.value.length > 0) {
    parts.push(t('manualCopy.extFilter', { value: selectedExtensions.value.join(', ') }));
  }
  if (selectedKeywords.value.length > 0) {
    parts.push(t('manualCopy.keywordFilter', { value: selectedKeywords.value.join(', ') }));
  }
  return parts.length > 0 ? parts.join(' | ') : t('manualCopy.noFiltersActive');
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
    // If target root is still empty after restoring saved form, default to config local_path
    if (!targetRootPath.value.trim() && cfg.local_path) {
      targetRootPath.value = cfg.local_path;
    }
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

async function focusSourceInput() {
  await nextTick();
  sourceInputRef.value?.focus();
}

function formatManualCopyError(error: unknown): string {
  const raw = String(error).replace(/^Error:\s*/, '');
  const [code, detail = ''] = raw.split('::', 2);
  const path = detail.trim();

  if (code === 'SOURCE_PATH_REQUIRED' || code === 'TARGET_ROOT_REQUIRED') {
    return t('manualCopy.fillRequired');
  }
  if (code === 'SOURCE_NOT_FOUND') {
    return t('manualCopy.sourceNotFound', { path });
  }
  if (code === 'INVALID_SOURCE_TYPE') {
    return t('manualCopy.invalidSourceType', { path });
  }
  if (code === 'TARGET_ROOT_NOT_FOUND') {
    return t('manualCopy.targetNotFound', { path });
  }
  if (code === 'TARGET_ROOT_NOT_DIRECTORY') {
    return t('manualCopy.targetNotDirectory', { path });
  }
  if (code === 'TARGET_INSIDE_SOURCE') {
    return t('manualCopy.targetInsideSource', { path });
  }
  if (code === 'TARGET_SAME_AS_SOURCE') {
    return t('manualCopy.targetSameAsSource', { path });
  }
  if (code === 'TARGET_FILE_CONFLICTS_WITH_DIRECTORY') {
    return t('manualCopy.targetFileConflictsWithDirectory', { path });
  }
  if (code === 'TARGET_DIRECTORY_CONFLICTS_WITH_FILE') {
    return t('manualCopy.targetDirectoryConflictsWithFile', { path });
  }
  if (code === 'DUPLICATE_TASK') {
    return t('manualCopy.duplicateTask', { detail: path || raw.replace(/^DUPLICATE_TASK::?/, '').trim() });
  }

  return t('manualCopy.failed', { error: raw });
}

function clearExistingTargetDecision() {
  existingTargetPreview.value = null;
  pendingSubmitRequest.value = null;
}

function existingTargetSummary(preview: ManualCopyPreview): string {
  if (preview.source_kind === 'file') {
    return t('manualCopy.targetExistsFileDecision', { path: preview.resolved_target_path });
  }
  return t('manualCopy.targetExistsDirectoryDecision', { path: preview.resolved_target_path });
}

function overwriteActionHint(preview: ManualCopyPreview): string {
  if (preview.source_kind === 'file') {
    return t('manualCopy.overwriteFileHint');
  }
  return t('manualCopy.overwriteDirectoryHint');
}

function skipActionHint(preview: ManualCopyPreview): string {
  if (preview.source_kind === 'file') {
    return t('manualCopy.skipFileHint');
  }
  return t('manualCopy.skipDirectoryHint');
}

async function enqueueCopy(source: string, target: string, overwriteExisting: boolean) {
  const exts = [...selectedExtensions.value];
  const kws = [...selectedKeywords.value];
  const ack = await queueTemporaryCopy(source, target, overwriteExisting, exts, kws);

  statusTone.value = 'success';
  statusMsg.value = ack.queued_ahead > 0
    ? t('manualCopy.addedToQueueWithAhead', { count: ack.queued_ahead })
    : t('manualCopy.addedToQueue');

  updateManualCopyForm({
    sourcePath: '',
    targetRootPath: target,
  });

  sourcePath.value = '';
  emit('success');
  await focusSourceInput();
}

async function confirmExistingTarget(overwriteExisting: boolean) {
  if (!pendingSubmitRequest.value) return;

  isSubmitting.value = true;
  statusMsg.value = '';
  statusTone.value = 'info';

  try {
    await enqueueCopy(
      pendingSubmitRequest.value.source,
      pendingSubmitRequest.value.target,
      overwriteExisting,
    );
  } catch (error) {
    statusTone.value = 'error';
    statusMsg.value = formatManualCopyError(error);
  } finally {
    clearExistingTargetDecision();
    isSubmitting.value = false;
  }
}

function cancelExistingTargetDecision() {
  clearExistingTargetDecision();
  statusTone.value = 'info';
  statusMsg.value = t('manualCopy.submitCancelled');
}

async function selectTargetDirectory() {
  isSelectingTarget.value = true;
  try {
    const selected = await openDirectory();
    if (selected) {
      targetRootPath.value = selected;
      updateManualCopyForm({ targetRootPath: selected });
      statusMsg.value = '';
    } else {
      // User cancelled directory selection
      statusTone.value = 'info';
      statusMsg.value = t('manualCopy.directorySelectionCancelled');
    }
  } catch (error) {
    statusTone.value = 'error';
    statusMsg.value = t('manualCopy.selectDirFailed', { error: String(error) });
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
    const source = sourcePath.value.trim();
    const target = targetRootPath.value.trim();
    const preview = await previewTemporaryCopy(source, target);

    if (preview.target_exists) {
      existingTargetPreview.value = preview;
      pendingSubmitRequest.value = { source, target };
      return;
    }

    await enqueueCopy(source, target, false);
  } catch (error) {
    statusTone.value = 'error';
    statusMsg.value = formatManualCopyError(error);
  } finally {
    isSubmitting.value = false;
  }
}

function closeModal() {
  clearExistingTargetDecision();
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
  restoreFormData();
  loadConfig();
});

watch(() => props.isOpen, (open) => {
  if (open) {
    isSubmitting.value = false;
    statusMsg.value = '';
    statusTone.value = 'info';
    clearExistingTargetDecision();
    loadConfig();
    focusSourceInput();
  }
});
</script>

<template>
  <Transition name="modal-fade">
    <div v-if="isOpen" class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <!-- Modal Container -->
      <div
        class="relative bg-white rounded-2xl shadow-xl max-w-2xl w-full mx-4 max-h-[90vh] overflow-y-auto"
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
                ref="sourceInputRef"
                v-model="sourcePath"
                type="text"
                :disabled="isSubmitting || Boolean(existingTargetPreview)"
                class="w-full p-3 border border-slate-300 rounded-xl focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all disabled:cursor-not-allowed disabled:bg-slate-100"
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
                  :disabled="isSubmitting || Boolean(existingTargetPreview)"
                  class="flex-1 p-3 border border-slate-300 rounded-xl focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all disabled:cursor-not-allowed disabled:bg-slate-100"
                  :placeholder="t('manualCopy.targetPlaceholder')"
                />
                <button
                  @click="selectTargetDirectory"
                  :disabled="isSelectingTarget || isSubmitting || Boolean(existingTargetPreview)"
                  :title="t('manualCopy.browseFolder')"
                  class="px-4 py-3 rounded-xl border border-slate-300 bg-slate-50 hover:bg-slate-100 transition-colors disabled:opacity-60 disabled:cursor-not-allowed inline-flex items-center gap-2 text-slate-600 font-medium"
                >
                  <FolderOpen class="w-4 h-4" />
                  <span class="hidden sm:inline">{{ t('manualCopy.browse') }}</span>
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

          <!-- Filter Rules Card -->
          <div class="bg-slate-50 rounded-xl border border-slate-200 p-5 space-y-4">
            <div class="flex items-center gap-2 text-slate-800 font-semibold">
              <ShieldCheck class="w-4 h-4 text-blue-600" />
              {{ t('manualCopy.filterTitle') }}
            </div>

            <div v-if="hasAnyGlobalFilter" class="space-y-3 text-sm">
              <p class="text-slate-500">{{ t('manualCopy.filterPickHint') }}</p>

              <!-- Extension checkboxes -->
              <div v-if="globalExtensions.length > 0">
                <div class="font-medium text-slate-700 mb-2">{{ t('manualCopy.extFilterLabel') }}</div>
                <div class="flex flex-wrap gap-2">
                  <label
                    v-for="ext in globalExtensions"
                    :key="'ext-' + ext"
                    class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border cursor-pointer transition-colors select-none text-sm"
                    :class="selectedExtensions.includes(ext)
                      ? 'bg-blue-50 border-blue-300 text-blue-700'
                      : 'bg-white border-slate-200 text-slate-600 hover:border-slate-300'"
                  >
                    <input
                      type="checkbox"
                      :checked="selectedExtensions.includes(ext)"
                      class="sr-only"
                      @change="toggleExtension(ext)"
                    />
                    <span
                      class="w-4 h-4 rounded border flex items-center justify-center text-xs shrink-0"
                      :class="selectedExtensions.includes(ext)
                        ? 'bg-blue-600 border-blue-600 text-white'
                        : 'border-slate-300 bg-white'"
                    >
                      <svg v-if="selectedExtensions.includes(ext)" class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="3" d="M5 13l4 4L19 7" /></svg>
                    </span>
                    {{ ext }}
                  </label>
                </div>
              </div>

              <!-- Keyword checkboxes -->
              <div v-if="globalKeywords.length > 0">
                <div class="font-medium text-slate-700 mb-2">{{ t('manualCopy.keywordFilterLabel') }}</div>
                <div class="flex flex-wrap gap-2">
                  <label
                    v-for="kw in globalKeywords"
                    :key="'kw-' + kw"
                    class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border cursor-pointer transition-colors select-none text-sm"
                    :class="selectedKeywords.includes(kw)
                      ? 'bg-purple-50 border-purple-300 text-purple-700'
                      : 'bg-white border-slate-200 text-slate-600 hover:border-slate-300'"
                  >
                    <input
                      type="checkbox"
                      :checked="selectedKeywords.includes(kw)"
                      class="sr-only"
                      @change="toggleKeyword(kw)"
                    />
                    <span
                      class="w-4 h-4 rounded border flex items-center justify-center text-xs shrink-0"
                      :class="selectedKeywords.includes(kw)
                        ? 'bg-purple-600 border-purple-600 text-white'
                        : 'border-slate-300 bg-white'"
                    >
                      <svg v-if="selectedKeywords.includes(kw)" class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="3" d="M5 13l4 4L19 7" /></svg>
                    </span>
                    {{ kw }}
                  </label>
                </div>
              </div>

              <!-- Active filter summary -->
              <div class="rounded-lg bg-white border border-slate-200 px-3 py-2 text-xs text-slate-500">
                {{ filterSummary }}
              </div>
            </div>

            <div v-else class="text-sm text-slate-500">
              {{ t('manualCopy.noGlobalFilters') }}
            </div>
          </div>

          <!-- Other Rules Card -->
          <div class="bg-slate-50 rounded-xl border border-slate-200 p-5 space-y-3">
            <div class="space-y-3 text-sm text-slate-600">
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
            {{ isSubmitting ? t('manualCopy.submitting') : t('manualCopy.startCopy') }}
          </button>
        </div>

        <!-- Conflict resolution overlay — fixed so it always covers viewport regardless of modal scroll -->
        <Teleport to="body">
          <Transition name="confirm-fade">
            <div
              v-if="existingTargetPreview"
              class="fixed inset-0 z-[70] flex items-center justify-center bg-black/50 backdrop-blur-sm px-4"
              @click="!isSubmitting && cancelExistingTargetDecision()"
            >
              <div
                class="w-full max-w-xl rounded-2xl border border-slate-200 bg-white shadow-2xl shadow-slate-400/20 overflow-hidden"
                @click.stop
              >
                <!-- Dialog header -->
                <div class="flex items-start gap-4 px-6 pt-6 pb-5 border-b border-slate-100">
                  <div class="flex-shrink-0 w-10 h-10 rounded-xl bg-amber-50 border border-amber-200 flex items-center justify-center">
                    <AlertTriangle class="w-5 h-5 text-amber-600" />
                  </div>
                  <div class="min-w-0 pt-0.5">
                    <div class="text-base font-semibold text-slate-800">
                      {{ t('manualCopy.targetExistsDecisionTitle') }}
                    </div>
                    <p class="text-sm leading-6 text-slate-500 mt-1.5 break-all">
                      {{ existingTargetSummary(existingTargetPreview) }}
                    </p>
                  </div>
                </div>

                <!-- Option cards -->
                <div class="p-4 space-y-2.5">
                  <!-- Overwrite -->
                  <button
                    @click="confirmExistingTarget(true)"
                    :disabled="isSubmitting"
                    class="w-full flex items-start gap-3.5 rounded-xl border border-blue-200 bg-blue-50 px-4 py-3.5 text-left transition-all hover:border-blue-300 hover:bg-blue-100 hover:shadow-sm active:scale-[0.99] disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    <div class="flex-shrink-0 w-8 h-8 rounded-lg bg-blue-600 flex items-center justify-center mt-0.5">
                      <RefreshCw class="w-3.5 h-3.5 text-white" />
                    </div>
                    <div class="min-w-0 flex-1">
                      <div class="text-sm font-semibold text-blue-800">
                        {{ t('manualCopy.overwriteAndQueue') }}
                      </div>
                      <div class="text-xs text-slate-600 mt-1 leading-5 break-words">
                        {{ overwriteActionHint(existingTargetPreview) }}
                      </div>
                    </div>
                  </button>

                  <!-- Copy new files only -->
                  <button
                    @click="confirmExistingTarget(false)"
                    :disabled="isSubmitting"
                    class="w-full flex items-start gap-3.5 rounded-xl border border-emerald-200 bg-emerald-50 px-4 py-3.5 text-left transition-all hover:border-emerald-300 hover:bg-emerald-100 hover:shadow-sm active:scale-[0.99] disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    <div class="flex-shrink-0 w-8 h-8 rounded-lg bg-emerald-600 flex items-center justify-center mt-0.5">
                      <FilePlus2 class="w-3.5 h-3.5 text-white" />
                    </div>
                    <div class="min-w-0 flex-1">
                      <div class="text-sm font-semibold text-emerald-800">
                        {{ t('manualCopy.skipAndQueue') }}
                      </div>
                      <div class="text-xs text-slate-600 mt-1 leading-5 break-words">
                        {{ skipActionHint(existingTargetPreview) }}
                      </div>
                    </div>
                  </button>
                </div>

                <!-- Footer cancel -->
                <div class="px-4 pb-4 flex justify-end border-t border-slate-100 pt-3">
                  <button
                    @click="cancelExistingTargetDecision"
                    :disabled="isSubmitting"
                    class="px-4 py-2 rounded-lg border border-slate-200 bg-white text-sm font-medium text-slate-600 hover:bg-slate-50 hover:border-slate-300 transition-colors disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    {{ t('manualCopy.cancelConflictDecision') }}
                  </button>
                </div>
              </div>
            </div>
          </Transition>
        </Teleport>
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

.confirm-fade-enter-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}
.confirm-fade-leave-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}
.confirm-fade-enter-from,
.confirm-fade-leave-to {
  opacity: 0;
  transform: scale(0.97);
}
.confirm-fade-enter-to,
.confirm-fade-leave-from {
  opacity: 1;
  transform: scale(1);
}
</style>
