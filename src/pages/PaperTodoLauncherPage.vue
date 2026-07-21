<script setup lang="ts">
import {
  ChevronLeft,
  ChevronRight,
  Eye,
  EyeOff,
  FilePlus2,
  Plus,
  Settings2,
  StickyNote,
} from 'lucide-vue-next';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import { usePaperTodo } from '@/composables/usePaperTodo';
import {
  createDesktopPaper,
  openPaperTodoSettings,
  savePaperLauncherPosition,
  setAllPaperWindows,
  setPaperLauncherExpanded,
  type PaperKind,
} from '@/lib/paperTodo';

defineOptions({ name: 'PaperTodoLauncherPage' });

const { t } = useI18n();
const store = usePaperTodo();
const expanded = ref(false);
const busy = ref(false);
let collapseTimer: ReturnType<typeof setTimeout> | null = null;
let dragSaveTimer: ReturnType<typeof setTimeout> | null = null;
let dragArmTimer: ReturnType<typeof setTimeout> | null = null;
let unlistenMoved: (() => void) | null = null;
let dragArmed = false;
let lastDragMoveAt = 0;

const edge = computed(() => store.settings.value.launcherEdge);
const paperCount = computed(() => store.papers.value.length);

function cancelCollapse(): void {
  if (collapseTimer) clearTimeout(collapseTimer);
  collapseTimer = null;
}

async function setExpanded(value: boolean): Promise<void> {
  cancelCollapse();
  expanded.value = value;
  await setPaperLauncherExpanded(value);
}

function toggleExpanded(): void {
  if (Date.now() - lastDragMoveAt < 450) return;
  void setExpanded(!expanded.value);
}

function startLauncherDrag(event: MouseEvent): void {
  if (event.button !== 0) return;
  dragArmed = true;
  if (dragArmTimer) clearTimeout(dragArmTimer);
  dragArmTimer = setTimeout(() => {
    dragArmed = false;
    dragArmTimer = null;
  }, 1_500);
  void getCurrentWindow().startDragging().catch((reason) => {
    dragArmed = false;
    store.error.value = String(reason);
  });
}

function scheduleLauncherPositionSave(): void {
  if (!dragArmed) return;
  lastDragMoveAt = Date.now();
  if (dragSaveTimer) clearTimeout(dragSaveTimer);
  dragSaveTimer = setTimeout(async () => {
    dragArmed = false;
    dragSaveTimer = null;
    try {
      await savePaperLauncherPosition();
      await store.refreshFromDisk();
    } catch (reason) {
      store.error.value = String(reason);
    }
  }, 300);
}

function scheduleCollapse(): void {
  cancelCollapse();
  collapseTimer = setTimeout(() => void setExpanded(false), 550);
}

async function createPaper(kind: PaperKind): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  try {
    await createDesktopPaper(kind);
    await store.refreshFromDisk();
    await setExpanded(false);
  } catch (reason) {
    store.error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

async function setWindows(action: 'show' | 'hide'): Promise<void> {
  await setAllPaperWindows(action);
  await setExpanded(false);
}

async function openSettings(): Promise<void> {
  await openPaperTodoSettings();
  await setExpanded(false);
}

onMounted(async () => {
  await store.initialize();
  unlistenMoved = await getCurrentWindow().onMoved(scheduleLauncherPositionSave);
});
onBeforeUnmount(() => {
  cancelCollapse();
  if (dragSaveTimer) clearTimeout(dragSaveTimer);
  if (dragArmTimer) clearTimeout(dragArmTimer);
  unlistenMoved?.();
});
</script>

<template>
  <div
    class="flex h-screen w-screen items-stretch overflow-hidden bg-transparent p-1"
    :class="edge === 'left' ? 'justify-end' : 'justify-start'"
    @mouseenter="cancelCollapse"
    @mouseleave="scheduleCollapse"
  >
    <div
      class="flex h-full w-full items-center overflow-hidden rounded-md border border-slate-700/80 bg-[#111827] text-slate-100 shadow-[0_10px_28px_rgba(2,6,23,0.38)]"
    >
      <button
        type="button"
        class="launcher-handle"
        :class="edge === 'left' ? 'order-2' : 'order-1'"
        :title="t('paperTodo.launcher.moveHint')"
        :aria-label="expanded ? t('paperTodo.launcher.collapse') : t('paperTodo.launcher.expand')"
        @mousedown="startLauncherDrag"
        @click="toggleExpanded"
      >
        <ChevronRight v-if="expanded && edge === 'left'" class="h-3.5 w-3.5" />
        <ChevronLeft v-else-if="expanded" class="h-3.5 w-3.5" />
        <StickyNote v-else class="h-4 w-4 text-amber-300" />
        <span v-if="!expanded" class="text-[10px] font-semibold text-slate-300">{{ paperCount }}</span>
      </button>

      <div
        class="flex min-w-0 flex-1 items-center gap-1 px-1.5"
        :class="edge === 'left' ? 'order-1 justify-end' : 'order-2 justify-start'"
        :aria-hidden="!expanded"
      >
        <button type="button" class="launcher-action" :disabled="busy" :title="t('paperTodo.newTodoPaper')" :aria-label="t('paperTodo.newTodoPaper')" @click="createPaper('todo')">
          <Plus class="h-4 w-4" />
        </button>
        <button type="button" class="launcher-action" :disabled="busy" :title="t('paperTodo.newNotePaper')" :aria-label="t('paperTodo.newNotePaper')" @click="createPaper('note')">
          <FilePlus2 class="h-4 w-4" />
        </button>
        <span class="mx-0.5 h-5 w-px bg-slate-700"></span>
        <button type="button" class="launcher-action" :title="t('paperTodo.showAll')" :aria-label="t('paperTodo.showAll')" @click="setWindows('show')"><Eye class="h-4 w-4" /></button>
        <button type="button" class="launcher-action" :title="t('paperTodo.hideAll')" :aria-label="t('paperTodo.hideAll')" @click="setWindows('hide')"><EyeOff class="h-4 w-4" /></button>
        <button type="button" class="launcher-action" :title="t('paperTodo.settingsLabel')" :aria-label="t('paperTodo.settingsLabel')" @click="openSettings"><Settings2 class="h-4 w-4" /></button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.launcher-handle {
  display: inline-flex;
  width: 2.5rem;
  height: 100%;
  flex: 0 0 2.5rem;
  cursor: grab;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.1rem;
  border-color: rgb(71 85 105 / 0.7);
  background: rgb(30 41 59 / 0.95);
}
.launcher-handle:active { cursor: grabbing; }
.launcher-action {
  display: inline-flex;
  width: 2rem;
  height: 2rem;
  flex: 0 0 2rem;
  cursor: pointer;
  align-items: center;
  justify-content: center;
  border-radius: 5px;
  color: rgb(203 213 225);
  transition: background-color 160ms ease, color 160ms ease;
}
.launcher-action:hover:not(:disabled), .launcher-handle:hover { background: rgb(51 65 85); color: white; }
.launcher-action:focus-visible, .launcher-handle:focus-visible { outline: 2px solid rgb(56 189 248 / 0.75); outline-offset: -2px; }
.launcher-action:disabled { cursor: default; opacity: 0.4; }
@media (prefers-reduced-motion: reduce) {
  .launcher-action { transition: none; }
}
</style>
