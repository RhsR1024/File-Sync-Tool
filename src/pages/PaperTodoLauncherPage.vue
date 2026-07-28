<script setup lang="ts">
import {
  Check,
  ChevronDown,
  ChevronRight,
  FilePlus2,
  ListPlus,
  PencilLine,
  X,
} from 'lucide-vue-next';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import { usePaperTodo } from '@/composables/usePaperTodo';
import {
  closePaperWindow,
  createDesktopPaper,
  dragPaperLauncher,
  openPaperWindow,
  movePaperId,
  setPaperLauncherExpanded,
  type PaperDocument,
} from '@/lib/paperTodo';

defineOptions({ name: 'PaperTodoLauncherPage' });

const { t, locale } = useI18n();
const store = usePaperTodo();
const expanded = ref(false);
const masterCapsule = ref<HTMLElement | null>(null);
const openingPaperId = ref<string | null>(null);
const creatingKind = ref<'todo' | 'note' | null>(null);
const deletingPaperId = ref<string | null>(null);
const draggedPaperId = ref<string | null>(null);
const dropTargetId = ref<string | null>(null);
const dropSide = ref<'before' | 'after'>('before');
let collapseTimer: ReturnType<typeof setTimeout> | null = null;
// Native window commands can run concurrently on Tauri's command threads. Keep
// them in user-action order so a slow startup sync cannot finish after a
// later expand and leave the Vue state out of sync with the native window size.
let launcherSyncQueue: Promise<void> = Promise.resolve();
let draggingLauncher = false;
let paperDragFinished = false;
// Last known pointer state, kept explicitly because the webview reports the
// expand reposition as a leave/enter pair the user never performed.
let pointerInside = false;
// Wall-clock deadline until which `mouseleave` is treated as reposition noise
// rather than intent. Expanding grows and slides the window under a cursor
// that never moved, and the drag loop flushes the pointer events it swallowed
// at the same moment; a flag the first stray event can clear does not survive
// that, a deadline does.
let settleUntil = 0;
const SETTLE_MS = 400;
const COLLAPSE_DELAY_MS = 700;

const edge = computed(() => store.settings.value.launcherEdge);
const paperCount = computed(() => store.papers.value.length);
const expandedRowCount = computed(() => (
  paperCount.value === 0 ? 2 : paperCount.value + 1
));
const systemDark = typeof window !== 'undefined'
  && window.matchMedia('(prefers-color-scheme: dark)').matches;
const useDarkTheme = computed(() => {
  const theme = store.settings.value.theme;
  return theme === 'dark' || (theme === 'system' && systemDark);
});

function compactPaperTitle(title: string): string {
  const characters = Array.from(title.trim());
  return characters.length > 10 ? `${characters.slice(0, 9).join('')}…` : characters.join('');
}

function paperTitleIsTruncated(title: string): boolean {
  return Array.from(title.trim()).length > 10;
}

function cancelCollapse(): void {
  if (collapseTimer) clearTimeout(collapseTimer);
  collapseTimer = null;
}

/**
 * The pointer is over the launcher, so auto-collapse is disarmed. Bound to
 * both enter and move: when the window is resized underneath a stationary
 * cursor the webview may consider the pointer to have never left, in which
 * case no `mouseenter` follows and only a move reports the truth.
 */
function noteLauncherHovered(): void {
  pointerInside = true;
  cancelCollapse();
}

/**
 * Logical width the collapsed capsule needs for its own label. The capsule is
 * sized to `max-content`, so this stays the label's true width even when the
 * native window is currently narrower than it.
 */
function measureCapsuleWidth(): number | null {
  const element = masterCapsule.value;
  if (!element) return null;
  const width = element.getBoundingClientRect().width;
  return width > 0 ? Math.ceil(width) : null;
}

/**
 * Native size and position commands run on Tauri's command threads and can
 * finish out of order. Funnelling every sync through one chain keeps the window
 * matching the last action the user took.
 */
function queueLauncherSync(
  value: boolean,
  itemCount: number,
  capsuleWidth: number | null,
): Promise<void> {
  const run = () => setPaperLauncherExpanded(value, itemCount, capsuleWidth);
  const sync = launcherSyncQueue.then(run, run);
  launcherSyncQueue = sync.catch(() => undefined);
  return sync;
}

