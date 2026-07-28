<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import {
  Bold,
  CheckCheck,
  ChevronDown,
  ChevronUp,
  CirclePlus,
  ExternalLink,
  FilePlus2,
  FileText,
  GripVertical,
  Image,
  Italic,
  Link,
  Maximize2,
  PanelLeftClose,
  PanelRightClose,
  Pin,
  PinOff,
  Play,
  Plus,
  Redo2,
  RotateCcw,
  StickyNote,
  Trash2,
  Undo2,
  X,
} from 'lucide-vue-next';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, type CSSProperties } from 'vue';
import { useI18n } from 'vue-i18n';
import { VueDraggable } from 'vue-draggable-plus';

import PaperTodoMarkdown from '@/components/paper-todo/PaperTodoMarkdown.vue';
import { usePaperTodo } from '@/composables/usePaperTodo';
import {
  createTodoItem,
  createDesktopPaper,
  importPaperImage,
  isPaperEmpty,
  isPowerPaper,
  MAX_NOTE_LENGTH,
  openPaperNoteExternally,
  openPaperWindow,
  runPaperScript,
  setPaperEdgePeek,
  splitTodoPaste,
  type PaperDocument,
} from '@/lib/paperTodo';

const props = withDefaults(defineProps<{
  paperId: string;
  standalone?: boolean;
}>(), {
  standalone: false,
});

const emit = defineEmits<{ deleted: [id: string] }>();
const { t } = useI18n();
const store = usePaperTodo();
const newTodoText = ref('');
const noteTextarea = ref<HTMLTextAreaElement | null>(null);
const previewMode = ref<'edit' | 'split' | 'preview'>('edit');
const pendingImage = ref(false);
const deleteConfirmationOpen = ref(false);
const deleteDialog = ref<HTMLElement | null>(null);
const scriptConfirmationOpen = ref(false);
const scriptDialog = ref<HTMLElement | null>(null);
const capsuleHovered = ref(false);
const peekedAway = ref(false);
let peekTimer: ReturnType<typeof setTimeout> | null = null;

const paper = computed(() => store.state.value.papers.find((item) => item.id === props.paperId) ?? null);
const settings = computed(() => store.state.value.settings);
const notes = computed(() => store.state.value.papers.filter((item) => item.kind === 'note' && item.id !== props.paperId));
const completedCount = computed(() => paper.value?.items.filter((item) => item.completed).length ?? 0);
const isScript = computed(() => Boolean(paper.value && isPowerPaper(paper.value)));
const systemDark = typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches;
const useDarkTheme = computed(() => settings.value.theme === 'dark' || (settings.value.theme === 'system' && systemDark));
const paletteClass = computed(() => ({
  warm: useDarkTheme.value ? 'bg-[#29251b] text-amber-50' : 'bg-[#fffdf3] text-slate-800',
  ink: useDarkTheme.value ? 'bg-zinc-900 text-zinc-100' : 'bg-zinc-100 text-zinc-900',
  forest: useDarkTheme.value ? 'bg-[#13241a] text-emerald-50' : 'bg-[#f3fbf5] text-emerald-950',
  frost: useDarkTheme.value ? 'bg-[#14232d] text-sky-50' : 'bg-[#f4faff] text-slate-800',
}[settings.value.palette]));
const todoTextClass = computed(() => ({
  small: 'text-xs', medium: 'text-sm', large: 'text-base', xlarge: 'text-lg',
}[settings.value.todoFontSize]));
const noteTextClass = computed(() => ({
  small: 'text-xs', medium: 'text-sm', large: 'text-base', xlarge: 'text-lg',
}[settings.value.noteFontSize]));
const titleTextClass = computed(() => ({
  small: 'text-xs', medium: 'text-sm', large: 'text-base', xlarge: 'text-lg',
}[settings.value.titleFontSize]));
const capsuleTextClass = computed(() => ({
  small: 'text-xs', medium: 'text-sm', large: 'text-base', xlarge: 'text-lg',
}[settings.value.capsuleFontSize]));
// The capsule is the paper folded away, not a shrunken window: it draws its own
// compact surface instead of squeezing the expanded header into 216 px.
const isCapsule = computed(() => Boolean(props.standalone && paper.value?.collapsed));
const capsuleAccent = computed(() => ({
  warm: useDarkTheme.value ? '#d9a441' : '#b8791a',
  ink: useDarkTheme.value ? '#a1a1aa' : '#52525b',
  forest: useDarkTheme.value ? '#4ade80' : '#15803d',
  frost: useDarkTheme.value ? '#7dd3fc' : '#0369a1',
}[settings.value.palette]));
const capsuleDockSide = computed(() => paper.value?.geometry.dockEdge ?? 'right');
/// Share of the spine that is inked in: completed items for a todo paper, and
/// how full the note is for a note paper. Readable from the docked sliver alone.
const capsuleFill = computed(() => {
  const current = paper.value;
  if (!current) return 0;
  if (current.kind === 'todo') {
    return current.items.length ? completedCount.value / current.items.length : 0;
  }
  return current.content.length ? Math.min(1, current.content.length / 4_000) : 0;
});
const capsuleMeta = computed(() => {
  const current = paper.value;
  if (!current) return '';
  if (current.kind === 'todo') {
    return current.items.length ? `${completedCount.value}/${current.items.length}` : '';
  }
  const lines = current.content.trim() ? current.content.trim().split('\n').length : 0;
  return lines ? t('paperTodo.capsuleLines', { count: lines }) : '';
});
const canAutoHideCapsule = computed(() => Boolean(
  props.standalone
  && settings.value.autoDockCapsules
  && settings.value.autoHideDockedCapsules,
));

