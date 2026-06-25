<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { X, Play, FolderOpen, ShieldCheck, AlertTriangle, RefreshCw, FilePlus2, Info, Loader2, Clock, Zap } from 'lucide-vue-next';
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
import LoadingSkeleton from '@/components/LoadingSkeleton.vue';
import { pushToast, type ToastTone } from '@/composables/useToast';
import { resolveBatchTargets, type BatchEntryResolution } from '@/lib/manualCopyBatch';

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
// Inline error message for in-form validation feedback. Rendered below the
// inputs so users see exactly which field/value the backend rejected without
// hunting for a toast.
const inlineError = ref('');
const isSubmitting = ref(false);
const isLoadingConfig = ref(false);
const config = ref<AppConfig | null>(null);
const isSelectingTarget = ref(false);
const sourceInputRef = ref<HTMLInputElement | HTMLTextAreaElement | null>(null);
const modalRef = ref<HTMLElement | null>(null);
const existingTargetPreview = ref<ManualCopyPreview | null>(null);
const pendingSubmitRequest = ref<{ source: string; target: string; skipStability: boolean } | null>(null);

// --- Recently-modified ("just generated") confirmation prompt ---
// When the source file was modified within the stability guard window, the
// backend would otherwise hold it in a stability wait. We surface a 10s
// countdown prompt so the user can choose to copy immediately. No choice (or
// timeout) defaults to waiting (the safe behavior).
const RECENCY_PROMPT_SECONDS = 10;
const recencyPrompt = ref<{ secsAgo: number } | null>(null);
const recencyCountdown = ref(RECENCY_PROMPT_SECONDS);
let recencyResolve: ((choice: 'immediate' | 'wait') => void) | null = null;
let recencyTimer: ReturnType<typeof setInterval> | null = null;

// Filter selections: user picks which global extensions/keywords to apply (default: none selected = copy all)
const selectedExtensions = ref<string[]>([]);
const selectedKeywords = ref<string[]>([]);

// --- Batch mode state ---
// sourceLines: each trimmed non-empty line of the textarea = one batch entry.
const sourceLines = computed(() =>
  sourcePath.value.split(/\r?\n/).map((s) => s.trim()).filter(Boolean),
);
const isBatchMode = computed(() => sourceLines.value.length >= 2);

const batchResolutions = ref<BatchEntryResolution[]>([]);
type BatchPreviewStatus =
  | 'ok'
  | 'target_exists'
  | 'source_missing'
  | 'duplicate_in_batch'
  | 'invalid_path';
const batchRowPreview = ref<Map<string, { status: BatchPreviewStatus; finalTarget: string; errored?: boolean }>>(new Map());
const batchRowChecked = ref<Map<string, boolean>>(new Map());
const batchPreviewOpen = ref(false);
const batchSubmitting = ref(false);

// Tracks the focused element before the modal opens so we can return focus to
// it when the modal closes (a11y: keyboard users land back on the trigger).
let previouslyFocused: HTMLElement | null = null;

// Pushes a toast through the M01 shared queue.
function notify(message: string, tone: ToastTone = 'info', ttlMs?: number) {
  pushToast(message, tone, ttlMs !== undefined ? { ttlMs } : undefined);
}