async function setExpanded(value: boolean): Promise<void> {
  cancelCollapse();
  expanded.value = value;
  // Expanding both grows the window and slides it along the edge, and the
  // webview reports that reposition as a `mouseleave` the user never made.
  // Hold auto-collapse off for a fixed period so the list cannot fold itself
  // back up before it has been seen.
  if (value) {
    pointerInside = true;
    settleUntil = Date.now() + SETTLE_MS;
  } else {
    settleUntil = 0;
  }
  // Reserve the creation row and, when there are no papers, the empty-state
  // row above it so both creation buttons remain inside the native window.
  const itemCount = expandedRowCount.value;
  // Measure after the label has switched, so a collapse reports the width of
  // the count it is about to show rather than the one it is leaving.
  await nextTick();
  await queueLauncherSync(value, itemCount, value ? null : measureCapsuleWidth());
}

/** Re-report the collapsed capsule width after its label changed. */
async function syncCollapsedWidth(): Promise<void> {
  if (expanded.value) return;
  await nextTick();
  await queueLauncherSync(false, expandedRowCount.value, measureCapsuleWidth());
}

async function startLauncherDrag(event: MouseEvent): Promise<void> {
  if (event.button !== 0 || draggedPaperId.value || draggingLauncher) return;
  cancelCollapse();
  pointerInside = true;
  draggingLauncher = true;
  try {
    // The capsule is the whole drag handle now, so a press is ambiguous: the
    // native loop pins it to the display edge, clamps it to the primary
    // monitor, and reports whether it ever travelled. A press that did not move
    // is the expand/collapse click.
    const moved = await dragPaperLauncher();
    if (moved) {
      settleUntil = Date.now() + SETTLE_MS;
      await store.refreshFromDisk();
    } else {
      await setExpanded(!expanded.value);
    }
  } catch (reason) {
    store.error.value = String(reason);
  } finally {
    draggingLauncher = false;
  }
}

/**
 * Pointer presses are resolved by the drag loop, so only keyboard activation —
 * which reports no click count — still has to toggle here.
 */
function toggleFromKeyboard(event: MouseEvent): void {
  if (event.detail !== 0) return;
  void setExpanded(!expanded.value);
}

function scheduleCollapse(): void {
  if (!expanded.value) return;
  // A leave that lands inside the settle window is the window moving out from
  // under a stationary cursor, not the user pointing away. Swallowing it keeps
  // the pointer marked as present, so a click that expands the launcher can no
  // longer fold it straight back up.
  if (Date.now() < settleUntil) return;
  pointerInside = false;
  cancelCollapse();
  // Reordering papers and dragging the launcher both move things around under
  // the cursor; neither is the user pointing away from the launcher.
  if (draggedPaperId.value || draggingLauncher) return;
  collapseTimer = setTimeout(() => {
    collapseTimer = null;
    if (pointerInside || draggedPaperId.value || draggingLauncher) return;
    void setExpanded(false);
  }, COLLAPSE_DELAY_MS);
}

async function openPaper(paper: PaperDocument): Promise<void> {
  if (paperDragFinished || openingPaperId.value) return;
  openingPaperId.value = paper.id;
  try {
    if (!paper.desktopOpen) {
      store.updatePaper(
        paper.id,
        (value) => { value.desktopOpen = true; },
        { immediate: true },
      );
      await store.flush();
    }
    await openPaperWindow(paper, store.settings.value);
  } catch (reason) {
    store.error.value = String(reason);
  } finally {
    openingPaperId.value = null;
  }
}

async function createPaper(kind: 'todo' | 'note'): Promise<void> {
  if (creatingKind.value) return;
  cancelCollapse();
  pointerInside = true;
  creatingKind.value = kind;
  try {
    await createDesktopPaper(kind);
    await store.refreshFromDisk();
  } catch (reason) {
    store.error.value = String(reason);
  } finally {
    creatingKind.value = null;
  }
}