/** Tooltips are opt-in; screen-reader labels stay on the controls regardless. */
function tip(key: string): string | undefined {
  return settings.value.hoverTips ? t(key) : undefined;
}

const renderingStyle = computed<CSSProperties>(() => ({
  WebkitFontSmoothing: settings.value.textRendering === 'soft' ? 'antialiased' : 'auto',
  textRendering: settings.value.textRendering === 'sharp' ? 'geometricPrecision' : 'auto',
}));
const paperStyle = computed<CSSProperties>(() => ({
  ...renderingStyle.value,
  fontFamily: settings.value.fontFamily,
  fontSize: `${settings.value.interfaceScale}%`,
  colorScheme: useDarkTheme.value ? 'dark' : 'light',
}));
const capsuleStyle = computed<CSSProperties>(() => ({
  ...paperStyle.value,
  '--paper-capsule-accent': capsuleAccent.value,
} as CSSProperties));

function update(mutator: (value: PaperDocument) => void, history = false, immediate = false): void {
  store.updatePaper(props.paperId, mutator, { history, immediate });
}

function addTodos(lines: string[]): void {
  if (!lines.length) return;
  update((value) => value.items.push(...lines.map(createTodoItem)), true, true);
  newTodoText.value = '';
}

function submitTodo(): void {
  addTodos(splitTodoPaste(newTodoText.value));
}

function onTodoPaste(event: ClipboardEvent): void {
  const text = event.clipboardData?.getData('text/plain') ?? '';
  const lines = splitTodoPaste(text);
  if (lines.length <= 1) return;
  event.preventDefault();
  addTodos(lines);
}

function toggleTodo(id: string): void {
  update((value) => {
    const item = value.items.find((candidate) => candidate.id === id);
    if (!item) return;
    item.completed = !item.completed;
    if (item.completed && settings.value.autoClearCompleted) {
      value.items = value.items.filter((candidate) => candidate.id !== id);
    }
  }, true, true);
}

function deleteTodo(id: string): void {
  update((value) => { value.items = value.items.filter((item) => item.id !== id); }, true, true);
}

function clearCompleted(): void {
  if (!completedCount.value) return;
  update((value) => { value.items = value.items.filter((item) => !item.completed); }, true, true);
}

function onTodoListKeydown(event: KeyboardEvent): void {
  if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
  const key = event.key.toLowerCase();
  const target = event.target as HTMLElement | null;
  // Leave native caret undo/redo to text fields; list-level history is driven
  // from the footer buttons and these shortcuts only when focus is elsewhere.
  if (target && target.closest('input,textarea,select')) return;
  if (key === 'z' && store.canUndo(props.paperId)) {
    event.preventDefault();
    store.undoPaper(props.paperId);
  } else if ((key === 'y' || (key === 'z' && event.shiftKey)) && store.canRedo(props.paperId)) {
    event.preventDefault();
    store.redoPaper(props.paperId);
  }
}

function onSortStart(): void {
  update(() => {}, true);
}

function onSortEnd(): void {
  update(() => {}, false, true);
}

function changeTitle(event: Event): void {
  const title = (event.target as HTMLInputElement).value.slice(0, settings.value.titleMaxLength);
  update((value) => { value.title = title; });
}

function changeNote(event: Event): void {
  const content = (event.target as HTMLTextAreaElement).value.slice(0, MAX_NOTE_LENGTH);
  update((value) => { value.content = content; });
}

async function formatSelection(prefix: string, suffix = prefix): Promise<void> {
  const textarea = noteTextarea.value;
  if (!textarea || !paper.value) return;
  const start = textarea.selectionStart;
  const end = textarea.selectionEnd;
  const selected = paper.value.content.slice(start, end);
  update((value) => {
    value.content = `${value.content.slice(0, start)}${prefix}${selected}${suffix}${value.content.slice(end)}`;
  }, false, true);
  await nextTick();
  textarea.focus();
  textarea.setSelectionRange(start + prefix.length, end + prefix.length);
}

function onNoteKeydown(event: KeyboardEvent): void {
  if (event.key === 'Enter' && !event.shiftKey && paper.value && noteTextarea.value) {
    const textarea = noteTextarea.value;
    const start = textarea.selectionStart;
    const lineStart = paper.value.content.lastIndexOf('\n', start - 1) + 1;
    const currentLine = paper.value.content.slice(lineStart, start);
    const list = currentLine.match(/^(\s*)([-*+] |(\d+)[.)] |\[[ xX]\] )/);
    if (list) {
      event.preventDefault();
      const marker = list[3] ? `${Number(list[3]) + 1}. ` : list[2];
      const contentWithoutMarker = currentLine.slice(list[0].length);
      const insertion = contentWithoutMarker.trim() ? `\n${list[1]}${marker}` : '';
      update((value) => {
        const before = contentWithoutMarker.trim() ? value.content.slice(0, start) : value.content.slice(0, lineStart);
        value.content = `${before}${insertion}${value.content.slice(start)}`;
      }, false, true);
      void nextTick(() => {
        const cursor = (contentWithoutMarker.trim() ? start : lineStart) + insertion.length;
        textarea.setSelectionRange(cursor, cursor);
      });
      return;
    }
  }
  if (!(event.ctrlKey || event.metaKey)) return;
  const key = event.key.toLowerCase();
  if (key === 'b') {
    event.preventDefault();
    void formatSelection('**');
  } else if (key === 'i') {
    event.preventDefault();
    void formatSelection('*');
  } else if (key === 'k') {
    event.preventDefault();
    void formatSelection('[', '](https://)');
  }
}

