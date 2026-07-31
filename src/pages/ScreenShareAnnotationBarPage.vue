<script setup lang="ts">
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Eraser, Undo2, X } from 'lucide-vue-next';
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import {
  screenShareAnnotationBarReady,
  screenShareClearAnnotations,
  screenShareGetAnnotationState,
  screenShareRemoveAnnotation,
  screenShareSetAnnotationBarVisible,
} from '@/lib/tauri';
import { annotationBarView, carryDismissal } from '@/screen-share-web/lib/annotation-bar';
import { emptyDocument, normalizeDocument } from '@/screen-share-web/lib/annotation-state';
import type { AnnotationDocument } from '@/screen-share-web/types';

defineOptions({ name: 'ScreenShareAnnotationBarPage' });

const { t } = useI18n();
const annotationBarWindow = getCurrentWindow();

const documentState = ref<AnnotationDocument>(emptyDocument());
const pendingAction = ref<'undo' | 'clear' | null>(null);
const actionError = ref(false);
/**
 * Persistent-annotation count at the moment the host dismissed the bar. The bar
 * stays hidden until a newer annotation arrives, so dismissing is not the same
 * as turning the feature off for the session.
 */
const dismissedAtCount = ref<number | null>(null);

let unlistenAnnotationState: UnlistenFn | null = null;
let visibilitySync = Promise.resolve();

const barView = computed(() => annotationBarView(documentState.value, dismissedAtCount.value));
const annotationCount = computed(() => barView.value.count);
const latestShapeId = computed(() => barView.value.latestShapeId);
const shouldShow = computed(() => barView.value.visible);

function applyDocument(value: unknown) {
  const next = normalizeDocument(value, documentState.value);
  const current = documentState.value;
  if (next.sessionId === current.sessionId) {
    if (next.sourceEpoch < current.sourceEpoch) return;
    if (next.sourceEpoch === current.sourceEpoch && next.revision < current.revision) return;
  }
  documentState.value = next;
}

async function undoLatest() {
  const latest = latestShapeId.value;
  if (!latest || pendingAction.value) return;
  pendingAction.value = 'undo';
  actionError.value = false;
  try {
    await screenShareRemoveAnnotation(latest);
    // The annotation event refreshes the count; nothing to apply locally.
  } catch {
    actionError.value = true;
  } finally {
    pendingAction.value = null;
  }
}

async function clearAll() {
  if (pendingAction.value) return;
  pendingAction.value = 'clear';
  actionError.value = false;
  try {
    await screenShareClearAnnotations();
  } catch {
    actionError.value = true;
  } finally {
    pendingAction.value = null;
  }
}

function requestBarVisibility(visible: boolean): Promise<void> {
  // Serialize window commands so a slower, stale "show" request cannot win
  // after the annotation count reaches zero or the host dismisses the bar.
  visibilitySync = visibilitySync.then(async () => {
    if (!visible) {
      try {
        // Hide the native window directly as a fail-safe. The Rust command
        // below also updates the session-side visibility flag.
        await annotationBarWindow.hide();
      } catch {
        /* The Rust command below remains the authoritative fallback. */
      }
    }
    try {
      await screenShareSetAnnotationBarVisible(visible);
    } catch {
      /* A stale session or a stopped share already hides the window. */
    }
  });
  return visibilitySync;
}

function dismiss() {
  dismissedAtCount.value = annotationCount.value;
  // The bar can be visible while the reactive view already says "hidden"
  // (for example after the last annotation is removed). Always issue the hide
  // command instead of relying solely on the visibility watcher to re-run.
  void requestBarVisibility(false);
}

function startDragging(event: MouseEvent) {
  if (event.button !== 0 || (event.target as Element).closest('button')) return;
  event.preventDefault();
  void annotationBarWindow.startDragging().catch(() => {
    /* A closing or stale window no longer needs to be draggable. */
  });
}

// Visibility lives in Rust so the bar can appear without stealing focus from
// whatever the host is presenting.
watch(shouldShow, (visible) => {
  actionError.value = false;
  void requestBarVisibility(visible);
});

// Keep the stored dismissal in step with the live count, so clearing (or
// removing part of) the annotations lets a later one bring the bar back.
watch(annotationCount, (count) => {
  const next = carryDismissal(count, dismissedAtCount.value);
  if (next !== dismissedAtCount.value) dismissedAtCount.value = next;
});