async function deletePaper(paper: PaperDocument): Promise<void> {
  if (deletingPaperId.value || openingPaperId.value) return;
  cancelCollapse();
  pointerInside = true;
  deletingPaperId.value = paper.id;
  try {
    await store.removePaper(paper.id);
    if (!store.papers.value.some((candidate) => candidate.id === paper.id)) {
      await closePaperWindow(paper.id);
    }
  } catch (reason) {
    store.error.value = String(reason);
  } finally {
    deletingPaperId.value = null;
  }
}

function beginPaperDrag(event: DragEvent, paperId: string): void {
  cancelCollapse();
  draggedPaperId.value = paperId;
  dropTargetId.value = null;
  paperDragFinished = true;
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData('text/plain', paperId);
  }
}

function updateDropTarget(event: DragEvent, paperId: string): void {
  if (!draggedPaperId.value || draggedPaperId.value === paperId) {
    dropTargetId.value = null;
    return;
  }
  const target = event.currentTarget as HTMLElement;
  const bounds = target.getBoundingClientRect();
  dropTargetId.value = paperId;
  dropSide.value = event.clientY >= bounds.top + bounds.height / 2 ? 'after' : 'before';
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
}

async function persistPaperMove(sourceId: string, targetId: string, side: 'before' | 'after') {
  const orderedIds = movePaperId(
    store.papers.value.map((paper) => paper.id),
    sourceId,
    targetId,
    side,
  );
  try {
    await store.reorderPapers(orderedIds);
  } catch (reason) {
    store.error.value = String(reason);
    await store.refreshFromDisk();
  }
}

async function dropPaper(targetId: string): Promise<void> {
  const sourceId = draggedPaperId.value;
  if (sourceId && sourceId !== targetId) {
    await persistPaperMove(sourceId, targetId, dropSide.value);
  }
  finishPaperDrag();
}

async function movePaperByKeyboard(paperId: string, offset: -1 | 1): Promise<void> {
  const ids = store.papers.value.map((paper) => paper.id);
  const current = ids.indexOf(paperId);
  const target = current + offset;
  if (current < 0 || target < 0 || target >= ids.length) return;
  [ids[current], ids[target]] = [ids[target], ids[current]];
  try {
    await store.reorderPapers(ids);
  } catch (reason) {
    store.error.value = String(reason);
    await store.refreshFromDisk();
  }
}

function finishPaperDrag(): void {
  draggedPaperId.value = null;
  dropTargetId.value = null;
  window.setTimeout(() => { paperDragFinished = false; }, 0);
}

// The collapsed label carries the count, so both the paper list and the active
// locale change how wide the capsule has to be.
watch([paperCount, locale], () => {
  if (expanded.value) void setExpanded(true);
  else void syncCollapsedWidth();
});

onMounted(async () => {
  try {
    await store.initialize();
    // The backend creates and synchronizes the launcher in collapsed mode
    // before this webview loads, but it cannot know how wide the rendered
    // label is. Reporting it through the same queue as every later action
    // keeps a slow startup sync from landing after the user's first click.
    await syncCollapsedWidth();
  } catch (reason) {
    store.error.value = String(reason);
  }
});

onBeforeUnmount(() => {
  cancelCollapse();
});
</script>