function onNoteWheel(event: WheelEvent): void {
  if (!event.ctrlKey) return;
  event.preventDefault();
  update((value) => {
    value.zoom = Math.min(200, Math.max(50, value.zoom + (event.deltaY < 0 ? 10 : -10)));
  }, false, true);
}

async function insertImage(source: 'file' | 'clipboard'): Promise<void> {
  if (!paper.value || pendingImage.value) return;
  pendingImage.value = true;
  try {
    const asset = await importPaperImage(source, settings.value.autoCompressImages);
    if (!asset) return;
    const marker = `\n![image|100%](i:${asset.id})\n`;
    const textarea = noteTextarea.value;
    const position = textarea?.selectionStart ?? paper.value.content.length;
    update((value) => {
      value.content = `${value.content.slice(0, position)}${marker}${value.content.slice(position)}`;
    }, false, true);
  } catch (reason) {
    store.error.value = String(reason);
  } finally {
    pendingImage.value = false;
  }
}

function onNotePaste(event: ClipboardEvent): void {
  if ([...(event.clipboardData?.items ?? [])].some((item) => item.type.startsWith('image/'))) {
    event.preventDefault();
    void insertImage('clipboard');
  }
}

async function openScriptConfirmation(): Promise<void> {
  if (!paper.value) return;
  scriptConfirmationOpen.value = true;
  await nextTick();
  scriptDialog.value?.focus();
}

async function launchScript(): Promise<void> {
  scriptConfirmationOpen.value = false;
  if (!paper.value) return;
  try {
    await runPaperScript(paper.value, settings.value);
  } catch (reason) {
    store.error.value = String(reason);
  }
}

async function openExternal(): Promise<void> {
  if (!paper.value) return;
  try {
    await openPaperNoteExternally(paper.value, settings.value.externalExtension);
  } catch (reason) {
    store.error.value = String(reason);
  }
}

async function openDesktop(): Promise<void> {
  if (!paper.value) return;
  try {
    update((value) => { value.desktopOpen = true; }, false, true);
    await openPaperWindow(paper.value, settings.value);
  } catch (reason) {
    store.error.value = String(reason);
  }
}

async function createSiblingPaper(kind: 'todo' | 'note'): Promise<void> {
  try {
    await createDesktopPaper(kind);
    await store.refreshFromDisk();
  } catch (reason) {
    store.error.value = String(reason);
  }
}

async function openDeleteConfirmation(): Promise<void> {
  deleteConfirmationOpen.value = true;
  await nextTick();
  deleteDialog.value?.focus();
}

async function openLinkedNote(id: string | null): Promise<void> {
  if (!id) return;
  const note = store.state.value.papers.find((candidate) => candidate.id === id && candidate.kind === 'note');
  if (!note) return;
  store.updatePaper(note.id, (value) => { value.desktopOpen = true; }, { immediate: true });
  await openPaperWindow(note, settings.value);
}

async function closeDesktop(): Promise<void> {
  const current = paper.value;
  if (!current) return;
  if (isPaperEmpty(current)) {
    const id = current.id;
    await store.removePaper(id);
    emit('deleted', id);
    return;
  }
  update((value) => { value.desktopOpen = false; }, false, true);
  await store.flush();
  await getCurrentWindow().close();
}

async function saveCurrentPaper(): Promise<void> {
  if (!paper.value) return;
  update(() => {}, false, true);
  await store.flush();
}

function cancelPeekTimer(): void {
  if (peekTimer) clearTimeout(peekTimer);
  peekTimer = null;
}

async function setEdgePeek(peek: boolean): Promise<void> {
  const current = paper.value;
  if (!current || !current.collapsed || !canAutoHideCapsule.value) return;
  if (peekedAway.value === peek) return;
  peekedAway.value = peek;
  // The slide takes ~110 ms in Rust; hold move tracking a little longer so the
  // trailing frames cannot be mistaken for a user reposition.
  store.suspendGeometryTracking(400);
  try {
    await setPaperEdgePeek(
      current.id,
      current.geometry.dockEdge ?? 'nearest',
      peek,
      settings.value.animations,
    );
  } catch (reason) {
    peekedAway.value = !peek;
    store.error.value = String(reason);
  }
}

/** Rest the capsule against the edge once the pointer has clearly left it. */
function schedulePeekAway(delay = 900): void {
  cancelPeekTimer();
  if (!canAutoHideCapsule.value || capsuleHovered.value) return;
  peekTimer = setTimeout(() => {
    peekTimer = null;
    void setEdgePeek(true);
  }, delay);
}

function onCapsuleEnter(): void {
  capsuleHovered.value = true;
  cancelPeekTimer();
  void setEdgePeek(false);
}

function onCapsuleLeave(): void {
  capsuleHovered.value = false;
  schedulePeekAway(650);
}

async function dockCapsule(edge: 'left' | 'right' | 'nearest'): Promise<void> {
  const current = paper.value;
  if (!current || !props.standalone) return;
  store.suspendGeometryTracking(400);
  const resolved = await invoke<string>('paper_todo_dock_window', { id: current.id, edge });
  update((value) => { value.geometry.dockEdge = resolved === 'left' ? 'left' : 'right'; }, false, true);
}