const canSubmit = computed(
  () =>
    !isBatchMode.value
    && sourcePath.value.trim().length > 0
    && targetRootPath.value.trim().length > 0
    && !isSubmitting.value
    && !existingTargetPreview.value,
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
  isLoadingConfig.value = true;
  try {
    const cfg = await getConfig();
    config.value = cfg;
    // If target root is still empty after restoring saved form, default to config local_path
    if (!targetRootPath.value.trim() && cfg.local_path) {
      targetRootPath.value = cfg.local_path;
    }
  } catch (error) {
    notify(t('manualCopy.loadConfigFailed', { error: String(error) }), 'error');
  } finally {
    isLoadingConfig.value = false;
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

// True when the source is a freshly-modified single file that the backend would
// hold in the stability wait. Mirrors the backend condition: stability check
// enabled AND modified within recent_file_guard_mins.
function isRecentSource(preview: ManualCopyPreview): boolean {
  if (preview.source_kind !== 'file') return false;
  const secs = preview.source_modified_secs_ago;
  if (secs === null || secs === undefined) return false;
  const cfg = config.value;
  if (!cfg || cfg.stability_check_secs <= 0) return false;
  return secs < cfg.recent_file_guard_mins * 60;
}

// Shows the countdown prompt and resolves with the user's choice. Resolves to
// 'wait' automatically after RECENCY_PROMPT_SECONDS or if dismissed.
function confirmRecency(secsAgo: number): Promise<'immediate' | 'wait'> {
  return new Promise((resolve) => {
    recencyResolve = resolve;
    recencyPrompt.value = { secsAgo };
    recencyCountdown.value = RECENCY_PROMPT_SECONDS;
    recencyTimer = setInterval(() => {
      recencyCountdown.value -= 1;
      if (recencyCountdown.value <= 0) {
        resolveRecency('wait');
      }
    }, 1000);
  });
}

function resolveRecency(choice: 'immediate' | 'wait') {
  if (recencyTimer !== null) {
    clearInterval(recencyTimer);
    recencyTimer = null;
  }
  recencyPrompt.value = null;
  const resolve = recencyResolve;
  recencyResolve = null;
  resolve?.(choice);
}

// Human-readable "modified N seconds/minutes ago" for the prompt.
function formatModifiedAgo(secsAgo: number): string {
  if (secsAgo < 60) {
    return t('manualCopy.recency.secsAgo', { secs: secsAgo });
  }
  return t('manualCopy.recency.minsAgo', { mins: Math.floor(secsAgo / 60) });
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

async function enqueueCopy(
  source: string,
  target: string,
  overwriteExisting: boolean,
  skipStability = false,
) {
  const exts = [...selectedExtensions.value];
  const kws = [...selectedKeywords.value];
  const ack = await queueTemporaryCopy(source, target, overwriteExisting, exts, kws, skipStability);

  notify(
    ack.queued_ahead > 0
      ? t('manualCopy.addedToQueueWithAhead', { count: ack.queued_ahead })
      : t('manualCopy.addedToQueue'),
    'success',
  );

  updateManualCopyForm({
    sourcePath: '',
    targetRootPath: target,
  });

  sourcePath.value = '';
  inlineError.value = '';
  emit('success');
  await focusSourceInput();
}

async function confirmExistingTarget(overwriteExisting: boolean) {
  if (!pendingSubmitRequest.value) return;

  isSubmitting.value = true;
  inlineError.value = '';

  try {
    await enqueueCopy(
      pendingSubmitRequest.value.source,
      pendingSubmitRequest.value.target,
      overwriteExisting,
      pendingSubmitRequest.value.skipStability,
    );
  } catch (error) {
    inlineError.value = formatManualCopyError(error);
    notify(inlineError.value, 'error');
  } finally {
    clearExistingTargetDecision();
    isSubmitting.value = false;
  }
}

function cancelExistingTargetDecision() {
  clearExistingTargetDecision();
  notify(t('manualCopy.submitCancelled'), 'info');
}

async function selectTargetDirectory() {
  isSelectingTarget.value = true;
  try {
    const selected = await openDirectory();
    if (selected) {
      targetRootPath.value = selected;
      updateManualCopyForm({ targetRootPath: selected });
      inlineError.value = '';
    } else {
      // User cancelled directory selection
      notify(t('manualCopy.directorySelectionCancelled'), 'info');
    }
  } catch (error) {
    inlineError.value = t('manualCopy.selectDirFailed', { error: String(error) });
    notify(inlineError.value, 'error');
  } finally {
    isSelectingTarget.value = false;
  }
}

async function previewBatch() {
  inlineError.value = '';
  batchSubmitting.value = false;

  const target = targetRootPath.value.trim();
  if (!target) {
    inlineError.value = t('manualCopy.fillRequired');
    return;
  }

  const sources = sourceLines.value;
  if (sources.length === 0) {
    inlineError.value = t('manualCopy.fillRequired');
    return;
  }

  const resolutions = resolveBatchTargets(sources, target);
  batchResolutions.value = resolutions;

  const previewMap = new Map<string, { status: BatchPreviewStatus; finalTarget: string; errored?: boolean }>();
  const checkedMap = new Map<string, boolean>();

  await Promise.all(
    resolutions.map(async (r) => {
      if (r.status === 'invalid_path') {
        previewMap.set(r.rawSource, { status: 'invalid_path', finalTarget: '' });
        checkedMap.set(r.rawSource, false);
        return;
      }
      if (r.status === 'duplicate_in_batch') {
        previewMap.set(r.rawSource, { status: 'duplicate_in_batch', finalTarget: r.finalTarget });
        checkedMap.set(r.rawSource, false);
        return;
      }
      try {
        const preview = await previewTemporaryCopy(r.rawSource, r.effectiveTargetRoot);
        const status: BatchPreviewStatus = preview.target_exists ? 'target_exists' : 'ok';
        previewMap.set(r.rawSource, { status, finalTarget: preview.resolved_target_path });
        checkedMap.set(r.rawSource, status === 'ok');
      } catch (error) {
        previewMap.set(r.rawSource, { status: 'source_missing', finalTarget: r.finalTarget, errored: true });
        checkedMap.set(r.rawSource, false);
      }
    }),
  );

  batchRowPreview.value = previewMap;
  batchRowChecked.value = checkedMap;
  batchPreviewOpen.value = true;
}

async function submitBatch() {
  if (!batchPreviewOpen.value || checkedBatchCount.value === 0) return;
  inlineError.value = '';
  batchSubmitting.value = true;
  const exts = [...selectedExtensions.value];
  const kws = [...selectedKeywords.value];

  const ordered = batchResolutions.value.filter(
    (r) => batchRowChecked.value.get(r.rawSource) === true,
  );

  const total = ordered.length;
  let ok = 0;
  const failedRows: string[] = [];

  for (const r of ordered) {
    const preview = batchRowPreview.value.get(r.rawSource);
    const overwrite = preview?.status === 'target_exists';
    try {
      await queueTemporaryCopy(r.rawSource, r.effectiveTargetRoot, overwrite, exts, kws);
      ok++;
    } catch (error) {
      failedRows.push(r.rawSource);
      const nextPreview = new Map(batchRowPreview.value);
      nextPreview.set(r.rawSource, {
        status: 'source_missing',
        finalTarget: r.finalTarget,
        errored: true,
      });
      batchRowPreview.value = nextPreview;
      // Log to console only; toast summary is pushed below.
      console.warn('queueTemporaryCopy failed for', r.rawSource, error);
    }
  }

  batchSubmitting.value = false;

  if (failedRows.length === 0) {
    notify(t('manualCopy.batch.toastSuccessAll', { count: ok }), 'success');
    // Reset and close like the single-source success flow.
    sourcePath.value = '';
    batchPreviewOpen.value = false;
    batchResolutions.value = [];
    batchRowPreview.value = new Map();
    batchRowChecked.value = new Map();
    emit('success');
    emit('close');
  } else {
    notify(
      t('manualCopy.batch.toastPartial', { ok, total }),
      'error',
    );
    // Keep the modal open; failed rows remain visibly red for the user.
  }
}

function backToBatchEdit() {
  batchPreviewOpen.value = false;
}

function toggleAllBatchRows(checked: boolean) {
  const next = new Map<string, boolean>();
  for (const r of batchResolutions.value) {
    if (r.status === 'invalid_path' || r.status === 'duplicate_in_batch') {
      next.set(r.rawSource, false);
      continue;
    }
    next.set(r.rawSource, checked);
  }
  batchRowChecked.value = next;
}

function toggleBatchRow(rawSource: string) {
  const next = new Map(batchRowChecked.value);
  next.set(rawSource, !next.get(rawSource));
  batchRowChecked.value = next;
}

const checkedBatchCount = computed(() => {
  let n = 0;
  for (const checked of batchRowChecked.value.values()) if (checked) n++;
  return n;
});

const allBatchRowsChecked = computed(() => {
  // Selectable = anything except invalid_path (invalid rows have no
  // resolvable target so they cannot be enqueued and stay disabled).
  const selectable = batchResolutions.value.filter((r) => r.status !== 'invalid_path');
  if (selectable.length === 0) return false;
  return selectable.every((r) => batchRowChecked.value.get(r.rawSource) === true);
});

function batchStatusLabel(status: BatchPreviewStatus): string {
  if (status === 'ok') return t('manualCopy.batch.statusOk');
  if (status === 'target_exists') return t('manualCopy.batch.statusTargetExists');
  if (status === 'source_missing') return t('manualCopy.batch.statusSourceMissing');
  if (status === 'duplicate_in_batch') return t('manualCopy.batch.statusDuplicateInBatch');
  return t('manualCopy.batch.statusInvalidPath');
}

function batchStatusClass(status: BatchPreviewStatus): string {
  if (status === 'ok') return 'bg-emerald-100 text-emerald-700 border-emerald-200';
  if (status === 'target_exists') return 'bg-amber-100 text-amber-700 border-amber-200';
  return 'bg-red-100 text-red-700 border-red-200';
}

async function submitCopy() {
  if (!canSubmit.value) {
    inlineError.value = t('manualCopy.fillRequired');
    return;
  }

  isSubmitting.value = true;
  inlineError.value = '';

  try {
    const source = sourcePath.value.trim();
    const target = targetRootPath.value.trim();
    const preview = await previewTemporaryCopy(source, target);

    // Gate on a freshly-generated source file first (independent of any target
    // conflict) so only one prompt is shown at a time.
    let skipStability = false;
    if (isRecentSource(preview)) {
      const choice = await confirmRecency(preview.source_modified_secs_ago as number);
      skipStability = choice === 'immediate';
    }

    if (preview.target_exists) {
      existingTargetPreview.value = preview;
      pendingSubmitRequest.value = { source, target, skipStability };
      return;
    }

    await enqueueCopy(source, target, false, skipStability);
  } catch (error) {
    inlineError.value = formatManualCopyError(error);
    notify(inlineError.value, 'error');
  } finally {
    isSubmitting.value = false;
  }
}

function closeModal() {
  if (existingTargetPreview.value) {
    return;
  }
  clearExistingTargetDecision();
  emit('close');
}

function onModalKeydown(event: KeyboardEvent) {
  // Focus-trap: cycle Tab between the first and last focusable element so the
  // user never tabs out of the modal.
  if (event.key !== 'Tab' || !modalRef.value) return;
  const focusables = Array.from(
    modalRef.value.querySelectorAll<HTMLElement>(
      'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ),
  ).filter((el) => !el.hasAttribute('aria-hidden'));
  if (focusables.length === 0) return;
  const first = focusables[0];
  const last = focusables[focusables.length - 1];
  const active = document.activeElement as HTMLElement | null;
  if (event.shiftKey && active === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && active === last) {
    event.preventDefault();
    first.focus();
  }
}

// Save form data on input
watch([sourcePath, targetRootPath], () => {
  updateManualCopyForm({
    sourcePath: sourcePath.value,
    targetRootPath: targetRootPath.value,
  });
});

watch([sourcePath, targetRootPath], () => {
  batchPreviewOpen.value = false;
  batchResolutions.value = [];
  batchRowPreview.value = new Map();
  batchRowChecked.value = new Map();
});

onMounted(() => {
  restoreFormData();
  loadConfig();
});

onBeforeUnmount(() => {
  // Defensive cleanup — closing the modal during a paint flush sometimes
  // leaves a focus-trap listener behind otherwise.
  if (typeof document !== 'undefined' && previouslyFocused) {
    previouslyFocused = null;
  }
  // Drop any pending recency countdown timer so it cannot fire after teardown.
  if (recencyTimer !== null) {
    clearInterval(recencyTimer);
    recencyTimer = null;
  }
});

watch(() => props.isOpen, (open) => {
  if (open) {
    isSubmitting.value = false;
    inlineError.value = '';
    clearExistingTargetDecision();
    // Stash the previously-focused element so we can restore focus when the
    // modal closes (a11y baseline for dialog windows).
    if (typeof document !== 'undefined') {
      previouslyFocused = document.activeElement as HTMLElement | null;
    }
    loadConfig();
    focusSourceInput();
  } else {
    // Restore focus to the trigger element after the modal closes so keyboard
    // users land back where they came from.
    requestAnimationFrame(() => {
      previouslyFocused?.focus?.();
      previouslyFocused = null;
    });
  }
});
</script>

<template>
  <Transition name="modal-fade">
    <div
      v-if="isOpen"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 px-4"
      @keydown="onModalKeydown"
    >
      <!-- Modal Container -->
      <div
        ref="modalRef"
        class="relative bg-white rounded-2xl shadow-xl max-w-2xl w-full max-h-[90vh] flex flex-col overflow-hidden focus:outline-none"
        role="dialog"
        aria-modal="true"
        aria-labelledby="manual-copy-title"
        aria-describedby="manual-copy-desc"
        tabindex="-1"
      >
        <!-- Modal Header -->
        <div class="shrink-0 flex items-center justify-between border-b border-slate-200 bg-white px-6 py-4">
          <div>
            <h3 id="manual-copy-title" class="text-lg font-bold text-slate-800">{{ t('manualCopy.title') }}</h3>
            <p id="manual-copy-desc" class="text-sm text-slate-500 mt-1">{{ t('manualCopy.subtitle') }}</p>
          </div>
          <button
            @click="closeModal"
            class="p-2 rounded-lg hover:bg-slate-100 transition-colors motion-reduce:transition-none text-slate-600 hover:text-slate-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-1"
            :aria-label="t('common.close')"
            :title="t('common.close')"
          >
            <X class="w-5 h-5" aria-hidden="true" />
          </button>
        </div>

        <!-- Modal Content -->
        <div class="flex-1 overflow-y-auto p-6 space-y-6">
          <!-- Loading skeleton during config load -->
          <div v-if="isLoadingConfig && !config" class="space-y-3" role="status" aria-live="polite">
            <LoadingSkeleton variant="text-line" :lines="2" />
            <LoadingSkeleton variant="text-line" :lines="2" />
            <LoadingSkeleton variant="card" />
          </div>

          <template v-else>
            <!-- Form Section -->
            <div class="space-y-4">
              <!-- Source Path Input -->
              <div>
                <label for="manual-copy-source" class="block text-sm font-medium text-slate-700 mb-2">
                  {{ t('manualCopy.sourcePath') }}
                </label>
                <textarea
                  id="manual-copy-source"
                  ref="sourceInputRef"
                  v-model="sourcePath"
                  rows="3"
                  :disabled="isSubmitting || batchSubmitting || Boolean(existingTargetPreview)"
                  class="w-full p-3 border border-slate-300 rounded-xl focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all motion-reduce:transition-none disabled:cursor-not-allowed disabled:bg-slate-100 font-mono text-sm resize-y min-h-[3.25rem] max-h-[12rem]"
                  :placeholder="isBatchMode ? t('manualCopy.batch.placeholder') : t('manualCopy.sourcePlaceholder')"
                  :aria-invalid="Boolean(inlineError) || undefined"
                />
              </div>

              <!-- Target Path Input -->
              <div>
                <label for="manual-copy-target" class="block text-sm font-medium text-slate-700 mb-2">
                  {{ t('manualCopy.targetRootPath') }}
                </label>
                <div class="flex gap-2">
                  <input
                    id="manual-copy-target"
                    v-model="targetRootPath"
                    type="text"
                    :disabled="isSubmitting || Boolean(existingTargetPreview)"
                    class="flex-1 p-3 border border-slate-300 rounded-xl focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all motion-reduce:transition-none disabled:cursor-not-allowed disabled:bg-slate-100"
                    :placeholder="t('manualCopy.targetPlaceholder')"
                    :aria-invalid="Boolean(inlineError) || undefined"
                  />
                  <button
                    @click="selectTargetDirectory"
                    :disabled="isSelectingTarget || isSubmitting || Boolean(existingTargetPreview)"
                    :title="t('manualCopy.browseFolder')"
                    :aria-label="t('manualCopy.browseFolder')"
                    class="px-4 py-3 rounded-xl border border-slate-300 bg-slate-50 hover:bg-slate-100 transition-colors motion-reduce:transition-none disabled:opacity-60 disabled:cursor-not-allowed inline-flex items-center gap-2 text-slate-600 font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-1"
                  >
                    <FolderOpen class="w-4 h-4" aria-hidden="true" />
                    <span class="hidden sm:inline">{{ t('manualCopy.browse') }}</span>
                  </button>
                </div>
                <p class="text-xs text-slate-400 mt-2">{{ t('manualCopy.targetHint') }}</p>
              </div>
            </div>

            <!-- Batch mode preview (only when N >= 2 lines pasted) -->
            <div
              v-if="isBatchMode"
              class="rounded-xl border border-blue-200 bg-blue-50/40 px-5 py-4 space-y-4"
            >
              <div class="flex items-center justify-between gap-3">
                <span class="text-sm font-medium text-blue-700">
                  {{ t('manualCopy.batch.filtersApplyAll', { count: sourceLines.length }) }}
                </span>
                <button
                  v-if="!batchPreviewOpen"
                  type="button"
                  @click="previewBatch"
                  :disabled="batchSubmitting || isSubmitting"
                  class="inline-flex items-center gap-2 px-4 py-2 rounded-lg bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
                >
                  <Play class="w-4 h-4" aria-hidden="true" />
                  {{ t('manualCopy.batch.previewButton', { count: sourceLines.length }) }}
                </button>
                <button
                  v-else
                  type="button"
                  @click="backToBatchEdit"
                  class="text-sm text-slate-500 hover:text-slate-700"
                >
                  {{ t('manualCopy.batch.backToEdit') }}
                </button>
              </div>

              <div v-if="batchPreviewOpen" class="space-y-2">
                <table class="w-full text-sm border border-slate-200 rounded-lg overflow-hidden bg-white">
                  <thead class="bg-slate-50 text-slate-600 text-xs uppercase tracking-wide">
                    <tr>
                      <th class="px-3 py-2 text-left w-10">
                        <input
                          type="checkbox"
                          :checked="allBatchRowsChecked"
                          @change="(e) => toggleAllBatchRows((e.target as HTMLInputElement).checked)"
                          :aria-label="t('manualCopy.batch.selectAll')"
                        />
                      </th>
                      <th class="px-3 py-2 text-left">{{ t('manualCopy.batch.colSource') }}</th>
                      <th class="px-3 py-2 text-left">{{ t('manualCopy.batch.colTarget') }}</th>
                      <th class="px-3 py-2 text-left w-32">{{ t('manualCopy.batch.colStatus') }}</th>
                    </tr>
                  </thead>
                  <tbody class="divide-y divide-slate-100">
                    <tr
                      v-for="r in batchResolutions"
                      :key="r.rawSource"
                      class="hover:bg-slate-50"
                    >
                      <td class="px-3 py-2 align-top">
                        <input
                          type="checkbox"
                          :checked="batchRowChecked.get(r.rawSource) === true"
                          :disabled="batchRowPreview.get(r.rawSource)?.status === 'invalid_path'"
                          @change="toggleBatchRow(r.rawSource)"
                        />
                      </td>
                      <td class="px-3 py-2 font-mono text-xs break-all text-slate-700">{{ r.rawSource }}</td>
                      <td class="px-3 py-2 font-mono text-xs break-all text-slate-600">
                        {{ batchRowPreview.get(r.rawSource)?.finalTarget || r.finalTarget || '—' }}
                      </td>
                      <td class="px-3 py-2 align-top">
                        <span
                          class="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium border"
                          :class="batchStatusClass(batchRowPreview.get(r.rawSource)?.status ?? r.status as BatchPreviewStatus)"
                        >
                          {{ batchStatusLabel(batchRowPreview.get(r.rawSource)?.status ?? r.status as BatchPreviewStatus) }}
                        </span>
                      </td>
                    </tr>
                  </tbody>
                </table>
              </div>
              <div v-else class="text-xs text-slate-500">
                {{ t('manualCopy.batch.emptyPreviewHint') }}
              </div>
            </div>

            <!-- Inline error message (form validation feedback). Toast-style
                 success/info notifications are pushed through useToast. -->
            <div
              v-if="inlineError"
              role="alert"
              aria-live="assertive"
              class="rounded-xl px-4 py-3 text-sm border bg-red-50 text-red-600 border-red-100 flex items-start gap-2"
            >
              <AlertTriangle class="w-4 h-4 mt-0.5 shrink-0" aria-hidden="true" />
              <span class="break-all">{{ inlineError }}</span>
            </div>

            <!-- Filter Rules Card -->
            <div class="bg-slate-50 rounded-xl border border-slate-200 p-5 space-y-4">
              <div class="flex items-center gap-2 text-slate-800 font-semibold">
                <ShieldCheck class="w-4 h-4 text-blue-600" aria-hidden="true" />
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
                      class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border cursor-pointer transition-colors motion-reduce:transition-none select-none text-sm focus-within:ring-2 focus-within:ring-blue-500/50 focus-within:ring-offset-1"
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
                        aria-hidden="true"
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
                      class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border cursor-pointer transition-colors motion-reduce:transition-none select-none text-sm focus-within:ring-2 focus-within:ring-purple-500/50 focus-within:ring-offset-1"
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
                        aria-hidden="true"
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

            <!-- Other Rules Card (stability + execution mode shown as info-tone
                 cards so the read-only contracts have a visible hint icon). -->
            <div class="bg-blue-50/40 rounded-xl border border-blue-100 p-5 space-y-3">
              <div class="space-y-3 text-sm text-slate-600">
                <div class="flex items-start gap-2">
                  <Info class="w-4 h-4 text-blue-500 shrink-0 mt-0.5" aria-hidden="true" />
                  <div>
                    <div class="font-medium text-slate-700 mb-1">{{ t('manualCopy.stabilityTitle') }}</div>
                    <div>{{ stabilitySummary }}</div>
                  </div>
                </div>
                <div class="flex items-start gap-2">
                  <Info class="w-4 h-4 text-blue-500 shrink-0 mt-0.5" aria-hidden="true" />
                  <div>
                    <div class="font-medium text-slate-700 mb-1">{{ t('manualCopy.modeTitle') }}</div>
                    <div>{{ t('manualCopy.modeDesc') }}</div>
                  </div>
                </div>
              </div>
            </div>
          </template>
        </div>

        <!-- Modal Footer -->
        <div class="shrink-0 border-t border-slate-200 bg-white px-6 py-4 flex items-center justify-end gap-3">
          <button
            v-if="!isBatchMode"
            @click="submitCopy"
            :disabled="!canSubmit"
            class="inline-flex items-center gap-2 px-5 py-2.5 rounded-xl text-white font-medium transition-colors motion-reduce:transition-none disabled:opacity-60 disabled:cursor-not-allowed bg-blue-600 hover:bg-blue-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 focus-visible:ring-offset-1"
          >
            <Loader2 v-if="isSubmitting" class="w-4 h-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
            <Play v-else class="w-4 h-4" aria-hidden="true" />
            {{ isSubmitting ? t('manualCopy.submitting') : t('manualCopy.startCopy') }}
          </button>
          <button
            v-else
            @click="submitBatch"
            :disabled="!batchPreviewOpen || checkedBatchCount === 0 || batchSubmitting"
            class="inline-flex items-center gap-2 px-5 py-2.5 rounded-xl text-white font-medium transition-colors motion-reduce:transition-none disabled:opacity-60 disabled:cursor-not-allowed bg-blue-600 hover:bg-blue-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 focus-visible:ring-offset-1"
          >
            <Loader2 v-if="batchSubmitting" class="w-4 h-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
            <Play v-else class="w-4 h-4" aria-hidden="true" />
            {{ t('manualCopy.batch.submitButton', { count: checkedBatchCount }) }}
          </button>
        </div>

        <!-- Conflict resolution overlay — fixed so it always covers viewport regardless of modal scroll -->
        <Teleport to="body">
          <Transition name="confirm-fade">
            <div
              v-if="existingTargetPreview"
              class="fixed inset-0 z-[70] flex items-center justify-center bg-black/50 backdrop-blur-sm px-4"
              role="dialog"
              aria-modal="true"
              aria-labelledby="manual-copy-conflict-title"
              @click="!isSubmitting && cancelExistingTargetDecision()"
            >
              <div
                class="w-full max-w-xl rounded-2xl border border-slate-200 bg-white shadow-2xl shadow-slate-400/20 overflow-hidden"
                @click.stop
              >
                <!-- Dialog header -->
                <div class="flex items-start gap-4 px-6 pt-6 pb-5 border-b border-slate-100">
                  <div class="flex-shrink-0 w-10 h-10 rounded-xl bg-amber-50 border border-amber-200 flex items-center justify-center">
                    <AlertTriangle class="w-5 h-5 text-amber-600" aria-hidden="true" />
                  </div>
                  <div class="min-w-0 pt-0.5">
                    <div id="manual-copy-conflict-title" class="text-base font-semibold text-slate-800">
                      {{ t('manualCopy.targetExistsDecisionTitle') }}
                    </div>
                    <p class="text-sm leading-6 text-slate-500 mt-1.5 break-all">
                      {{ existingTargetSummary(existingTargetPreview) }}
                    </p>
                  </div>
                </div>

                <!-- Option cards -->
                <div class="p-4 space-y-2.5">
                  <!-- Overwrite (destructive) -->
                  <button
                    @click="confirmExistingTarget(true)"
                    :disabled="isSubmitting"
                    class="w-full flex items-start gap-3.5 rounded-xl border border-amber-300 bg-amber-50 px-4 py-3.5 text-left transition-all motion-reduce:transition-none hover:border-amber-400 hover:bg-amber-100 hover:shadow-sm active:scale-[0.99] motion-reduce:active:scale-100 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500/60 focus-visible:ring-offset-2"
                    :aria-label="t('manualCopy.overwriteAndQueue')"
                  >
                    <div class="flex-shrink-0 w-8 h-8 rounded-lg bg-amber-600 flex items-center justify-center mt-0.5">
                      <RefreshCw class="w-3.5 h-3.5 text-white" aria-hidden="true" />
                    </div>
                    <div class="min-w-0 flex-1">
                      <div class="text-sm font-semibold text-amber-800">
                        {{ t('manualCopy.overwriteAndQueue') }}
                      </div>
                      <div class="text-xs text-slate-600 mt-1 leading-5 break-words">
                        {{ overwriteActionHint(existingTargetPreview) }}
                      </div>
                    </div>
                  </button>

                  <!-- Copy new files only (safe option) -->
                  <button
                    @click="confirmExistingTarget(false)"
                    :disabled="isSubmitting"
                    class="w-full flex items-start gap-3.5 rounded-xl border border-emerald-200 bg-emerald-50 px-4 py-3.5 text-left transition-all motion-reduce:transition-none hover:border-emerald-300 hover:bg-emerald-100 hover:shadow-sm active:scale-[0.99] motion-reduce:active:scale-100 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/60 focus-visible:ring-offset-2"
                    :aria-label="t('manualCopy.skipAndQueue')"
                  >
                    <div class="flex-shrink-0 w-8 h-8 rounded-lg bg-emerald-600 flex items-center justify-center mt-0.5">
                      <FilePlus2 class="w-3.5 h-3.5 text-white" aria-hidden="true" />
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
                    class="px-4 py-2 rounded-lg border border-slate-200 bg-white text-sm font-medium text-slate-600 hover:bg-slate-50 hover:border-slate-300 transition-colors motion-reduce:transition-none disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-1"
                  >
                    {{ t('manualCopy.cancelConflictDecision') }}
                  </button>
                </div>
              </div>
            </div>
          </Transition>
        </Teleport>

        <!-- Recently-modified ("just generated") confirmation prompt -->
        <Teleport to="body">
          <Transition name="confirm-fade">
            <div
              v-if="recencyPrompt"
              class="fixed inset-0 z-[80] flex items-center justify-center bg-black/50 backdrop-blur-sm px-4"
              role="dialog"
              aria-modal="true"
              aria-labelledby="manual-copy-recency-title"
              @click="resolveRecency('wait')"
            >
              <div
                class="w-full max-w-lg rounded-2xl border border-slate-200 bg-white shadow-2xl shadow-slate-400/20 overflow-hidden"
                @click.stop
              >
                <!-- Header -->
                <div class="flex items-start gap-4 px-6 pt-6 pb-5 border-b border-slate-100">
                  <div class="flex-shrink-0 w-10 h-10 rounded-xl bg-amber-50 border border-amber-200 flex items-center justify-center">
                    <Clock class="w-5 h-5 text-amber-600" aria-hidden="true" />
                  </div>
                  <div class="min-w-0 pt-0.5">
                    <div id="manual-copy-recency-title" class="text-base font-semibold text-slate-800">
                      {{ t('manualCopy.recency.title') }}
                    </div>
                    <p class="text-sm leading-6 text-slate-500 mt-1.5">
                      {{ t('manualCopy.recency.body', {
                        ago: formatModifiedAgo(recencyPrompt.secsAgo),
                        secs: config?.stability_check_secs ?? 0,
                      }) }}
                    </p>
                  </div>
                </div>

                <!-- Option cards -->
                <div class="p-4 space-y-2.5">
                  <!-- Copy immediately (skip wait) -->
                  <button
                    @click="resolveRecency('immediate')"
                    class="w-full flex items-start gap-3.5 rounded-xl border border-blue-300 bg-blue-50 px-4 py-3.5 text-left transition-all motion-reduce:transition-none hover:border-blue-400 hover:bg-blue-100 hover:shadow-sm active:scale-[0.99] motion-reduce:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 focus-visible:ring-offset-2"
                    :aria-label="t('manualCopy.recency.copyNow')"
                  >
                    <div class="flex-shrink-0 w-8 h-8 rounded-lg bg-blue-600 flex items-center justify-center mt-0.5">
                      <Zap class="w-3.5 h-3.5 text-white" aria-hidden="true" />
                    </div>
                    <div class="min-w-0 flex-1">
                      <div class="text-sm font-semibold text-blue-800">
                        {{ t('manualCopy.recency.copyNow') }}
                      </div>
                      <div class="text-xs text-slate-600 mt-1 leading-5 break-words">
                        {{ t('manualCopy.recency.copyNowHint') }}
                      </div>
                    </div>
                  </button>

                  <!-- Wait for stability (default) -->
                  <button
                    @click="resolveRecency('wait')"
                    class="w-full flex items-start gap-3.5 rounded-xl border border-emerald-200 bg-emerald-50 px-4 py-3.5 text-left transition-all motion-reduce:transition-none hover:border-emerald-300 hover:bg-emerald-100 hover:shadow-sm active:scale-[0.99] motion-reduce:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/60 focus-visible:ring-offset-2"
                    :aria-label="t('manualCopy.recency.waitNow')"
                  >
                    <div class="flex-shrink-0 w-8 h-8 rounded-lg bg-emerald-600 flex items-center justify-center mt-0.5">
                      <ShieldCheck class="w-3.5 h-3.5 text-white" aria-hidden="true" />
                    </div>
                    <div class="min-w-0 flex-1">
                      <div class="text-sm font-semibold text-emerald-800">
                        {{ t('manualCopy.recency.waitNow') }}
                      </div>
                      <div class="text-xs text-slate-600 mt-1 leading-5 break-words">
                        {{ t('manualCopy.recency.waitNowHint') }}
                      </div>
                    </div>
                  </button>
                </div>

                <!-- Countdown footer -->
                <div class="px-6 pb-4 pt-1 text-center text-xs text-slate-500">
                  {{ t('manualCopy.recency.countdown', { secs: recencyCountdown }) }}
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

/* Respect user preferences: skip the slide/scale transitions when the OS
   reports prefers-reduced-motion. The opacity step survives so the modal
   still announces presence/absence visually. */
@media (prefers-reduced-motion: reduce) {
  .confirm-fade-enter-from,
  .confirm-fade-leave-to,
  .confirm-fade-enter-to,
  .confirm-fade-leave-from {
    transform: none;
  }
  .modal-fade-enter-active,
  .modal-fade-leave-active,
  .confirm-fade-enter-active,
  .confirm-fade-leave-active {
    transition-duration: 120ms;
  }
}
</style>