<template>
  <div
    class="launcher-surface"
    :class="[
      edge === 'left' ? 'launcher-left' : 'launcher-right',
      useDarkTheme ? 'launcher-dark' : 'launcher-light',
    ]"
    @mouseenter="noteLauncherHovered"
    @mousemove="noteLauncherHovered"
    @mouseleave="scheduleCollapse"
    @contextmenu.prevent
  >
    <button
      ref="masterCapsule"
      type="button"
      class="launcher-master-capsule launcher-drag-handle"
      :title="t('paperTodo.launcher.moveHint')"
      :aria-label="expanded ? t('paperTodo.launcher.collapse') : t('paperTodo.launcher.expand')"
      :aria-expanded="expanded"
      aria-controls="paper-todo-capsule-list"
      @mousedown.stop.prevent="startLauncherDrag"
      @click="toggleFromKeyboard"
    >
      <ChevronDown v-if="expanded" class="launcher-chevron" aria-hidden="true" />
      <ChevronRight v-else class="launcher-chevron" aria-hidden="true" />
      <span>{{ expanded
        ? t('paperTodo.launcher.expandedLabel')
        : t('paperTodo.launcher.collapsedCount', { count: paperCount })
      }}</span>
    </button>

    <ol
      v-if="expanded"
      id="paper-todo-capsule-list"
      class="launcher-paper-list"
      :aria-label="t('paperTodo.launcher.paperList')"
    >
      <li
        v-for="paper in store.papers.value"
        :key="paper.id"
        class="launcher-paper-slot"
        :class="{
          'is-dragging': draggedPaperId === paper.id,
          'drop-before': dropTargetId === paper.id && dropSide === 'before',
          'drop-after': dropTargetId === paper.id && dropSide === 'after',
        }"
        draggable="true"
        @dragstart="beginPaperDrag($event, paper.id)"
        @dragover.prevent="updateDropTarget($event, paper.id)"
        @drop.prevent="dropPaper(paper.id)"
        @dragend="finishPaperDrag"
      >
        <button
          type="button"
          class="launcher-paper-capsule"
          :disabled="openingPaperId === paper.id || deletingPaperId === paper.id"
          :title="paperTitleIsTruncated(paper.title) ? paper.title : undefined"
          :aria-label="t('paperTodo.launcher.openPaper', { title: paper.title })"
          @click="openPaper(paper)"
          @keydown.alt.up.prevent="movePaperByKeyboard(paper.id, -1)"
          @keydown.alt.down.prevent="movePaperByKeyboard(paper.id, 1)"
        >
          <Check v-if="paper.kind === 'todo'" class="launcher-paper-icon" aria-hidden="true" />
          <PencilLine v-else class="launcher-paper-icon" aria-hidden="true" />
          <span class="launcher-paper-title">{{ compactPaperTitle(paper.title) }}</span>
        </button>
        <button
          type="button"
          class="launcher-paper-delete"
          draggable="false"
          :disabled="deletingPaperId !== null"
          :title="t('paperTodo.deletePaper')"
          :aria-label="t('paperTodo.deletePaper')"
          @mousedown.stop
          @click.stop="deletePaper(paper)"
        >
          <X aria-hidden="true" />
        </button>
      </li>
      <li v-if="paperCount === 0" class="launcher-empty">
        {{ t('paperTodo.launcher.empty') }}
      </li>
      <li class="launcher-create-actions">
        <button
          type="button"
          class="launcher-create-button"
          :disabled="creatingKind !== null"
          :title="t('paperTodo.newTodoPaper')"
          :aria-label="t('paperTodo.newTodoPaper')"
          @click="createPaper('todo')"
        >
          <ListPlus aria-hidden="true" />
        </button>
        <button
          type="button"
          class="launcher-create-button"
          :disabled="creatingKind !== null"
          :title="t('paperTodo.newNotePaper')"
          :aria-label="t('paperTodo.newNotePaper')"
          @click="createPaper('note')"
        >
          <FilePlus2 aria-hidden="true" />
        </button>
      </li>
    </ol>
  </div>
</template>

<style scoped>
.launcher-surface {
  display: flex;
  width: 100%;
  height: 100%;
  box-sizing: border-box;
  flex-direction: column;
  align-items: stretch;
  gap: 4px;
  overflow: hidden;
  padding-block: 3px;
  pointer-events: none;
  color: #4f493f;
  font-size: 12px;
}
.launcher-right { padding-left: 3px; padding-right: 0; }
.launcher-left { padding-left: 0; padding-right: 3px; }
.launcher-master-capsule,
.launcher-paper-capsule {
  display: flex;
  width: 100%;
  min-width: 0;
  align-items: center;
  border: 1px solid rgb(213 184 123 / 0.78);
  background: rgb(255 249 235 / 0.98);
  color: inherit;
  box-shadow: 0 1px 2px rgb(94 70 29 / 0.08);
  transition: background-color 160ms ease, border-color 160ms ease, opacity 160ms ease;
}
/* `max-content` rather than a fixed width: the capsule is the whole control
   now, so any slack past its label would be dead window hanging off the count.
   It also keeps the measured width honest while the native window is still the
   previous label's size. */