async function applyWindowMode(collapsed: boolean): Promise<void> {
  if (!paper.value) return;
  cancelPeekTimer();
  peekedAway.value = false;
  update((value) => { value.collapsed = collapsed; }, false, true);
  if (props.standalone) {
    store.suspendGeometryTracking(400);
    await invoke('paper_todo_set_window_mode', {
      id: paper.value.id,
      collapsed,
      pinned: paper.value.pinned,
      width: paper.value.geometry.width,
      height: paper.value.geometry.height,
    });
    if (collapsed && settings.value.autoDockCapsules) {
      await dockCapsule(paper.value.geometry.dockEdge ?? 'nearest');
      schedulePeekAway();
    }
  }
}

async function togglePinned(): Promise<void> {
  if (!paper.value) return;
  update((value) => { value.pinned = !value.pinned; }, false, true);
  if (props.standalone) await applyWindowMode(paper.value.collapsed);
}

async function dock(edge: 'left' | 'right'): Promise<void> {
  await dockCapsule(edge);
}

async function confirmDeletePaper(): Promise<void> {
  if (!paper.value) return;
  const id = paper.value.id;
  deleteConfirmationOpen.value = false;
  await store.removePaper(id);
  emit('deleted', id);
}

async function startWindowDrag(event: MouseEvent, explicitHandle = false): Promise<void> {
  if (!props.standalone || event.button !== 0) return;
  const target = event.target as HTMLElement;
  if (!explicitHandle && target.closest('button,input,textarea,select,a')) return;
  try {
    await getCurrentWindow().startDragging();
    if (paper.value?.collapsed && settings.value.autoDockCapsules) {
      await dockCapsule('nearest');
    }
  } catch (reason) {
    store.error.value = String(reason);
  }
}

/**
 * The capsule has one press gesture: drag it to move, or press and release
 * without moving to open the paper. Buttons inside it keep their own clicks.
 */
async function startCapsuleDrag(event: MouseEvent): Promise<void> {
  const current = paper.value;
  if (!current || !props.standalone || event.button !== 0) return;
  if ((event.target as HTMLElement).closest('button')) return;
  cancelPeekTimer();
  const capsuleWindow = getCurrentWindow();
  try {
    const origin = await capsuleWindow.outerPosition();
    // `startDragging` resolves when the OS drag loop ends, so the position
    // afterwards tells us whether this press was a move or a plain click.
    await capsuleWindow.startDragging();
    const landed = await capsuleWindow.outerPosition();
    if (Math.abs(landed.x - origin.x) < 3 && Math.abs(landed.y - origin.y) < 3) {
      await applyWindowMode(false);
      return;
    }
    peekedAway.value = false;
    if (settings.value.autoDockCapsules) await dockCapsule('nearest');
    schedulePeekAway();
  } catch (reason) {
    store.error.value = String(reason);
  }
}

// Turning auto-hide off must not strand a capsule that is already resting off
// screen: it has no visible surface left to grab, so bring it back immediately.
watch(canAutoHideCapsule, (enabled) => {
  const current = paper.value;
  if (enabled || !peekedAway.value || !current) return;
  cancelPeekTimer();
  peekedAway.value = false;
  store.suspendGeometryTracking(400);
  setPaperEdgePeek(
    current.id,
    current.geometry.dockEdge ?? 'nearest',
    false,
    settings.value.animations,
  ).catch((reason) => { store.error.value = String(reason); });
});

/** Save explicitly with Ctrl+S; Escape folds an expanded desktop paper away. */
function onWindowKeydown(event: KeyboardEvent): void {
  if (!props.standalone) return;
  if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 's') {
    event.preventDefault();
    void saveCurrentPaper();
    return;
  }
  if (event.key !== 'Escape') return;
  if (!paper.value || paper.value.collapsed || !settings.value.capsuleMode) return;
  void applyWindowMode(true);
}

onMounted(() => {
  // A paper restored as a capsule should settle against its edge on its own.
  if (isCapsule.value) schedulePeekAway(1_400);
  if (props.standalone) window.addEventListener('keydown', onWindowKeydown);
});

onBeforeUnmount(() => {
  cancelPeekTimer();
  window.removeEventListener('keydown', onWindowKeydown);
});
</script>