onMounted(async () => {
  document.documentElement.classList.add('screen-share-annotation-bar-window');
  document.body.classList.add('screen-share-annotation-bar-window');
  try {
    unlistenAnnotationState = await listen<unknown>('screen-share-annotation-state', (event) => {
      applyDocument(event.payload);
    });
    applyDocument(await screenShareGetAnnotationState());
    await screenShareAnnotationBarReady();
    // Reconcile both states after startup. An explicit false also removes a
    // stale zero-count bar left visible by an earlier asynchronous request.
    await requestBarVisibility(shouldShow.value);
  } catch {
    await annotationBarWindow.close();
  }
});

onUnmounted(() => {
  unlistenAnnotationState?.();
  document.documentElement.classList.remove('screen-share-annotation-bar-window');
  document.body.classList.remove('screen-share-annotation-bar-window');
});
</script>

<template>
  <main class="bar-root">
    <div
      v-if="shouldShow"
      class="bar-shell"
      :title="t('tools.screenShare.annotationBarMove')"
      @mousedown="startDragging"
    >
      <span class="bar-count">
        <span class="bar-dot" aria-hidden="true"></span>
        <span class="bar-count-text">
          {{ actionError
            ? t('tools.screenShare.annotationBarFailed')
            : t('tools.screenShare.annotationBarCount', { count: annotationCount }) }}
        </span>
      </span>
      <div class="bar-actions">
        <button
          type="button"
          class="bar-button"
          :disabled="!latestShapeId || pendingAction !== null"
          :title="t('tools.screenShare.annotationBarUndo')"
          @click="undoLatest"
        >
          <Undo2 class="bar-icon" aria-hidden="true" />
          {{ t('tools.screenShare.annotationBarUndo') }}
        </button>
        <button
          type="button"
          class="bar-button bar-button--danger"
          :disabled="annotationCount === 0 || pendingAction !== null"
          :title="t('tools.screenShare.annotationBarClear')"
          @click="clearAll"
        >
          <Eraser class="bar-icon" aria-hidden="true" />
          {{ t('tools.screenShare.annotationBarClear') }}
        </button>
        <button
          type="button"
          class="bar-dismiss"
          :title="t('tools.screenShare.annotationBarHide')"
          :aria-label="t('tools.screenShare.annotationBarHide')"
          @click="dismiss"
        >
          <X class="bar-icon" aria-hidden="true" />
        </button>
      </div>
    </div>
  </main>
</template>

<style scoped>
:global(html.screen-share-annotation-bar-window),
:global(body.screen-share-annotation-bar-window),
:global(body.screen-share-annotation-bar-window #app) {
  background: transparent !important;
}

.bar-root {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  overflow: hidden;
  background: transparent;
}

.bar-shell {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: 10px;
  border-radius: 12px;
  border: 1px solid rgb(148 163 184 / 0.28);
  background: rgb(15 23 42 / 0.92);
  padding: 8px 10px;
  cursor: move;
  font-family: inherit;
}

.bar-count {
  display: inline-flex;
  min-width: 0;
  align-items: center;
  gap: 6px;
  padding-left: 4px;
  color: rgb(226 232 240);
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
  user-select: none;
}

.bar-count-text {
  flex-shrink: 0;
}

.bar-dot {
  height: 7px;
  width: 7px;
  flex-shrink: 0;
  border-radius: 9999px;
  background: rgb(245 158 11);
}

.bar-actions {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  gap: 6px;
}

.bar-button {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border-radius: 8px;
  border: 1px solid rgb(148 163 184 / 0.3);
  background: rgb(51 65 85 / 0.7);
  padding: 6px 9px;
  color: rgb(241 245 249);
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
  cursor: pointer;
  transition: background-color 120ms ease, border-color 120ms ease;
}

.bar-button:hover:not(:disabled) {
  border-color: rgb(148 163 184 / 0.55);
  background: rgb(71 85 105 / 0.85);
}

.bar-button--danger:hover:not(:disabled) {
  border-color: rgb(248 113 113 / 0.6);
  background: rgb(127 29 29 / 0.75);
  color: rgb(254 226 226);
}

.bar-button:disabled {
  cursor: not-allowed;
  opacity: 0.42;
}

.bar-dismiss {
  display: inline-flex;
  height: 26px;
  width: 26px;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  cursor: pointer;
  color: rgb(148 163 184);
  transition: background-color 120ms ease, color 120ms ease;
}

.bar-dismiss:hover {
  background: rgb(71 85 105 / 0.7);
  color: rgb(241 245 249);
}

.bar-button:focus-visible,
.bar-dismiss:focus-visible {
  outline: 2px solid rgb(56 189 248);
  outline-offset: 2px;
}

.bar-icon {
  height: 13px;
  width: 13px;
  flex-shrink: 0;
}
</style>