.launcher-master-capsule {
  /* Deliberately uncapped: `max-width` would clamp the measurement to the
     window the capsule is trying to outgrow, so a longer label could never
     report the width it needs. The surface clips the one frame of overflow. */
  width: max-content;
  height: 31px;
  flex: 0 0 31px;
  cursor: grab;
  gap: 3px;
  /* The flat side sits `LAUNCHER_EDGE_OVERHANG` past the screen border, so the
     padding on that side is inflated to keep the visible inset even. */
  padding: 0 14px 0 10px;
  border-radius: 13px 0 0 13px;
  font-weight: 500;
  white-space: nowrap;
  pointer-events: auto;
  text-align: left;
}
.launcher-master-capsule:active { cursor: grabbing; }
.launcher-left .launcher-master-capsule {
  padding: 0 10px 0 14px;
  border-radius: 0 13px 13px 0;
}
.launcher-master-capsule span {
  overflow: hidden;
  text-overflow: ellipsis;
}
.launcher-chevron {
  width: 11px;
  height: 11px;
  flex: 0 0 11px;
  stroke-width: 2.25;
}
.launcher-paper-list {
  display: flex;
  min-height: 0;
  margin: 0;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 4px;
  overflow-x: hidden;
  overflow-y: auto;
  padding: 0;
  list-style: none;
  scrollbar-width: none;
  pointer-events: none;
}
.launcher-right .launcher-master-capsule,
.launcher-right .launcher-paper-slot,
.launcher-right .launcher-create-actions,
.launcher-right .launcher-empty { align-self: flex-end; }
.launcher-left .launcher-master-capsule,
.launcher-left .launcher-paper-slot,
.launcher-left .launcher-create-actions,
.launcher-left .launcher-empty { align-self: flex-start; }
.launcher-paper-list::-webkit-scrollbar { display: none; }
.launcher-paper-slot {
  position: relative;
  display: flex;
  width: fit-content;
  max-width: 100%;
  height: 30px;
  flex: 0 0 30px;
  pointer-events: auto;
  transition: filter 180ms ease;
}
.launcher-paper-slot:hover,
.launcher-paper-slot:focus-within {
  z-index: 3;
  filter: drop-shadow(0 4px 7px rgb(72 52 20 / 0.16));
}
.launcher-paper-slot::before,
.launcher-paper-slot::after {
  position: absolute;
  z-index: 2;
  right: 8px;
  left: 8px;
  height: 2px;
  border-radius: 999px;
  background: #d69b36;
  content: '';
  opacity: 0;
  pointer-events: none;
}
.launcher-paper-slot::before { top: -3px; }
.launcher-paper-slot::after { bottom: -3px; }
.launcher-paper-slot.drop-before::before,
.launcher-paper-slot.drop-after::after { opacity: 1; }
.launcher-paper-capsule {
  width: auto;
  height: 30px;
  flex: 0 1 auto;
  cursor: grab;
  gap: 3px;
  padding: 0 7px 0 5px;
  border-radius: 12px;
  text-align: left;
}
.launcher-paper-slot:hover .launcher-paper-capsule,
.launcher-paper-slot:focus-within .launcher-paper-capsule {
  border-right: 0;
  border-radius: 12px 0 0 12px;
}
.launcher-paper-capsule:active { cursor: grabbing; }
.launcher-paper-delete {
  display: flex;
  width: 0;
  height: 30px;
  flex: 0 0 0;
  cursor: pointer;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  border: 0 solid rgb(213 184 123 / 0.78);
  border-radius: 0;
  background: rgb(255 249 235 / 0.98);
  color: #8a806f;
  opacity: 0;
  pointer-events: none;
  transition: width 180ms ease, flex-basis 180ms ease, opacity 140ms ease,
    background-color 160ms ease, border-color 160ms ease, color 160ms ease;
}
.launcher-paper-delete svg { width: 11px; height: 11px; }
.launcher-paper-slot:hover .launcher-paper-delete,
.launcher-paper-slot:focus-within .launcher-paper-delete {
  width: 20px;
  flex-basis: 20px;
  border-width: 1px;
  border-radius: 0 10px 10px 0;
  opacity: 1;
  pointer-events: auto;
}
.launcher-paper-delete:hover:not(:disabled) {
  border-color: rgb(225 120 105 / 0.9);
  background: #fff0ea;
  color: #b34b3e;
}
.launcher-paper-icon {
  width: 12px;
  height: 12px;
  flex: 0 0 12px;
  color: #8a806f;
  stroke-width: 2;
}
.launcher-paper-title {
  flex: 0 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.launcher-paper-slot.is-dragging { opacity: 0.38; }
.launcher-master-capsule:hover,
.launcher-paper-capsule:hover:not(:disabled) {
  border-color: rgb(196 153 70 / 0.92);
  background: #fff3d5;
}
.launcher-master-capsule:focus-visible,
.launcher-paper-capsule:focus-visible,
.launcher-paper-delete:focus-visible {
  outline: 2px solid rgb(14 165 233 / 0.72);
  outline-offset: -2px;
}
.launcher-paper-capsule:disabled,
.launcher-paper-delete:disabled { cursor: default; opacity: 0.55; }
/* Fixed height, not padding around a text run: the native window reserves a
   whole row for this state, and an intrinsic height would push the creation
   row past the bottom edge. */
.launcher-empty {
  display: flex;
  width: 96px;
  height: 26px;
  flex: 0 0 26px;
  align-items: center;
  justify-content: center;
  padding-inline: 6px;
  color: #756c5f;
}
.launcher-create-actions {
  display: flex;
  width: 96px;
  height: 30px;
  flex: 0 0 30px;
  gap: 4px;
  padding-inline: 3px;
  pointer-events: auto;
}
.launcher-create-button {
  display: flex;
  min-width: 0;
  flex: 1 1 0;
  cursor: pointer;
  align-items: center;
  justify-content: center;
  border: 1px solid rgb(213 184 123 / 0.78);
  border-radius: 9px;
  background: rgb(255 249 235 / 0.98);
  color: #75633f;
  transition: background-color 160ms ease, border-color 160ms ease, color 160ms ease;
}
.launcher-create-button svg { width: 14px; height: 14px; }
.launcher-create-button:hover:not(:disabled) {
  border-color: rgb(196 153 70 / 0.92);
  background: #fff3d5;
  color: #6b4b16;
}
.launcher-create-button:focus-visible {
  outline: 2px solid rgb(14 165 233 / 0.72);
  outline-offset: 1px;
}
.launcher-create-button:disabled { cursor: default; opacity: 0.45; }
.launcher-dark { color: #e7dfd2; }
.launcher-dark .launcher-master-capsule,
.launcher-dark .launcher-paper-capsule,
.launcher-dark .launcher-paper-delete {
  border-color: rgb(143 119 80 / 0.82);
  background: rgb(49 45 39 / 0.98);
}
.launcher-dark .launcher-master-capsule:hover,
.launcher-dark .launcher-paper-capsule:hover:not(:disabled) {
  border-color: rgb(190 154 90 / 0.92);
  background: #3d362b;
}
.launcher-dark .launcher-paper-icon { color: #c7bda9; }
.launcher-dark .launcher-paper-delete { color: #b8aa94; }
.launcher-dark .launcher-paper-delete:hover:not(:disabled) {
  border-color: rgb(190 100 88 / 0.92);
  background: #4a2f2a;
  color: #f0a89e;
}
.launcher-dark .launcher-create-button {
  border-color: rgb(143 119 80 / 0.82);
  background: rgb(49 45 39 / 0.98);
  color: #d8c8aa;
}
.launcher-dark .launcher-create-button:hover:not(:disabled) {
  border-color: rgb(190 154 90 / 0.92);
  background: #3d362b;
  color: #f1d79d;
}
@media (prefers-reduced-motion: reduce) {
  .launcher-master-capsule,
  .launcher-paper-slot,
  .launcher-paper-capsule,
  .launcher-paper-delete,
  .launcher-create-button { transition: none; }
}
</style>