<template>
  <div
    v-if="paper && isCapsule"
    class="paper-capsule"
    :class="[
      paletteClass,
      capsuleDockSide === 'left' ? 'is-docked-left' : 'is-docked-right',
      !settings.animations && 'paper-no-motion',
    ]"
    :style="capsuleStyle"
    :title="tip('paperTodo.capsuleHint')"
    @mouseenter="onCapsuleEnter"
    @mouseleave="onCapsuleLeave"
    @mousedown="startCapsuleDrag"
    @dblclick.prevent="applyWindowMode(false)"
    @contextmenu.prevent
  >
    <span class="paper-capsule-spine" aria-hidden="true">
      <span class="paper-capsule-spine-fill" :style="{ height: `${Math.round(capsuleFill * 100)}%` }"></span>
    </span>
    <component
      :is="paper.kind === 'todo' ? StickyNote : FileText"
      class="paper-capsule-icon"
      aria-hidden="true"
    />
    <span class="paper-capsule-title" :class="[capsuleTextClass, settings.capsuleBold && 'font-bold']">
      {{ paper.title }}
    </span>
    <span v-if="capsuleMeta" class="paper-capsule-meta">{{ capsuleMeta }}</span>
    <span class="paper-capsule-actions">
      <button
        type="button"
        class="paper-capsule-action"
        :title="tip('paperTodo.expand')"
        :aria-label="t('paperTodo.openCapsule', { title: paper.title })"
        @click.stop="applyWindowMode(false)"
      >
        <ChevronDown class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        class="paper-capsule-action"
        :title="tip('paperTodo.close')"
        :aria-label="t('paperTodo.close')"
        @click.stop="closeDesktop"
      >
        <X class="h-3.5 w-3.5" />
      </button>
    </span>
  </div>

  <section
    v-else-if="paper"
    class="paper-surface relative flex min-h-0 flex-col overflow-hidden"
    :class="[
      paletteClass,
      standalone ? 'h-screen rounded-xl' : 'h-[520px] rounded-xl ring-1 ring-current/10 shadow-lg',
      !settings.animations && 'paper-no-motion',
    ]"
    :style="paperStyle"
    @contextmenu.prevent
  >
    <header
      class="flex min-h-12 shrink-0 items-center gap-1.5 border-b border-current/10 px-2.5"
      :class="standalone ? 'cursor-move select-none' : ''"
      @mousedown="startWindowDrag"
    >
      <button
        class="paper-icon-button"
        type="button"
        :title="tip(paper.pinned ? 'paperTodo.unpin' : 'paperTodo.pin')"
        :aria-label="t(paper.pinned ? 'paperTodo.unpin' : 'paperTodo.pin')"
        :aria-pressed="paper.pinned"
        @click="togglePinned"
      >
        <Pin v-if="paper.pinned" class="h-4 w-4" />
        <PinOff v-else class="h-4 w-4" />
      </button>
      <button
        v-if="standalone"
        class="paper-window-drag-handle"
        type="button"
        :title="tip('paperTodo.moveWindow')"
        :aria-label="t('paperTodo.moveWindow')"
        @mousedown.stop.prevent="startWindowDrag($event, true)"
      >
        <GripVertical class="h-4 w-4" />
      </button>
      <component :is="paper.kind === 'todo' ? StickyNote : FileText" class="h-4 w-4 shrink-0 opacity-65" aria-hidden="true" />
      <input
        :value="paper.title"
        class="min-w-0 flex-1 bg-transparent px-1 font-semibold outline-none focus-visible:ring-2 focus-visible:ring-sky-500/40"
        :class="[titleTextClass, settings.titleBold && 'font-bold']"
        :maxlength="settings.titleMaxLength"
        :aria-label="t('paperTodo.paperTitle')"
        @input="changeTitle"
      >
      <span v-if="store.savingIds.value.has(paper.id)" class="h-1.5 w-1.5 animate-pulse rounded-full bg-sky-500 motion-reduce:animate-none" :title="tip('paperTodo.saving')"></span>
      <button v-if="paper.kind === 'note' && settings.showExternalOpenButton" class="paper-icon-button" type="button" :title="tip('paperTodo.openExternal')" :aria-label="t('paperTodo.openExternal')" @click="openExternal">
        <ExternalLink class="h-4 w-4" />
      </button>
      <button v-if="standalone && settings.showNewTodoButton" class="paper-icon-button" type="button" :title="tip('paperTodo.newTodoPaper')" :aria-label="t('paperTodo.newTodoPaper')" @click="createSiblingPaper('todo')">
        <Plus class="h-4 w-4" />
      </button>
      <button v-if="standalone && settings.showNewNoteButton" class="paper-icon-button" type="button" :title="tip('paperTodo.newNotePaper')" :aria-label="t('paperTodo.newNotePaper')" @click="createSiblingPaper('note')">
        <FilePlus2 class="h-4 w-4" />
      </button>
      <button v-if="isScript" class="paper-icon-button text-amber-600" type="button" :title="tip('paperTodo.runScript')" :aria-label="t('paperTodo.runScript')" @click="openScriptConfirmation">
        <Play class="h-4 w-4" />
      </button>
      <button v-if="!standalone" class="paper-icon-button" type="button" :title="tip('paperTodo.openDesktop')" :aria-label="t('paperTodo.openDesktop')" @click="openDesktop">
        <Maximize2 class="h-4 w-4" />
      </button>
      <button v-if="settings.capsuleMode" class="paper-icon-button" type="button" :title="tip('paperTodo.collapse')" :aria-label="t('paperTodo.collapse')" @click="applyWindowMode(true)">
        <ChevronUp class="h-4 w-4" />
      </button>
      <button v-if="standalone" class="paper-icon-button" type="button" :title="tip('paperTodo.close')" :aria-label="t('paperTodo.close')" @click="closeDesktop">
        <X class="h-4 w-4" />
      </button>
    </header>

    <div v-if="paper.kind === 'todo'" class="flex min-h-0 flex-1 flex-col" @keydown="onTodoListKeydown">
        <div class="flex shrink-0 gap-2 border-b border-current/10 p-3">
          <input
            v-model="newTodoText"
            class="min-w-0 flex-1 rounded-md border border-current/15 bg-white/45 px-3 py-2 text-sm outline-none placeholder:opacity-45 focus:border-sky-500/60 focus:ring-2 focus:ring-sky-500/15 dark:bg-black/10"
            :placeholder="t('paperTodo.newTodoPlaceholder')"
            @keydown.enter.prevent="submitTodo"
            @paste="onTodoPaste"
          >
          <button class="paper-command-button" type="button" :disabled="!newTodoText.trim()" :title="tip('paperTodo.addTodo')" :aria-label="t('paperTodo.addTodo')" @click="submitTodo">
            <CirclePlus class="h-4 w-4" />
          </button>
        </div>

        <VueDraggable
          v-model="paper.items"
          class="min-h-0 flex-1 overflow-y-auto px-2 py-2"
          handle=".paper-todo-drag-handle"
          :animation="settings.animations ? 150 : 0"
          @start="onSortStart"
          @end="onSortEnd"
        >
          <div v-for="item in paper.items" :key="item.id" class="group flex min-h-10 items-start gap-1 rounded-md px-1 py-1.5 hover:bg-current/5">
            <button class="paper-todo-drag-handle mt-0.5 flex h-8 w-6 cursor-grab items-center justify-center opacity-25 hover:opacity-70 active:cursor-grabbing" type="button" :aria-label="t('paperTodo.reorder')">
              <GripVertical class="h-4 w-4" />
            </button>
            <button
              type="button"
              class="mt-1 flex h-6 w-6 shrink-0 items-center justify-center rounded-full border border-current/25 transition-colors hover:border-emerald-500 hover:text-emerald-600 focus-visible:ring-2 focus-visible:ring-emerald-500/40"
              :class="item.completed ? 'bg-emerald-500 text-white border-emerald-500' : ''"
              :aria-label="item.completed ? t('paperTodo.markIncomplete') : t('paperTodo.markComplete')"
              @click="toggleTodo(item.id)"
            >
              <CheckCheck v-if="item.completed" class="h-3.5 w-3.5" />
            </button>
            <input
              v-model="item.text"
              class="mt-1 min-w-0 flex-1 bg-transparent leading-6 outline-none focus-visible:ring-2 focus-visible:ring-sky-500/30"
              :class="[todoTextClass, settings.todoBold && 'font-semibold', item.completed && 'line-through opacity-45']"
              maxlength="2000"
              @input="update(() => {}, false)"
              @change="update(() => {}, false, true)"
            >
            <select
              v-if="notes.length"
              v-model="item.linkedNoteId"
              class="mt-1 h-7 max-w-24 rounded border border-current/10 bg-transparent px-1 text-[11px] opacity-0 outline-none transition-opacity group-hover:opacity-70 focus:opacity-100"
              :title="tip('paperTodo.linkNote')" :aria-label="t('paperTodo.linkNote')"
              @change="update(() => {}, true, true)"
            >
              <option :value="null">{{ t('paperTodo.noLinkedNote') }}</option>
              <option v-for="note in notes" :key="note.id" :value="note.id">{{ settings.showLinkedNoteTitle ? note.title : t('paperTodo.note') }}</option>
            </select>
            <button v-if="item.linkedNoteId" class="paper-icon-button mt-0.5" type="button" :title="tip('paperTodo.openLinkedNote')" :aria-label="t('paperTodo.openLinkedNote')" @click="openLinkedNote(item.linkedNoteId)">
              <ExternalLink class="h-3.5 w-3.5" />
            </button>
            <button class="paper-icon-button mt-0.5 opacity-35 group-hover:opacity-70 focus:opacity-100" type="button" :title="tip('paperTodo.deleteTodo')" :aria-label="t('paperTodo.deleteTodo')" @click="deleteTodo(item.id)">
              <Trash2 class="h-3.5 w-3.5" />
            </button>
          </div>
          <div v-if="paper.items.length === 0" class="flex h-40 items-center justify-center text-sm opacity-40">{{ t('paperTodo.emptyTodo') }}</div>
        </VueDraggable>

        <footer class="flex min-h-11 shrink-0 items-center gap-1 border-t border-current/10 px-2">
          <span class="mr-auto text-xs opacity-50">{{ t('paperTodo.todoProgress', { done: completedCount, total: paper.items.length }) }}</span>
          <button class="paper-icon-button" type="button" :disabled="!store.canUndo(paper.id)" :title="tip('paperTodo.undo')" :aria-label="t('paperTodo.undo')" @click="store.undoPaper(paper.id)"><Undo2 class="h-4 w-4" /></button>
          <button class="paper-icon-button" type="button" :disabled="!store.canRedo(paper.id)" :title="tip('paperTodo.redo')" :aria-label="t('paperTodo.redo')" @click="store.redoPaper(paper.id)"><Redo2 class="h-4 w-4" /></button>
          <button class="paper-icon-button" type="button" :disabled="!completedCount" :title="tip('paperTodo.clearCompleted')" :aria-label="t('paperTodo.clearCompleted')" @click="clearCompleted"><RotateCcw class="h-4 w-4" /></button>
          <button class="paper-icon-button text-rose-600" type="button" :title="tip('paperTodo.deletePaper')" :aria-label="t('paperTodo.deletePaper')" @click="openDeleteConfirmation"><Trash2 class="h-4 w-4" /></button>
        </footer>
    </div>

    <div v-else class="flex min-h-0 flex-1 flex-col">
        <div class="flex min-h-10 shrink-0 items-center gap-1 border-b border-current/10 px-2">
          <button class="paper-icon-button font-bold" type="button" :title="tip('paperTodo.bold')" :aria-label="t('paperTodo.bold')" @click="formatSelection('**')"><Bold class="h-4 w-4" /></button>
          <button class="paper-icon-button italic" type="button" :title="tip('paperTodo.italic')" :aria-label="t('paperTodo.italic')" @click="formatSelection('*')"><Italic class="h-4 w-4" /></button>
          <button class="paper-icon-button" type="button" :title="tip('paperTodo.link')" :aria-label="t('paperTodo.link')" @click="formatSelection('[', '](https://)')"><Link class="h-4 w-4" /></button>
          <button class="paper-icon-button" type="button" :disabled="pendingImage" :title="tip('paperTodo.insertImage')" :aria-label="t('paperTodo.insertImage')" @click="insertImage('file')"><Image class="h-4 w-4" /></button>
          <span class="mx-1 h-4 w-px bg-current/15"></span>
          <button class="paper-text-button" type="button" :class="previewMode === 'edit' && 'bg-current/10'" @click="previewMode = 'edit'">{{ t('paperTodo.edit') }}</button>
          <button class="paper-text-button" type="button" :class="previewMode === 'split' && 'bg-current/10'" @click="previewMode = 'split'">{{ t('paperTodo.split') }}</button>
          <button class="paper-text-button" type="button" :class="previewMode === 'preview' && 'bg-current/10'" @click="previewMode = 'preview'">{{ t('paperTodo.preview') }}</button>
          <button class="paper-text-button ml-auto" type="button" :title="tip('paperTodo.resetZoom')" :aria-label="t('paperTodo.resetZoom')" @click="update(value => { value.zoom = 100; }, false, true)">{{ paper.zoom }}%</button>
        </div>
        <div class="grid min-h-0 flex-1" :class="previewMode === 'split' ? 'grid-cols-2' : 'grid-cols-1'">
          <textarea
            v-if="previewMode !== 'preview'"
            ref="noteTextarea"
            :value="paper.content"
            class="min-h-0 w-full resize-none bg-transparent p-3 font-mono leading-6 outline-none placeholder:opacity-40"
            :class="[noteTextClass, settings.noteBold && 'font-semibold', previewMode === 'split' && 'border-r border-current/10']"
            :style="{ fontSize: `${paper.zoom}%` }"
            :placeholder="t('paperTodo.notePlaceholder')"
            maxlength="500000"
            @input="changeNote"
            @keydown="onNoteKeydown"
            @wheel="onNoteWheel"
            @paste="onNotePaste"
          ></textarea>
          <div v-if="previewMode !== 'edit'" class="min-h-0 overflow-y-auto p-3 leading-6" :class="noteTextClass" :style="{ fontSize: `${paper.zoom}%` }">
            <PaperTodoMarkdown :content="paper.content" />
          </div>
        </div>
        <footer class="flex min-h-11 shrink-0 items-center border-t border-current/10 px-2">
          <span class="mr-auto text-xs opacity-45">{{ paper.content.length.toLocaleString() }} / {{ MAX_NOTE_LENGTH.toLocaleString() }}</span>
          <button v-if="standalone && settings.autoDockCapsules" class="paper-icon-button" type="button" :title="tip('paperTodo.dockLeft')" :aria-label="t('paperTodo.dockLeft')" @click="dock('left')"><PanelLeftClose class="h-4 w-4" /></button>
          <button v-if="standalone && settings.autoDockCapsules" class="paper-icon-button" type="button" :title="tip('paperTodo.dockRight')" :aria-label="t('paperTodo.dockRight')" @click="dock('right')"><PanelRightClose class="h-4 w-4" /></button>
          <button class="paper-icon-button text-rose-600" type="button" :title="tip('paperTodo.deletePaper')" :aria-label="t('paperTodo.deletePaper')" @click="openDeleteConfirmation"><Trash2 class="h-4 w-4" /></button>
        </footer>
    </div>

    <div
      v-if="deleteConfirmationOpen"
      ref="deleteDialog"
      class="absolute inset-0 z-20 flex items-center justify-center bg-slate-950/25 p-5 backdrop-blur-[1px]"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      :aria-label="t('paperTodo.deletePaper')"
      @keydown.esc.stop="deleteConfirmationOpen = false"
    >
      <div class="w-full max-w-72 rounded-md border border-current/15 bg-white p-4 text-slate-800 shadow-xl dark:bg-zinc-900 dark:text-zinc-100">
        <p class="text-sm leading-6">{{ t('paperTodo.confirmDeletePaper') }}</p>
        <div class="mt-4 flex justify-end gap-2">
          <button type="button" class="paper-confirm-button" autofocus @click="deleteConfirmationOpen = false">{{ t('common.cancel') }}</button>
          <button type="button" class="paper-confirm-button border-rose-300 text-rose-700 hover:bg-rose-50 dark:text-rose-300" @click="confirmDeletePaper">{{ t('paperTodo.deletePaper') }}</button>
        </div>
      </div>
    </div>

    <div
      v-if="scriptConfirmationOpen"
      ref="scriptDialog"
      class="absolute inset-0 z-20 flex items-center justify-center bg-slate-950/25 p-5 backdrop-blur-[1px]"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      :aria-label="t('paperTodo.runScript')"
      @keydown.esc.stop="scriptConfirmationOpen = false"
    >
      <div class="w-full max-w-72 rounded-md border border-current/15 bg-white p-4 text-slate-800 shadow-xl dark:bg-zinc-900 dark:text-zinc-100">
        <p class="text-sm leading-6">{{ t('paperTodo.confirmRunScript') }}</p>
        <div class="mt-4 flex justify-end gap-2">
          <button type="button" class="paper-confirm-button" autofocus @click="scriptConfirmationOpen = false">{{ t('common.cancel') }}</button>
          <button type="button" class="paper-confirm-button border-amber-300 text-amber-700 hover:bg-amber-50 dark:text-amber-300" @click="launchScript">{{ t('paperTodo.runScript') }}</button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.paper-surface {
  /* Clip the WebView-composited surface itself so antialiasing cannot leave a
     square dark fringe around the transparent native window. */
  border: 0;
  clip-path: inset(0 round 0.75rem);
}

