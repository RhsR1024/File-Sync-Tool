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
import { computed, nextTick, ref, type CSSProperties } from 'vue';
import { useI18n } from 'vue-i18n';
import { VueDraggable } from 'vue-draggable-plus';

import PaperTodoMarkdown from '@/components/paper-todo/PaperTodoMarkdown.vue';
import { usePaperTodo } from '@/composables/usePaperTodo';
import {
  createTodoItem,
  createDesktopPaper,
  importPaperImage,
  isPowerPaper,
  MAX_NOTE_LENGTH,
  openPaperNoteExternally,
  openPaperWindow,
  runPaperScript,
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

const paper = computed(() => store.state.value.papers.find((item) => item.id === props.paperId) ?? null);
const settings = computed(() => store.state.value.settings);
const notes = computed(() => store.state.value.papers.filter((item) => item.kind === 'note' && item.id !== props.paperId));
const completedCount = computed(() => paper.value?.items.filter((item) => item.completed).length ?? 0);
const isScript = computed(() => Boolean(paper.value && isPowerPaper(paper.value)));
const systemDark = typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches;
const useDarkTheme = computed(() => settings.value.theme === 'dark' || (settings.value.theme === 'system' && systemDark));
const paletteClass = computed(() => ({
  warm: useDarkTheme.value ? 'border-amber-800/70 bg-[#29251b] text-amber-50 shadow-black/30' : 'border-amber-200/90 bg-[#fffdf3] text-slate-800 shadow-amber-950/10',
  ink: useDarkTheme.value ? 'border-zinc-700 bg-zinc-900 text-zinc-100 shadow-black/30' : 'border-zinc-300 bg-zinc-100 text-zinc-900 shadow-zinc-950/10',
  forest: useDarkTheme.value ? 'border-emerald-900 bg-[#13241a] text-emerald-50 shadow-black/30' : 'border-emerald-200/90 bg-[#f3fbf5] text-emerald-950 shadow-emerald-950/10',
  frost: useDarkTheme.value ? 'border-sky-900 bg-[#14232d] text-sky-50 shadow-black/30' : 'border-sky-200/90 bg-[#f4faff] text-slate-800 shadow-sky-950/10',
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

async function launchScript(): Promise<void> {
  if (!paper.value || !window.confirm(t('paperTodo.confirmRunScript'))) return;
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
  update((value) => { value.desktopOpen = false; }, false, true);
  await store.flush();
  await getCurrentWindow().close();
}

async function applyWindowMode(collapsed: boolean): Promise<void> {
  if (!paper.value) return;
  update((value) => { value.collapsed = collapsed; }, false, true);
  if (props.standalone) {
    await invoke('paper_todo_set_window_mode', {
      id: paper.value.id,
      collapsed,
      pinned: paper.value.pinned,
      width: paper.value.geometry.width,
      height: paper.value.geometry.height,
    });
    if (collapsed && settings.value.autoDockCapsules) {
      await invoke('paper_todo_dock_window', {
        id: paper.value.id,
        edge: paper.value.geometry.dockEdge ?? 'nearest',
      });
    }
  }
}

async function togglePinned(): Promise<void> {
  if (!paper.value) return;
  update((value) => { value.pinned = !value.pinned; }, false, true);
  if (props.standalone) await applyWindowMode(paper.value.collapsed);
}

async function dock(edge: 'left' | 'right'): Promise<void> {
  if (!paper.value || !props.standalone) return;
  await invoke('paper_todo_dock_window', { id: paper.value.id, edge });
  update((value) => { value.geometry.dockEdge = edge; }, false, true);
}

async function confirmDeletePaper(): Promise<void> {
  if (!paper.value) return;
  const id = paper.value.id;
  deleteConfirmationOpen.value = false;
  await store.removePaper(id);
  emit('deleted', id);
}

function startWindowDrag(event: MouseEvent): void {
  if (!props.standalone || event.button !== 0) return;
  const target = event.target as HTMLElement;
  if (target.closest('button,input,textarea,select,a')) return;
  void getCurrentWindow().startDragging();
}
</script>

<template>
  <section
    v-if="paper"
    class="relative flex min-h-0 flex-col overflow-hidden border shadow-[0_18px_45px_var(--tw-shadow-color)]"
    :class="[
      paletteClass,
      standalone ? 'h-screen rounded-[7px]' : 'h-[520px] rounded-lg',
      !settings.animations && 'motion-reduce:transition-none',
    ]"
    :style="paperStyle"
  >
    <header
      class="flex min-h-12 shrink-0 items-center gap-1.5 border-b border-current/10 px-2.5"
      :class="standalone ? 'cursor-move select-none' : ''"
      @mousedown="startWindowDrag"
    >
      <button class="paper-icon-button" type="button" :title="paper.pinned ? t('paperTodo.unpin') : t('paperTodo.pin')" @click="togglePinned">
        <Pin v-if="paper.pinned" class="h-4 w-4" />
        <PinOff v-else class="h-4 w-4" />
      </button>
      <component :is="paper.kind === 'todo' ? StickyNote : FileText" class="h-4 w-4 shrink-0 opacity-65" aria-hidden="true" />
      <input
        :value="paper.title"
        class="min-w-0 flex-1 bg-transparent px-1 font-semibold outline-none focus-visible:ring-2 focus-visible:ring-sky-500/40"
        :class="[paper.collapsed && standalone ? capsuleTextClass : titleTextClass, (paper.collapsed && standalone ? settings.capsuleBold : settings.titleBold) && 'font-bold']"
        :maxlength="settings.titleMaxLength"
        :aria-label="t('paperTodo.paperTitle')"
        @input="changeTitle"
      >
      <span v-if="store.savingIds.value.has(paper.id)" class="h-1.5 w-1.5 animate-pulse rounded-full bg-sky-500 motion-reduce:animate-none" :title="t('paperTodo.saving')"></span>
      <button v-if="paper.kind === 'note' && settings.showExternalOpenButton" class="paper-icon-button" type="button" :title="t('paperTodo.openExternal')" @click="openExternal">
        <ExternalLink class="h-4 w-4" />
      </button>
      <button v-if="standalone && settings.showNewTodoButton" class="paper-icon-button" type="button" :title="t('paperTodo.newTodoPaper')" @click="createSiblingPaper('todo')">
        <Plus class="h-4 w-4" />
      </button>
      <button v-if="standalone && settings.showNewNoteButton" class="paper-icon-button" type="button" :title="t('paperTodo.newNotePaper')" @click="createSiblingPaper('note')">
        <FilePlus2 class="h-4 w-4" />
      </button>
      <button v-if="isScript" class="paper-icon-button text-amber-600" type="button" :title="t('paperTodo.runScript')" @click="launchScript">
        <Play class="h-4 w-4" />
      </button>
      <button v-if="!standalone" class="paper-icon-button" type="button" :title="t('paperTodo.openDesktop')" @click="openDesktop">
        <Maximize2 class="h-4 w-4" />
      </button>
      <button v-if="standalone && paper.collapsed" class="paper-icon-button" type="button" :title="t('paperTodo.expand')" @click="applyWindowMode(false)">
        <ChevronDown class="h-4 w-4" />
      </button>
      <button v-else-if="settings.capsuleMode" class="paper-icon-button" type="button" :title="t('paperTodo.collapse')" @click="applyWindowMode(true)">
        <ChevronUp class="h-4 w-4" />
      </button>
      <button v-if="standalone" class="paper-icon-button" type="button" :title="t('paperTodo.close')" @click="closeDesktop">
        <X class="h-4 w-4" />
      </button>
    </header>

    <template v-if="!paper.collapsed || !standalone">
      <div v-if="paper.kind === 'todo'" class="flex min-h-0 flex-1 flex-col">
        <div class="flex shrink-0 gap-2 border-b border-current/10 p-3">
          <input
            v-model="newTodoText"
            class="min-w-0 flex-1 rounded-md border border-current/15 bg-white/45 px-3 py-2 text-sm outline-none placeholder:opacity-45 focus:border-sky-500/60 focus:ring-2 focus:ring-sky-500/15 dark:bg-black/10"
            :placeholder="t('paperTodo.newTodoPlaceholder')"
            @keydown.enter.prevent="submitTodo"
            @paste="onTodoPaste"
          >
          <button class="paper-command-button" type="button" :disabled="!newTodoText.trim()" :title="t('paperTodo.addTodo')" @click="submitTodo">
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
              @change="update(() => {}, false, true)"
            >
            <select
              v-if="notes.length"
              v-model="item.linkedNoteId"
              class="mt-1 h-7 max-w-24 rounded border border-current/10 bg-transparent px-1 text-[11px] opacity-0 outline-none transition-opacity group-hover:opacity-70 focus:opacity-100"
              :title="t('paperTodo.linkNote')"
              @change="update(() => {}, true, true)"
            >
              <option :value="null">{{ t('paperTodo.noLinkedNote') }}</option>
              <option v-for="note in notes" :key="note.id" :value="note.id">{{ settings.showLinkedNoteTitle ? note.title : t('paperTodo.note') }}</option>
            </select>
            <button v-if="item.linkedNoteId" class="paper-icon-button mt-0.5" type="button" :title="t('paperTodo.openLinkedNote')" @click="openLinkedNote(item.linkedNoteId)">
              <ExternalLink class="h-3.5 w-3.5" />
            </button>
            <button class="paper-icon-button mt-0.5 opacity-0 group-hover:opacity-60 focus:opacity-100" type="button" :title="t('paperTodo.deleteTodo')" @click="deleteTodo(item.id)">
              <Trash2 class="h-3.5 w-3.5" />
            </button>
          </div>
          <div v-if="paper.items.length === 0" class="flex h-40 items-center justify-center text-sm opacity-40">{{ t('paperTodo.emptyTodo') }}</div>
        </VueDraggable>

        <footer class="flex min-h-11 shrink-0 items-center gap-1 border-t border-current/10 px-2">
          <span class="mr-auto text-xs opacity-50">{{ t('paperTodo.todoProgress', { done: completedCount, total: paper.items.length }) }}</span>
          <button class="paper-icon-button" type="button" :disabled="!store.canUndo(paper.id)" :title="t('paperTodo.undo')" @click="store.undoPaper(paper.id)"><Undo2 class="h-4 w-4" /></button>
          <button class="paper-icon-button" type="button" :disabled="!store.canRedo(paper.id)" :title="t('paperTodo.redo')" @click="store.redoPaper(paper.id)"><Redo2 class="h-4 w-4" /></button>
          <button class="paper-icon-button" type="button" :disabled="!completedCount" :title="t('paperTodo.clearCompleted')" @click="clearCompleted"><RotateCcw class="h-4 w-4" /></button>
          <button class="paper-icon-button text-rose-600" type="button" :title="t('paperTodo.deletePaper')" @click="openDeleteConfirmation"><Trash2 class="h-4 w-4" /></button>
        </footer>
      </div>

      <div v-else class="flex min-h-0 flex-1 flex-col">
        <div class="flex min-h-10 shrink-0 items-center gap-1 border-b border-current/10 px-2">
          <button class="paper-icon-button font-bold" type="button" :title="t('paperTodo.bold')" @click="formatSelection('**')"><Bold class="h-4 w-4" /></button>
          <button class="paper-icon-button italic" type="button" :title="t('paperTodo.italic')" @click="formatSelection('*')"><Italic class="h-4 w-4" /></button>
          <button class="paper-icon-button" type="button" :title="t('paperTodo.link')" @click="formatSelection('[', '](https://)')"><Link class="h-4 w-4" /></button>
          <button class="paper-icon-button" type="button" :disabled="pendingImage" :title="t('paperTodo.insertImage')" @click="insertImage('file')"><Image class="h-4 w-4" /></button>
          <span class="mx-1 h-4 w-px bg-current/15"></span>
          <button class="paper-text-button" type="button" :class="previewMode === 'edit' && 'bg-current/10'" @click="previewMode = 'edit'">{{ t('paperTodo.edit') }}</button>
          <button class="paper-text-button" type="button" :class="previewMode === 'split' && 'bg-current/10'" @click="previewMode = 'split'">{{ t('paperTodo.split') }}</button>
          <button class="paper-text-button" type="button" :class="previewMode === 'preview' && 'bg-current/10'" @click="previewMode = 'preview'">{{ t('paperTodo.preview') }}</button>
          <button class="paper-text-button ml-auto" type="button" :title="t('paperTodo.resetZoom')" @click="update(value => { value.zoom = 100; }, false, true)">{{ paper.zoom }}%</button>
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
          <button v-if="standalone && settings.autoDockCapsules" class="paper-icon-button" type="button" :title="t('paperTodo.dockLeft')" @click="dock('left')"><PanelLeftClose class="h-4 w-4" /></button>
          <button v-if="standalone && settings.autoDockCapsules" class="paper-icon-button" type="button" :title="t('paperTodo.dockRight')" @click="dock('right')"><PanelRightClose class="h-4 w-4" /></button>
          <button class="paper-icon-button text-rose-600" type="button" :title="t('paperTodo.deletePaper')" @click="openDeleteConfirmation"><Trash2 class="h-4 w-4" /></button>
        </footer>
      </div>
    </template>

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
  </section>
</template>

<style scoped>
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
  .paper-icon-button { transition: none; }
}
</style>