/*
 * The capsule reads as a sheet of paper resting on its edge rather than a
 * shrunken window. Its one deliberate flourish is the spine: the palette-inked
 * bar along the docked side. The spine is the binding of the paper, the
 * progress meter (filled by completed items), and — because auto-hide slides
 * the pill off the display until only this bar shows — the handle you grab to
 * bring it back. One element, three jobs, so nothing else needs decorating.
 */
.paper-capsule {
  position: relative;
  display: flex;
  width: 100%;
  height: 100%;
  align-items: center;
  gap: 7px;
  overflow: hidden;
  border-width: 1px;
  border-style: solid;
  border-radius: 999px;
  box-shadow: 0 2px 6px rgb(15 23 42 / 0.2);
  cursor: grab;
  user-select: none;
}
.paper-capsule:active { cursor: grabbing; }
.paper-capsule.is-docked-left { padding: 0 13px 0 16px; }
.paper-capsule.is-docked-right { padding: 0 16px 0 13px; }
.paper-capsule-spine {
  position: absolute;
  top: 5px;
  bottom: 5px;
  display: flex;
  width: 3px;
  flex-direction: column;
  justify-content: flex-end;
  overflow: hidden;
  border-radius: 999px;
  background: color-mix(in srgb, var(--paper-capsule-accent) 24%, transparent);
}
/* Stays within the sliver the window leaves on screen while parked. */
.is-docked-left .paper-capsule-spine { left: 5px; }
.is-docked-right .paper-capsule-spine { right: 5px; }
.paper-capsule-spine-fill {
  width: 100%;
  border-radius: 999px;
  background: var(--paper-capsule-accent);
  transition: height 240ms cubic-bezier(0.22, 1, 0.36, 1);
}
.paper-capsule-icon {
  width: 13px;
  height: 13px;
  flex: 0 0 13px;
  opacity: 0.5;
}
.paper-capsule-title {
  min-width: 0;
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  letter-spacing: 0.01em;
}
.paper-capsule-meta {
  flex: 0 0 auto;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  opacity: 0.48;
  transition: opacity 150ms ease;
}
/* Actions overlay the count instead of displacing it, so the resting capsule
   never reflows when the pointer arrives. */
.paper-capsule-actions {
  position: absolute;
  top: 0;
  bottom: 0;
  display: flex;
  align-items: center;
  gap: 1px;
  opacity: 0;
  pointer-events: none;
  transition: opacity 150ms ease;
}
.is-docked-left .paper-capsule-actions { right: 9px; }
.is-docked-right .paper-capsule-actions { right: 12px; }
.paper-capsule:hover .paper-capsule-meta,
.paper-capsule:focus-within .paper-capsule-meta { opacity: 0; }
.paper-capsule:hover .paper-capsule-actions,
.paper-capsule:focus-within .paper-capsule-actions {
  opacity: 1;
  pointer-events: auto;
}
.paper-capsule-action {
  display: inline-flex;
  width: 20px;
  height: 20px;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  cursor: pointer;
  opacity: 0.62;
  transition: background-color 140ms ease, opacity 140ms ease;
}
.paper-capsule-action:hover { background: rgb(100 116 139 / 0.2); opacity: 1; }
.paper-capsule-action:focus-visible {
  outline: 2px solid rgb(14 165 233 / 0.6);
  outline-offset: 1px;
  opacity: 1;
}
/* "Animations off" is a setting, not just an OS preference. */
.paper-no-motion,
.paper-no-motion :deep(*) {
  transition-duration: 0ms !important;
  animation-duration: 0ms !important;
}
.paper-icon-button {
  display: inline-flex;
  width: 2rem;
  height: 2rem;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  border-radius: 0.375rem;
  cursor: pointer;
  transition: background-color 160ms ease, color 160ms ease, opacity 160ms ease;
}
.paper-window-drag-handle {
  display: inline-flex;
  width: 1.25rem;
  height: 2rem;
  flex: 0 0 1.25rem;
  cursor: move;
  align-items: center;
  justify-content: center;
  border-radius: 0.25rem;
  opacity: 0.4;
}
.paper-window-drag-handle:hover { background: rgb(100 116 139 / 0.12); opacity: 0.85; }
.paper-window-drag-handle:focus-visible { outline: 2px solid rgb(14 165 233 / 0.55); outline-offset: 1px; opacity: 1; }
.paper-icon-button:hover:not(:disabled) { background: rgb(100 116 139 / 0.12); }
.paper-icon-button:focus-visible { outline: 2px solid rgb(14 165 233 / 0.55); outline-offset: 1px; }
.paper-icon-button:disabled { cursor: default; opacity: 0.25; }
.paper-command-button {
  display: inline-flex;
  width: 2.5rem;
  height: 2.5rem;
  align-items: center;
  justify-content: center;
  border-radius: 0.375rem;
  background: rgb(15 23 42 / 0.9);
  color: white;
  cursor: pointer;
}
.paper-command-button:disabled { cursor: default; opacity: 0.3; }
.paper-text-button {
  min-height: 1.75rem;
  border-radius: 0.3rem;
  padding: 0 0.45rem;
  font-size: 0.7rem;
  font-weight: 600;
  cursor: pointer;
}
.paper-confirm-button {
  min-height: 2.25rem;
  cursor: pointer;
  border: 1px solid rgb(203 213 225);
  border-radius: 0.375rem;
  padding: 0 0.75rem;
  font-size: 0.75rem;
  font-weight: 600;
}
.paper-confirm-button:hover { background: rgb(248 250 252 / 0.85); }
.paper-confirm-button:focus-visible { outline: 2px solid rgb(14 165 233 / 0.55); outline-offset: 2px; }
@media (prefers-reduced-motion: reduce) {
  .paper-icon-button,
  .paper-capsule-action,
  .paper-capsule-meta,
  .paper-capsule-actions,
  .paper-capsule-spine-fill { transition: none; }
}
</style>
