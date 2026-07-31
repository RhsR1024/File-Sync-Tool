<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  Bold,
  CheckCheck,
  ChevronDown,
  ChevronUp,
  CirclePlus,
  ExternalLink,
  Eye,
  FilePlus2,
  FileText,
  GripVertical,
  Image,
  Italic,
  Link,
  List,
  ListPlus,
  Maximize2,
  Minus,
  MoreHorizontal,
  PanelLeftClose,
  Pin,
  PinOff,
  Play,
  Redo2,
  RotateCcw,
  StickyNote,
  Trash2,
  Undo2,
} from 'lucide-vue-next';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, type CSSProperties } from 'vue';
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
  setPaperWindowPinned,
  splitTodoPaste,
  type PaperDocument,
  type PaperTodoItem,
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
const newTodoInput = ref<HTMLInputElement | null>(null);
const noteTextarea = ref<HTMLTextAreaElement | null>(null);
const previewMode = ref<'edit' | 'split' | 'preview'>('edit');
const pendingImage = ref(false);
const deleteConfirmationOpen = ref(false);
const deletingPaper = ref(false);
const deleteDialog = ref<HTMLElement | null>(null);
const scriptConfirmationOpen = ref(false);
const scriptDialog = ref<HTMLElement | null>(null);
const overflowMenuOpen = ref(false);
const overflowMenu = ref<HTMLElement | null>(null);
const overflowTrigger = ref<HTMLButtonElement | null>(null);
const linkMenuItemId = ref<string | null>(null);
const linkMenu = ref<HTMLElement[]>([]);
const linkMenuTrigger = ref<HTMLElement | null>(null);
const todoFilter = ref<'all' | 'active'>('all');
const completedGroupOpen = ref(false);
const compactToolbarVisible = ref(false);
const outlineOpen = ref(true);
const outlineWidth = ref(96);

const paper = computed(() => store.state.value.papers.find((item) => item.id === props.paperId) ?? null);
const settings = computed(() => store.state.value.settings);
const notes = computed(() => store.state.value.papers.filter((item) => item.kind === 'note' && item.id !== props.paperId));
const completedCount = computed(() => paper.value?.items.filter((item) => item.completed).length ?? 0);
const todoProgress = computed(() => {
  const total = paper.value?.items.length ?? 0;
  return total ? Math.round((completedCount.value / total) * 100) : 0;
});
const noteHeadings = computed(() => {
  if (paper.value?.kind !== 'note') return [];
  let offset = 0;
  return paper.value.content.split('\n').flatMap((line) => {
    const match = line.match(/^(#{1,3})\s+(.+)$/);
    const entry = match ? [{ level: match[1].length, label: match[2].trim(), offset }] : [];
    offset += line.length + 1;
    return entry;
  }).slice(0, 24);
});
const paperHeaderMeta = computed(() => {
  const current = paper.value;
  if (!current) return '';
  const status = store.savingIds.value.has(current.id) ? t('paperTodo.saving') : t('paperTodo.saved');
  if (paperSkin.value === 'quiet') {
    return current.kind === 'todo'
      ? t('paperTodo.quietTodoMeta', { done: completedCount.value, total: current.items.length, status })
      : t('paperTodo.noteStatus', { count: current.content.length.toLocaleString(), status });
  }
  if (current.kind === 'todo') return `${completedCount.value}/${current.items.length}`;
  const lines = current.content.trim() ? current.content.trim().split('\n').length : 0;
  return t('paperTodo.capsuleLines', { count: lines });
});
const isScript = computed(() => Boolean(paper.value && isPowerPaper(paper.value)));
const systemThemeQuery = typeof window !== 'undefined' ? window.matchMedia('(prefers-color-scheme: dark)') : null;
const systemDark = ref(systemThemeQuery?.matches ?? false);
const useDarkTheme = computed(() => settings.value.theme === 'dark' || (settings.value.theme === 'system' && systemDark.value));
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
const visualSizeIndex = { small: 0, medium: 1, large: 2, xlarge: 3 } as const;
const paletteAccent = computed(() => ({
  warm: useDarkTheme.value ? '#d9a441' : '#b8791a',
  ink: useDarkTheme.value ? '#a1a1aa' : '#52525b',
  forest: useDarkTheme.value ? '#4ade80' : '#15803d',
  frost: useDarkTheme.value ? '#7dd3fc' : '#0369a1',
}[settings.value.palette]));
const paperSkin = computed(() => settings.value.paperSkin);
const skinVars = computed<CSSProperties>(() => {
  const dark = useDarkTheme.value;
  const tokens = {
    classic: {
      base: dark ? '#29251b' : '#fffdf3', ink: dark ? '#fff7df' : '#1e293b',
      muted: dark ? '#c7bfae' : '#64748b', hairline: dark ? 'rgba(255,255,255,.12)' : 'rgba(30,41,59,.10)',
      radius: '12px', header: '48px', row: '40px', footer: '44px',
    },
    grain: {
      base: dark ? '#2a251b' : '#fdf8e7', ink: dark ? '#ece2cd' : '#3d3527',
      muted: dark ? '#b9aa8e' : '#786b54', hairline: dark ? 'rgba(200,166,96,.28)' : 'rgba(150,116,52,.32)',
      radius: '14px', header: '42px', row: '36px', footer: '34px',
    },
    quiet: {
      base: dark ? '#17191d' : '#fcfcf9', ink: dark ? '#e7e9ec' : '#23262b',
      muted: dark ? '#7c848f' : '#737b86', hairline: 'transparent',
      radius: '16px', header: '72px', row: '38px', footer: '44px',
    },
    desk: {
      base: dark ? '#1b2027' : '#fffdf6', ink: dark ? '#e5e9ef' : '#1f2530',
      muted: dark ? '#939ca8' : '#697381', hairline: dark ? 'rgba(148,163,184,.18)' : 'rgba(15,23,42,.09)',
      radius: '10px', header: '40px', row: '31px', footer: '36px',
    },
  }[paperSkin.value];
  const sizeSets = {
    classic: { title: [12, 14, 16, 18], todo: [12, 14, 16, 18], note: [12, 14, 16, 18] },
    grain: { title: [12.5, 14, 16, 18], todo: [12.5, 13, 15, 17], note: [12.5, 13.5, 15.5, 17.5] },
    quiet: { title: [14, 17, 19, 21], todo: [12.5, 13.5, 15.5, 17.5], note: [12.5, 13.5, 15.5, 17.5] },
    desk: { title: [12.5, 13.5, 15, 17], todo: [12.5, 12.5, 14, 16], note: [12.5, 13, 14.5, 16] },
  }[paperSkin.value];
  return {
    '--paper-base': tokens.base,
    '--paper-ink': tokens.ink,
    '--paper-muted': tokens.muted,
    '--paper-hairline': tokens.hairline,
    '--paper-tint': paletteAccent.value,
    '--paper-radius': tokens.radius,
    '--paper-header-h': tokens.header,
    '--paper-row-h': tokens.row,
    '--paper-footer-h': tokens.footer,
    '--paper-title-font-size': `${sizeSets.title[visualSizeIndex[settings.value.titleFontSize]]}px`,
    '--paper-todo-font-size': `${sizeSets.todo[visualSizeIndex[settings.value.todoFontSize]]}px`,
    '--paper-note-font-size': `${sizeSets.note[visualSizeIndex[settings.value.noteFontSize]]}px`,
  } as CSSProperties;
});

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
  ...skinVars.value,
  fontFamily: settings.value.fontFamily,
  fontSize: `${settings.value.interfaceScale}%`,
  colorScheme: useDarkTheme.value ? 'dark' : 'light',
  ...(paperSkin.value === 'classic' ? {} : {
    background: 'color-mix(in srgb, var(--paper-tint) 4%, var(--paper-base))',
    color: 'var(--paper-ink)',
  }),
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
  store.error.value = '';
  deleteConfirmationOpen.value = true;
  await nextTick();
  deleteDialog.value?.focus();
}

function trapDeleteDialogFocus(event: KeyboardEvent): void {
  const focusable = Array.from(
    deleteDialog.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [],
  );
  if (!focusable.length) return;
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

async function openLinkedNote(id: string | null): Promise<void> {
  if (!id) return;
  const note = store.state.value.papers.find((candidate) => candidate.id === id && candidate.kind === 'note');
  if (!note) return;
  store.updatePaper(note.id, (value) => { value.desktopOpen = true; }, { immediate: true });
  await openPaperWindow(note, settings.value);
}

async function minimizeToLauncher(): Promise<void> {
  const current = paper.value;
  if (!current) return;
  update((value) => { value.desktopOpen = false; }, false, true);
  await store.flush();
  await getCurrentWindow().close();
}

function todoPriorityClass(item: PaperTodoItem): string {
  const active = paper.value?.items.filter((candidate) => !candidate.completed) ?? [];
  const index = active.findIndex((candidate) => candidate.id === item.id);
  if (index === 0) return 'is-high';
  if (index === 1) return 'is-medium';
  return 'is-none';
}

function linkedNoteTitle(id: string | null): string {
  if (!id) return t('paperTodo.noLinkedNote');
  return notes.value.find((note) => note.id === id)?.title ?? t('paperTodo.note');
}

function closeLinkMenu(restoreFocus = false): void {
  linkMenuItemId.value = null;
  if (restoreFocus) void nextTick(() => linkMenuTrigger.value?.focus());
}

async function toggleLinkMenu(id: string, event: MouseEvent): Promise<void> {
  if (linkMenuItemId.value === id) {
    closeLinkMenu();
    return;
  }
  linkMenuItemId.value = id;
  linkMenuTrigger.value = event.currentTarget as HTMLElement;
  await nextTick();
  linkMenu.value[0]?.querySelector<HTMLButtonElement>('button')?.focus();
}

function selectLinkedNote(itemId: string, noteId: string | null): void {
  update((value) => {
    const item = value.items.find((candidate) => candidate.id === itemId);
    if (item) item.linkedNoteId = noteId;
  }, true, true);
  closeLinkMenu(true);
}

function onLinkMenuKeydown(event: KeyboardEvent): void {
  const buttons = [...(linkMenu.value[0]?.querySelectorAll<HTMLButtonElement>('button') ?? [])];
  if (!buttons.length) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    closeLinkMenu(true);
    return;
  }
  const index = Math.max(0, buttons.indexOf(document.activeElement as HTMLButtonElement));
  let next = -1;
  if (event.key === 'ArrowDown') next = (index + 1) % buttons.length;
  if (event.key === 'ArrowUp') next = (index - 1 + buttons.length) % buttons.length;
  if (event.key === 'Home') next = 0;
  if (event.key === 'End') next = buttons.length - 1;
  if (event.key === 'Tab') next = event.shiftKey ? (index - 1 + buttons.length) % buttons.length : (index + 1) % buttons.length;
  if (next < 0) return;
  event.preventDefault();
  buttons[next].focus();
}

function closeOverflowMenu(restoreFocus = false): void {
  overflowMenuOpen.value = false;
  if (restoreFocus) void nextTick(() => overflowTrigger.value?.focus());
}

async function toggleOverflowMenu(): Promise<void> {
  overflowMenuOpen.value = !overflowMenuOpen.value;
  if (!overflowMenuOpen.value) return;
  await nextTick();
  overflowMenu.value?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
}

function onOverflowMenuKeydown(event: KeyboardEvent): void {
  const buttons = [...(overflowMenu.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
  if (!buttons.length) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    closeOverflowMenu(true);
    return;
  }
  const index = Math.max(0, buttons.indexOf(document.activeElement as HTMLButtonElement));
  let next = -1;
  if (event.key === 'ArrowDown') next = (index + 1) % buttons.length;
  if (event.key === 'ArrowUp') next = (index - 1 + buttons.length) % buttons.length;
  if (event.key === 'Home') next = 0;
  if (event.key === 'End') next = buttons.length - 1;
  if (event.key === 'Tab') next = event.shiftKey ? (index - 1 + buttons.length) % buttons.length : (index + 1) % buttons.length;
  if (next < 0) return;
  event.preventDefault();
  buttons[next].focus();
}

function onDocumentPointerDown(event: PointerEvent): void {
  const target = event.target as Node;
  if (overflowMenuOpen.value && !overflowMenu.value?.contains(target) && !overflowTrigger.value?.contains(target)) {
    closeOverflowMenu();
  }
  if (linkMenuItemId.value && !linkMenu.value.some((element) => element.contains(target)) && !linkMenuTrigger.value?.contains(target)) {
    closeLinkMenu();
  }
}

function focusNoteHeading(offset: number): void {
  previewMode.value = 'edit';
  void nextTick(() => {
    noteTextarea.value?.focus();
    noteTextarea.value?.setSelectionRange(offset, offset);
  });
}

let outlineResizeStartX = 0;
let outlineResizeStartWidth = 96;

function onOutlineResize(event: PointerEvent): void {
  outlineWidth.value = Math.min(156, Math.max(72, outlineResizeStartWidth + event.clientX - outlineResizeStartX));
}

function stopOutlineResize(): void {
  window.removeEventListener('pointermove', onOutlineResize);
  window.removeEventListener('pointerup', stopOutlineResize);
}

function startOutlineResize(event: PointerEvent): void {
  event.preventDefault();
  outlineResizeStartX = event.clientX;
  outlineResizeStartWidth = outlineWidth.value;
  window.addEventListener('pointermove', onOutlineResize);
  window.addEventListener('pointerup', stopOutlineResize, { once: true });
}

async function saveCurrentPaper(): Promise<void> {
  if (!paper.value) return;
  update(() => {}, false, true);
  await store.flush();
}

async function togglePinned(): Promise<void> {
  if (!paper.value) return;
  update((value) => { value.pinned = !value.pinned; }, false, true);
  if (props.standalone) await setPaperWindowPinned(paper.value.id, paper.value.pinned);
}

async function confirmDeletePaper(): Promise<void> {
  if (!paper.value || deletingPaper.value) return;
  const id = paper.value.id;
  deletingPaper.value = true;
  try {
    await store.removePaper(id);
    if (store.state.value.papers.some((candidate) => candidate.id === id)) return;
    deleteConfirmationOpen.value = false;
    emit('deleted', id);
  } finally {
    deletingPaper.value = false;
  }
}

async function startWindowDrag(event: MouseEvent, explicitHandle = false): Promise<void> {
  if (!props.standalone || event.button !== 0) return;
  const target = event.target as HTMLElement;
  if (!explicitHandle && target.closest('button,input,textarea,select,a')) return;
  try {
    await getCurrentWindow().startDragging();
  } catch (reason) {
    store.error.value = String(reason);
  }
}

/** Save explicitly with Ctrl+S; Escape returns a desktop paper to the launcher. */
function onWindowKeydown(event: KeyboardEvent): void {
  if (!props.standalone) return;
  if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 's') {
    event.preventDefault();
    void saveCurrentPaper();
    return;
  }
  if (event.key !== 'Escape') return;
  if (overflowMenuOpen.value) {
    event.preventDefault();
    closeOverflowMenu(true);
    return;
  }
  if (!paper.value) return;
  event.preventDefault();
  void minimizeToLauncher();
}

function onSystemThemeChange(event: MediaQueryListEvent): void {
  systemDark.value = event.matches;
}

onMounted(() => {
  if (props.standalone) window.addEventListener('keydown', onWindowKeydown);
  document.addEventListener('pointerdown', onDocumentPointerDown);
  systemThemeQuery?.addEventListener('change', onSystemThemeChange);
});

onBeforeUnmount(() => {
  stopOutlineResize();
  window.removeEventListener('keydown', onWindowKeydown);
  document.removeEventListener('pointerdown', onDocumentPointerDown);
  systemThemeQuery?.removeEventListener('change', onSystemThemeChange);
});
</script>

<template>
  <section
    v-if="paper"
    class="paper-surface relative flex min-h-0 flex-col overflow-hidden"
    :class="[
      paletteClass,
      `paper-skin-${paperSkin}`,
      `paper-kind-${paper.kind}`,
      standalone ? 'h-screen' : 'h-[520px] ring-1 ring-current/10 shadow-lg',
      !settings.animations && 'paper-no-motion',
    ]"
    :style="paperStyle"
    @contextmenu.prevent
  >
    <header
      class="paper-header flex shrink-0 items-center gap-1.5 px-2.5"
      :class="standalone ? 'cursor-move select-none' : ''"
      @mousedown="startWindowDrag"
    >
      <button
        class="paper-icon-button paper-pin-command"
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
      <span class="paper-kind-badge">
        <component :is="paper.kind === 'todo' ? StickyNote : FileText" class="paper-kind-icon h-4 w-4 shrink-0" aria-hidden="true" />
        <span class="paper-kind-label">{{ t(paper.kind === 'todo' ? 'paperTodo.todoKind' : 'paperTodo.noteKind') }}</span>
      </span>
      <input
        :value="paper.title"
        class="paper-title-input min-w-0 flex-1 bg-transparent px-1 font-semibold outline-none focus-visible:ring-2 focus-visible:ring-sky-500/40"
        :class="[titleTextClass, settings.titleBold && 'font-bold']"
        :maxlength="settings.titleMaxLength"
        :aria-label="t('paperTodo.paperTitle')"
        @input="changeTitle"
      >
      <span class="paper-header-meta">
        {{ paperHeaderMeta }}
      </span>
      <span v-if="store.savingIds.value.has(paper.id)" class="h-1.5 w-1.5 animate-pulse rounded-full bg-sky-500 motion-reduce:animate-none" :title="tip('paperTodo.saving')"></span>
      <button v-if="paper.kind === 'note' && settings.showExternalOpenButton" class="paper-icon-button paper-header-extra" type="button" :title="tip('paperTodo.openExternal')" :aria-label="t('paperTodo.openExternal')" @click="openExternal">
        <ExternalLink class="h-4 w-4" />
      </button>
      <button v-if="standalone && settings.showNewTodoButton" class="paper-icon-button paper-header-extra" type="button" :title="tip('paperTodo.newTodoPaper')" :aria-label="t('paperTodo.newTodoPaper')" @click="createSiblingPaper('todo')">
        <ListPlus class="h-4 w-4" />
      </button>
      <button v-if="standalone && settings.showNewNoteButton" class="paper-icon-button paper-header-extra" type="button" :title="tip('paperTodo.newNotePaper')" :aria-label="t('paperTodo.newNotePaper')" @click="createSiblingPaper('note')">
        <FilePlus2 class="h-4 w-4" />
      </button>
      <button v-if="isScript" class="paper-icon-button paper-header-extra text-amber-600" type="button" :title="tip('paperTodo.runScript')" :aria-label="t('paperTodo.runScript')" @click="openScriptConfirmation">
        <Play class="h-4 w-4" />
      </button>
      <button v-if="!standalone" class="paper-icon-button paper-header-extra" type="button" :title="tip('paperTodo.openDesktop')" :aria-label="t('paperTodo.openDesktop')" @click="openDesktop">
        <Maximize2 class="h-4 w-4" />
      </button>
      <div v-if="paperSkin !== 'classic'" class="paper-overflow-wrap">
        <button
          ref="overflowTrigger"
          class="paper-icon-button"
          type="button"
          :title="tip('paperTodo.more')"
          :aria-label="t('paperTodo.more')"
          :aria-expanded="overflowMenuOpen"
          aria-haspopup="menu"
          @click="toggleOverflowMenu"
        >
          <MoreHorizontal class="h-4 w-4" />
        </button>
        <div
          v-if="overflowMenuOpen"
          ref="overflowMenu"
          class="paper-overflow-menu"
          role="menu"
          @keydown="onOverflowMenuKeydown"
        >
          <button type="button" role="menuitem" @click="closeOverflowMenu(); togglePinned()"><Pin class="h-4 w-4" />{{ t(paper.pinned ? 'paperTodo.unpin' : 'paperTodo.pin') }}</button>
          <button v-if="standalone && settings.showNewTodoButton" type="button" role="menuitem" @click="closeOverflowMenu(); createSiblingPaper('todo')"><ListPlus class="h-4 w-4" />{{ t('paperTodo.newTodoPaper') }}</button>
          <button v-if="standalone && settings.showNewNoteButton" type="button" role="menuitem" @click="closeOverflowMenu(); createSiblingPaper('note')"><FilePlus2 class="h-4 w-4" />{{ t('paperTodo.newNotePaper') }}</button>
          <button v-if="paper.kind === 'note' && settings.showExternalOpenButton" type="button" role="menuitem" @click="closeOverflowMenu(); openExternal()"><ExternalLink class="h-4 w-4" />{{ t('paperTodo.openExternal') }}</button>
          <button v-if="isScript" type="button" role="menuitem" @click="closeOverflowMenu(); openScriptConfirmation()"><Play class="h-4 w-4" />{{ t('paperTodo.runScript') }}</button>
          <button v-if="paper.kind === 'note' && paperSkin !== 'desk'" type="button" role="menuitem" @click="compactToolbarVisible = !compactToolbarVisible; closeOverflowMenu()"><Bold class="h-4 w-4" />{{ t(compactToolbarVisible ? 'paperTodo.hideToolbar' : 'paperTodo.showToolbar') }}</button>
          <button v-if="paper.kind === 'note' && paperSkin === 'grain'" type="button" role="menuitem" @click="previewMode = 'split'; closeOverflowMenu()"><PanelLeftClose class="h-4 w-4" />{{ t('paperTodo.split') }}</button>
          <button v-if="paper.kind === 'todo'" type="button" role="menuitem" :disabled="!store.canUndo(paper.id)" @click="closeOverflowMenu(); store.undoPaper(paper.id)"><Undo2 class="h-4 w-4" />{{ t('paperTodo.undo') }}</button>
          <button v-if="paper.kind === 'todo'" type="button" role="menuitem" :disabled="!store.canRedo(paper.id)" @click="closeOverflowMenu(); store.redoPaper(paper.id)"><Redo2 class="h-4 w-4" />{{ t('paperTodo.redo') }}</button>
          <button v-if="paper.kind === 'todo'" type="button" role="menuitem" :disabled="!completedCount" @click="closeOverflowMenu(); clearCompleted()"><RotateCcw class="h-4 w-4" />{{ t('paperTodo.clearCompleted') }}</button>
        </div>
      </div>
      <button v-if="standalone" class="paper-icon-button text-rose-600" type="button" :title="tip('paperTodo.deletePaper')" :aria-label="t('paperTodo.deletePaper')" @click="openDeleteConfirmation">
        <Trash2 class="h-4 w-4" />
      </button>
      <button v-if="standalone" class="paper-icon-button" type="button" :title="tip('paperTodo.minimizeToLauncher')" :aria-label="t('paperTodo.minimizeToLauncher')" @click="minimizeToLauncher">
        <Minus class="h-4 w-4" />
      </button>
    </header>

    <div v-if="paper.kind === 'todo'" class="flex min-h-0 flex-1 flex-col" @keydown="onTodoListKeydown">
        <div class="paper-todo-entry flex shrink-0 gap-2">
          <input
            ref="newTodoInput"
            v-model="newTodoText"
            class="paper-todo-entry-input min-w-0 flex-1 rounded-md border border-current/15 bg-white/45 px-3 py-2 text-sm outline-none placeholder:opacity-45 focus:border-sky-500/60 focus:ring-2 focus:ring-sky-500/15 dark:bg-black/10"
            :placeholder="t(paperSkin === 'classic' ? 'paperTodo.newTodoPlaceholder' : paperSkin === 'quiet' ? 'paperTodo.addTodoQuiet' : 'paperTodo.addTodoInline')"
            @keydown.enter.prevent="submitTodo"
            @paste="onTodoPaste"
          >
          <button class="paper-command-button" type="button" :disabled="paperSkin !== 'quiet' && !newTodoText.trim()" :title="tip('paperTodo.addTodo')" :aria-label="t('paperTodo.addTodo')" @click="newTodoText.trim() ? submitTodo() : newTodoInput?.focus()">
            <CirclePlus class="h-4 w-4" />
          </button>
          <span class="paper-quiet-enter-hint">{{ t('paperTodo.enterHint') }}</span>
          <div class="paper-todo-filters" role="group" :aria-label="t('paperTodo.todoFilter')">
            <button type="button" :class="todoFilter === 'all' && 'is-active'" @click="todoFilter = 'all'">{{ t('paperTodo.filterAll') }}</button>
            <button type="button" :class="todoFilter === 'active' && 'is-active'" @click="todoFilter = 'active'">{{ t('paperTodo.filterActive') }}</button>
          </div>
        </div>

        <VueDraggable
          v-model="paper.items"
          class="paper-todo-list min-h-0 flex-1 overflow-y-auto px-2 py-2"
          handle=".paper-todo-drag-handle"
          draggable=".paper-todo-row"
          :animation="settings.animations ? 150 : 0"
          @start="onSortStart"
          @end="onSortEnd"
        >
          <div
            v-for="item in paper.items"
            v-show="(todoFilter === 'all' || !item.completed) && !(paperSkin === 'desk' && item.completed && !completedGroupOpen)"
            :key="item.id"
            class="paper-todo-row group relative flex items-start gap-1 rounded-md px-1 hover:bg-current/5"
            :class="item.completed && 'is-completed'"
          >
            <button class="paper-todo-drag-handle flex h-8 w-6 cursor-grab items-center justify-center opacity-25 hover:opacity-70 active:cursor-grabbing" type="button" :aria-label="t('paperTodo.reorder')">
              <GripVertical class="h-4 w-4" />
            </button>
            <button
              type="button"
              class="paper-todo-checkbox flex h-6 w-6 shrink-0 items-center justify-center rounded-full border border-current/25 transition-colors hover:border-emerald-500 hover:text-emerald-600 focus-visible:ring-2 focus-visible:ring-emerald-500/40"
              :class="item.completed ? 'bg-emerald-500 text-white border-emerald-500' : ''"
              :aria-label="item.completed ? t('paperTodo.markIncomplete') : t('paperTodo.markComplete')"
              @click="toggleTodo(item.id)"
            >
              <CheckCheck v-if="item.completed" class="h-3.5 w-3.5" />
            </button>
            <span class="paper-priority-mark" :class="todoPriorityClass(item)" aria-hidden="true"></span>
            <input
              v-model="item.text"
              class="paper-todo-text min-w-0 flex-1 bg-transparent leading-6 outline-none focus-visible:ring-2 focus-visible:ring-sky-500/30"
              :class="[todoTextClass, settings.todoBold && 'font-semibold', item.completed && 'line-through opacity-45']"
              maxlength="2000"
              @input="update(() => {}, false)"
              @change="update(() => {}, false, true)"
            >
            <select
              v-if="notes.length && paperSkin === 'classic'"
              v-model="item.linkedNoteId"
              class="paper-link-select h-7 max-w-24 rounded border border-current/10 bg-transparent px-1 text-[11px] opacity-0 outline-none transition-opacity group-hover:opacity-70 focus:opacity-100"
              :title="tip('paperTodo.linkNote')" :aria-label="t('paperTodo.linkNote')"
              @change="update(() => {}, true, true)"
            >
              <option :value="null">{{ t('paperTodo.noLinkedNote') }}</option>
              <option v-for="note in notes" :key="note.id" :value="note.id">{{ settings.showLinkedNoteTitle ? note.title : t('paperTodo.note') }}</option>
            </select>
            <div v-if="notes.length && paperSkin !== 'classic'" class="paper-link-wrap">
              <button
                type="button"
                class="paper-link-trigger"
                :class="item.linkedNoteId && 'is-linked'"
                :title="tip('paperTodo.linkNote')"
                :aria-label="t('paperTodo.linkNote')"
                :aria-expanded="linkMenuItemId === item.id"
                aria-haspopup="menu"
                @click="toggleLinkMenu(item.id, $event)"
              >
                <FileText v-if="paperSkin === 'desk' && item.linkedNoteId" class="h-3 w-3" />
                <Link v-else class="h-3 w-3" />
                <span v-if="paperSkin === 'desk' && item.linkedNoteId">{{ linkedNoteTitle(item.linkedNoteId) }}</span>
              </button>
              <div
                v-if="linkMenuItemId === item.id"
                ref="linkMenu"
                class="paper-link-menu"
                role="menu"
                :aria-label="t('paperTodo.linkNote')"
                @keydown="onLinkMenuKeydown"
              >
                <button type="button" role="menuitemradio" :aria-checked="!item.linkedNoteId" @click="selectLinkedNote(item.id, null)">{{ t('paperTodo.noLinkedNote') }}</button>
                <button v-for="note in notes" :key="note.id" type="button" role="menuitemradio" :aria-checked="item.linkedNoteId === note.id" @click="selectLinkedNote(item.id, note.id)">{{ settings.showLinkedNoteTitle ? note.title : t('paperTodo.note') }}</button>
              </div>
            </div>
            <button v-if="item.linkedNoteId" class="paper-icon-button paper-row-action" type="button" :title="tip('paperTodo.openLinkedNote')" :aria-label="t('paperTodo.openLinkedNote')" @click="openLinkedNote(item.linkedNoteId)">
              <ExternalLink class="h-3.5 w-3.5" />
            </button>
            <button class="paper-icon-button paper-row-action opacity-35 group-hover:opacity-70 focus:opacity-100" type="button" :title="tip('paperTodo.deleteTodo')" :aria-label="t('paperTodo.deleteTodo')" @click="deleteTodo(item.id)">
              <Trash2 class="h-3.5 w-3.5" />
            </button>
          </div>
          <div
            v-if="completedCount && (paperSkin === 'quiet' || paperSkin === 'desk') && todoFilter === 'all'"
            class="paper-completed-group"
          >
            <button
              v-if="paperSkin === 'desk'"
              type="button"
              :aria-expanded="completedGroupOpen"
              @click="completedGroupOpen = !completedGroupOpen"
            >
              <ChevronDown v-if="completedGroupOpen" class="h-3.5 w-3.5" />
              <ChevronUp v-else class="h-3.5 w-3.5 -rotate-90" />
              <span>{{ t('paperTodo.completedGroup', { count: completedCount }) }}</span>
            </button>
            <span v-else>{{ t('paperTodo.completedGroup', { count: completedCount }) }}</span>
            <button v-if="paperSkin === 'desk'" type="button" class="paper-completed-clear" @click="clearCompleted">{{ t('paperTodo.clearCompletedShort') }}</button>
          </div>
          <div v-if="paper.items.length === 0" class="flex h-40 items-center justify-center text-sm opacity-40">{{ t('paperTodo.emptyTodo') }}</div>
        </VueDraggable>

        <div v-if="paperSkin === 'grain'" class="paper-progress-track" aria-hidden="true"><span :style="{ width: `${todoProgress}%` }"></span></div>
        <footer class="paper-todo-footer flex shrink-0 items-center gap-1 px-2">
          <span class="paper-footer-status mr-auto text-xs opacity-50">{{ t('paperTodo.todoProgress', { done: completedCount, total: paper.items.length }) }}</span>
          <span class="paper-grain-footer-hint">{{ t('paperTodo.todoKeyboardHint') }}</span>
          <span class="paper-desk-progress" :aria-label="t('paperTodo.todoProgress', { done: completedCount, total: paper.items.length })">
            <i><b :style="{ width: `${todoProgress}%` }"></b></i><em>{{ todoProgress }}%</em>
          </span>
          <button class="paper-icon-button paper-footer-icon" type="button" :disabled="!store.canUndo(paper.id)" :title="tip('paperTodo.undo')" :aria-label="t('paperTodo.undo')" @click="store.undoPaper(paper.id)"><Undo2 class="h-4 w-4" /></button>
          <button class="paper-icon-button paper-footer-icon" type="button" :disabled="!store.canRedo(paper.id)" :title="tip('paperTodo.redo')" :aria-label="t('paperTodo.redo')" @click="store.redoPaper(paper.id)"><Redo2 class="h-4 w-4" /></button>
          <button class="paper-desk-history-button" type="button" :disabled="!store.canUndo(paper.id)" @click="store.undoPaper(paper.id)"><Undo2 class="h-3 w-3" />{{ t('paperTodo.undo') }}</button>
          <button class="paper-desk-history-button" type="button" :disabled="!store.canRedo(paper.id)" @click="store.redoPaper(paper.id)"><Redo2 class="h-3 w-3" />{{ t('paperTodo.redo') }}</button>
          <button class="paper-icon-button paper-footer-icon" type="button" :disabled="!completedCount" :title="tip('paperTodo.clearCompleted')" :aria-label="t('paperTodo.clearCompleted')" @click="clearCompleted"><RotateCcw class="h-4 w-4" /></button>
        </footer>
    </div>

    <div v-else class="flex min-h-0 flex-1 flex-col">
        <div class="paper-note-toolbar flex min-h-10 shrink-0 items-center gap-1 px-2" :class="compactToolbarVisible && 'is-forced-visible'">
          <span class="paper-note-shortcuts">{{ t('paperTodo.markdownHint') }}</span>
          <button class="paper-icon-button paper-format-button font-bold" type="button" :title="tip('paperTodo.bold')" :aria-label="t('paperTodo.bold')" @click="formatSelection('**')"><Bold class="h-4 w-4" /></button>
          <button class="paper-icon-button paper-format-button italic" type="button" :title="tip('paperTodo.italic')" :aria-label="t('paperTodo.italic')" @click="formatSelection('*')"><Italic class="h-4 w-4" /></button>
          <button class="paper-icon-button paper-format-button" type="button" :title="tip('paperTodo.link')" :aria-label="t('paperTodo.link')" @click="formatSelection('[', '](https://)')"><Link class="h-4 w-4" /></button>
          <button v-if="paperSkin === 'desk'" class="paper-icon-button paper-format-button" type="button" :title="tip('paperTodo.list')" :aria-label="t('paperTodo.list')" @click="formatSelection('- ', '')"><List class="h-4 w-4" /></button>
          <button class="paper-icon-button" type="button" :disabled="pendingImage" :title="tip('paperTodo.insertImage')" :aria-label="t('paperTodo.insertImage')" @click="insertImage('file')"><Image class="h-4 w-4" /></button>
          <button v-if="paperSkin === 'quiet'" class="paper-quiet-preview-button" type="button" :class="previewMode === 'preview' && 'is-active'" :aria-pressed="previewMode === 'preview'" :title="tip('paperTodo.preview')" :aria-label="t('paperTodo.preview')" @click="previewMode = previewMode === 'preview' ? 'edit' : 'preview'"><Eye class="h-4 w-4" /></button>
          <button v-if="paperSkin === 'desk' && noteHeadings.length && !outlineOpen" class="paper-outline-toggle" type="button" :aria-pressed="outlineOpen" :title="tip('paperTodo.showOutline')" :aria-label="t('paperTodo.showOutline')" @click="outlineOpen = true"><PanelLeftClose class="h-4 w-4" /></button>
          <span class="paper-toolbar-divider mx-1 h-4 w-px bg-current/15"></span>
          <button class="paper-text-button paper-mode-button" type="button" :class="previewMode === 'edit' && 'is-active'" @click="previewMode = 'edit'">{{ t('paperTodo.edit') }}</button>
          <button class="paper-text-button paper-mode-button paper-split-mode" type="button" :class="previewMode === 'split' && 'is-active'" @click="previewMode = 'split'">{{ t('paperTodo.split') }}</button>
          <button class="paper-text-button paper-mode-button" type="button" :class="previewMode === 'preview' && 'is-active'" @click="previewMode = 'preview'">{{ t('paperTodo.preview') }}</button>
          <button class="paper-text-button ml-auto" type="button" :title="tip('paperTodo.resetZoom')" :aria-label="t('paperTodo.resetZoom')" @click="update(value => { value.zoom = 100; }, false, true)">{{ paper.zoom }}%</button>
        </div>
        <div
          class="paper-note-workspace grid min-h-0 flex-1"
          :class="[
            previewMode === 'split' ? 'is-split' : 'is-single',
            paperSkin === 'desk' && noteHeadings.length && outlineOpen && 'has-outline',
          ]"
          :style="{ '--paper-outline-width': `${outlineWidth}px` }"
        >
          <aside v-if="paperSkin === 'desk' && noteHeadings.length && outlineOpen" class="paper-note-outline" :aria-label="t('paperTodo.outline')">
            <div class="paper-outline-head"><span>{{ t('paperTodo.outline') }}</span><button type="button" :title="tip('paperTodo.hideOutline')" :aria-label="t('paperTodo.hideOutline')" @click="outlineOpen = false"><PanelLeftClose class="h-3 w-3" /></button></div>
            <button
              v-for="heading in noteHeadings"
              :key="`${heading.offset}-${heading.label}`"
              type="button"
              :style="{ paddingLeft: `${6 + (heading.level - 1) * 8}px` }"
              @click="focusNoteHeading(heading.offset)"
            >{{ heading.label }}</button>
            <span class="paper-outline-resizer" role="separator" aria-orientation="vertical" :aria-label="t('paperTodo.resizeOutline')" @pointerdown="startOutlineResize"></span>
          </aside>
          <textarea
            v-if="previewMode !== 'preview'"
            ref="noteTextarea"
            :value="paper.content"
            class="paper-note-editor min-h-0 w-full resize-none bg-transparent p-3 font-mono leading-6 outline-none placeholder:opacity-40"
            :class="[noteTextClass, settings.noteBold && 'font-semibold', previewMode === 'split' && 'border-r border-current/10']"
            :style="{ fontSize: `calc(var(--paper-note-font-size) * ${paper.zoom / 100})` }"
            :placeholder="t('paperTodo.notePlaceholder')"
            maxlength="500000"
            @input="changeNote"
            @keydown="onNoteKeydown"
            @wheel="onNoteWheel"
            @paste="onNotePaste"
          ></textarea>
          <div v-if="previewMode !== 'edit'" class="paper-note-preview min-h-0 overflow-y-auto p-3 leading-6" :class="noteTextClass" :style="{ fontSize: `calc(var(--paper-note-font-size) * ${paper.zoom / 100})` }">
            <PaperTodoMarkdown :content="paper.content" />
          </div>
        </div>
        <footer class="paper-note-footer flex shrink-0 items-center px-2">
          <span class="paper-note-status mr-auto text-xs opacity-55">{{ paperSkin === 'grain' ? t('paperTodo.noteStatus', { count: paper.content.length.toLocaleString(), status: store.savingIds.value.has(paper.id) ? t('paperTodo.saving') : t('paperTodo.saved') }) : t('paperTodo.noteCapacityStatus', { count: paper.content.length.toLocaleString(), max: MAX_NOTE_LENGTH.toLocaleString(), status: store.savingIds.value.has(paper.id) ? t('paperTodo.saving') : t('paperTodo.saved') }) }}</span>
          <div class="paper-grain-note-modes">
            <button type="button" :class="previewMode === 'edit' && 'is-active'" @click="previewMode = 'edit'">{{ t('paperTodo.edit') }}</button>
            <button type="button" :class="previewMode === 'preview' && 'is-active'" @click="previewMode = 'preview'">{{ t('paperTodo.preview') }}</button>
            <button type="button" :title="tip('paperTodo.resetZoom')" :aria-label="t('paperTodo.resetZoom')" @click="update(value => { value.zoom = 100; }, false, true)">{{ paper.zoom }}%</button>
          </div>
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
      @keydown.tab="trapDeleteDialogFocus"
    >
      <div class="w-full max-w-72 rounded-md border border-current/15 bg-white p-4 text-slate-800 shadow-xl dark:bg-zinc-900 dark:text-zinc-100">
          <p class="text-sm leading-6">{{ t('paperTodo.confirmDeletePaper', { title: paper.title }) }}</p>
          <p v-if="store.error.value" class="mt-2 text-xs leading-5 text-rose-600" role="alert">{{ store.error.value }}</p>
          <div class="mt-4 flex justify-end gap-2">
          <button type="button" class="paper-confirm-button" :disabled="deletingPaper" autofocus @click="deleteConfirmationOpen = false">{{ t('common.cancel') }}</button>
          <button type="button" class="paper-confirm-button border-rose-300 text-rose-700 hover:bg-rose-50 disabled:cursor-default disabled:opacity-50 dark:text-rose-300" :disabled="deletingPaper" @click="confirmDeletePaper">{{ deletingPaper ? t('common.loading') : t('paperTodo.deletePaper') }}</button>
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
  border-radius: var(--paper-radius);
  clip-path: inset(0 round var(--paper-radius));
}
.paper-header {
  min-height: var(--paper-header-h);
  border-bottom: 1px solid var(--paper-hairline);
}
.paper-kind-badge { display: inline-flex; min-width: 0; align-items: center; gap: 5px; }
.paper-kind-icon { opacity: 0.65; }
.paper-kind-label,
.paper-header-meta { display: none; }
.paper-overflow-wrap { position: relative; flex: 0 0 auto; }
.paper-overflow-menu {
  position: absolute;
  z-index: 30;
  top: calc(100% + 5px);
  right: 0;
  width: 190px;
  overflow: hidden;
  border: 1px solid var(--paper-hairline);
  border-radius: 9px;
  background: color-mix(in srgb, var(--paper-base) 96%, var(--paper-tint));
  box-shadow: 0 12px 26px rgb(15 23 42 / 0.2);
  padding: 4px;
  color: var(--paper-ink);
}
.paper-overflow-menu button {
  display: flex;
  width: 100%;
  min-height: 32px;
  cursor: pointer;
  align-items: center;
  gap: 9px;
  border-radius: 6px;
  padding: 0 9px;
  text-align: left;
  font-size: 12px;
}
.paper-overflow-menu button:hover,
.paper-overflow-menu button:focus-visible { background: color-mix(in srgb, var(--paper-tint) 11%, transparent); outline: none; }
.paper-overflow-menu button:disabled { cursor: default; opacity: 0.35; }
.paper-overflow-menu button.is-danger { color: #c2413b; }
.paper-link-wrap { position: relative; flex: 0 0 auto; align-self: center; }
.paper-link-trigger {
  display: inline-flex;
  min-width: 24px;
  height: 24px;
  cursor: pointer;
  align-items: center;
  justify-content: center;
  gap: 4px;
  border-radius: 5px;
  color: var(--paper-muted);
}
.paper-link-trigger:hover,
.paper-link-trigger:focus-visible,
.paper-link-trigger.is-linked { background: color-mix(in srgb, var(--paper-tint) 10%, transparent); color: var(--paper-ink); outline: none; }
.paper-link-menu {
  position: absolute;
  z-index: 25;
  top: calc(100% + 3px);
  right: 0;
  width: 158px;
  max-height: 190px;
  overflow-y: auto;
  border: 1px solid var(--paper-hairline);
  border-radius: 7px;
  background: color-mix(in srgb, var(--paper-base) 97%, var(--paper-tint));
  box-shadow: 0 10px 24px rgb(15 23 42 / 0.2);
  padding: 4px;
}
.paper-link-menu button { display: block; width: 100%; min-height: 30px; cursor: pointer; overflow: hidden; border-radius: 5px; padding: 0 8px; text-align: left; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; }
.paper-link-menu button:hover,
.paper-link-menu button:focus-visible { background: color-mix(in srgb, var(--paper-tint) 11%, transparent); outline: none; }
.paper-link-menu button[aria-checked='true'] { color: var(--paper-tint); font-weight: 700; }
.paper-todo-entry { border-bottom: 1px solid var(--paper-hairline); padding: 12px; }
.paper-todo-list { padding: 8px; }
.paper-todo-row { min-height: var(--paper-row-h); padding-top: 6px; padding-bottom: 6px; }
.paper-priority-mark,
.paper-completed-group,
.paper-grain-footer-hint,
.paper-desk-progress,
.paper-desk-history-button,
.paper-grain-note-modes,
.paper-quiet-preview-button,
.paper-outline-toggle,
.paper-quiet-enter-hint { display: none; }
.paper-todo-checkbox,
.paper-todo-text,
.paper-link-select,
.paper-row-action { margin-top: 4px; }
.paper-todo-filters { display: none; }
.paper-todo-footer,
.paper-note-footer { min-height: var(--paper-footer-h); border-top: 1px solid var(--paper-hairline); }
.paper-progress-track { height: 3px; flex: 0 0 3px; background: color-mix(in srgb, var(--paper-tint) 18%, transparent); }
.paper-progress-track > span { display: block; height: 100%; background: var(--paper-tint); transition: width 200ms ease; }
.paper-note-toolbar { border-bottom: 1px solid var(--paper-hairline); }
.paper-note-shortcuts { display: none; }
.paper-note-workspace.is-single { grid-template-columns: minmax(0, 1fr); }
.paper-note-workspace.is-split { grid-template-columns: repeat(2, minmax(0, 1fr)); }
.paper-note-outline { display: none; }
.paper-skin-classic .paper-header,
.paper-skin-classic .paper-todo-entry,
.paper-skin-classic .paper-todo-footer,
.paper-skin-classic .paper-note-toolbar,
.paper-skin-classic .paper-note-footer { border-color: color-mix(in srgb, currentColor 10%, transparent); }

.paper-skin-grain,
.paper-skin-quiet,
.paper-skin-desk { border-radius: var(--paper-radius); }
.paper-surface:not(.paper-skin-classic) .paper-todo-text { font-size: var(--paper-todo-font-size); }
.paper-skin-grain .paper-header-extra,
.paper-skin-quiet .paper-header-extra,
.paper-skin-desk .paper-header-extra,
.paper-skin-grain .paper-window-drag-handle,
.paper-skin-quiet .paper-window-drag-handle,
.paper-skin-desk .paper-window-drag-handle { display: none; }
.paper-skin-grain .paper-pin-command,
.paper-skin-quiet .paper-pin-command { display: none; }
.paper-skin-grain .paper-header-meta,
.paper-skin-quiet .paper-header-meta,
.paper-skin-desk .paper-header-meta { display: inline-flex; flex: 0 0 auto; align-items: center; color: var(--paper-muted); font-size: 10.5px; font-variant-numeric: tabular-nums; }

/* Grain: a bound notepad with a warm top strip and rules aligned to rows. */
.paper-skin-grain {
  background-image: repeating-linear-gradient(to bottom, transparent 0, transparent 35px, color-mix(in srgb, var(--paper-tint) 16%, transparent) 35px, color-mix(in srgb, var(--paper-tint) 16%, transparent) 36px);
  box-shadow: 0 14px 30px color-mix(in srgb, var(--paper-tint) 20%, transparent), 0 0 0 1px var(--paper-hairline);
}
.paper-skin-grain .paper-header {
  border-bottom-color: var(--paper-hairline);
  background: linear-gradient(color-mix(in srgb, var(--paper-tint) 15%, var(--paper-base)), color-mix(in srgb, var(--paper-tint) 10%, var(--paper-base)));
  padding-inline: 10px;
}
.paper-skin-grain .paper-title-input { font-size: var(--paper-title-font-size); font-weight: 700; }
.paper-skin-grain .paper-header-meta { border-radius: 999px; background: color-mix(in srgb, var(--paper-tint) 13%, transparent); padding: 2px 7px; color: var(--paper-ink); }
.paper-skin-grain .paper-todo-entry { min-height: var(--paper-row-h); border: 0; padding: 0 12px; }
.paper-skin-grain .paper-todo-entry-input { border: 0; border-radius: 0; background: transparent; padding-block: 0; box-shadow: none; }
.paper-skin-grain .paper-command-button { width: 28px; height: 28px; align-self: center; border-radius: 7px; background: var(--paper-tint); }
.paper-skin-grain .paper-todo-list { padding: 0 10px; }
.paper-skin-grain .paper-todo-row { align-items: center; border-radius: 4px; padding-block: 0; }
.paper-skin-grain .paper-todo-checkbox { width: 17px; height: 17px; margin-top: 0; border-radius: 5px; }
.paper-skin-grain .paper-todo-text { margin-top: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.paper-skin-grain .paper-todo-drag-handle,
.paper-skin-grain .paper-row-action,
.paper-skin-grain .paper-link-trigger { margin-top: 0; opacity: 0; transition: opacity 150ms ease; }
.paper-skin-grain .paper-todo-drag-handle { position: absolute; z-index: 3; right: 28px; }
.paper-skin-grain .paper-todo-row:hover .paper-todo-drag-handle,
.paper-skin-grain .paper-todo-row:focus-within .paper-todo-drag-handle,
.paper-skin-grain .paper-todo-row:hover .paper-row-action,
.paper-skin-grain .paper-todo-row:focus-within .paper-row-action,
.paper-skin-grain .paper-todo-row:hover .paper-link-trigger,
.paper-skin-grain .paper-todo-row:focus-within .paper-link-trigger { opacity: 0.7; }
.paper-skin-grain .paper-link-trigger.is-linked { opacity: 0.7; }
.paper-skin-grain .paper-todo-row.is-completed { opacity: 0.45; }
.paper-surface:not(.paper-skin-classic) .paper-todo-row.is-completed .paper-todo-checkbox { border-color: var(--paper-tint); background: var(--paper-tint); color: var(--paper-base); }
.paper-skin-grain .paper-todo-footer { border: 0; background: color-mix(in srgb, var(--paper-tint) 10%, var(--paper-base)); padding-inline: 12px; }
.paper-skin-grain .paper-todo-footer .paper-icon-button { display: none; }
.paper-skin-grain .paper-grain-footer-hint { display: inline; color: var(--paper-muted); font-size: 10px; }
.paper-skin-grain .paper-note-toolbar:not(.is-forced-visible) { display: none; }
.paper-skin-grain .paper-note-editor,
.paper-skin-grain .paper-note-preview { background-image: repeating-linear-gradient(to bottom, transparent 0, transparent 25px, color-mix(in srgb, var(--paper-tint) 14%, transparent) 25px, color-mix(in srgb, var(--paper-tint) 14%, transparent) 26px); background-attachment: local; }
.paper-skin-grain .paper-note-footer { border-color: var(--paper-hairline); background: color-mix(in srgb, var(--paper-tint) 10%, var(--paper-base)); }
.paper-skin-grain .paper-note-footer > .paper-icon-button { display: none; }
.paper-skin-grain .paper-grain-note-modes { display: inline-flex; align-items: center; gap: 2px; }
.paper-skin-grain .paper-grain-note-modes button { min-height: 22px; cursor: pointer; border-radius: 5px; padding: 0 7px; color: var(--paper-muted); font-size: 10.5px; font-weight: 600; }
.paper-skin-grain .paper-grain-note-modes button.is-active { background: color-mix(in srgb, var(--paper-tint) 13%, transparent); color: var(--paper-ink); }
.paper-skin-grain :deep(blockquote) { border-left: 3px solid var(--paper-tint); font-style: italic; }

/* Quiet: whitespace carries hierarchy; controls appear only when needed. */
.paper-skin-quiet { box-shadow: 0 18px 36px rgb(20 26 38 / 0.14), 0 0 0 1px color-mix(in srgb, var(--paper-ink) 7%, transparent); }
.paper-skin-quiet .paper-header { align-items: flex-start; border: 0; padding: 16px 14px 8px 20px; }
.paper-skin-quiet .paper-title-input { font-size: var(--paper-title-font-size); font-weight: 600; }
.paper-skin-quiet .paper-kind-badge { display: none; }
.paper-skin-quiet .paper-header-meta { position: absolute; top: 43px; left: 22px; }
.paper-skin-quiet .paper-header > .paper-icon-button,
.paper-skin-quiet .paper-overflow-wrap { opacity: 0; pointer-events: none; transition: opacity 180ms ease; }
.paper-skin-quiet:hover .paper-header > .paper-icon-button,
.paper-skin-quiet:focus-within .paper-header > .paper-icon-button,
.paper-skin-quiet:hover .paper-overflow-wrap,
.paper-skin-quiet:focus-within .paper-overflow-wrap { opacity: 1; pointer-events: auto; }
.paper-skin-quiet .paper-todo-entry { order: 3; min-height: 52px; align-items: center; border: 0; padding: 7px 20px 12px; }
.paper-skin-quiet .paper-todo-entry-input { border: 0; background: transparent; padding-inline: 5px; box-shadow: none; }
.paper-skin-quiet .paper-command-button { order: -1; width: 26px; height: 26px; border-radius: 999px; background: var(--paper-ink); color: var(--paper-base); }
.paper-skin-quiet .paper-quiet-enter-hint { display: inline; align-self: center; color: color-mix(in srgb, var(--paper-ink) 24%, transparent); font-size: 10.5px; }
.paper-skin-quiet .paper-todo-list { display: flex; flex-direction: column; padding: 2px 18px; }
.paper-skin-quiet .paper-todo-row { align-items: flex-start; padding: 3px 0; }
.paper-skin-quiet .paper-todo-row.is-completed { order: 2; min-height: 34px; color: var(--paper-muted); }
.paper-skin-quiet .paper-todo-checkbox { width: 18px; height: 18px; margin-top: 7px; border-width: 1.5px; }
.paper-skin-quiet .paper-todo-text { margin-top: 4px; min-height: 34px; white-space: normal; line-height: 20px; }
.paper-skin-quiet .paper-todo-checkbox { order: 0; }
.paper-skin-quiet .paper-todo-text { order: 1; }
.paper-skin-quiet .paper-priority-mark { order: 2; display: block; width: 5px; height: 5px; flex: 0 0 5px; align-self: flex-start; margin: 13px 3px 0 1px; border-radius: 999px; background: #c05a3c; }
.paper-skin-quiet .paper-priority-mark.is-medium,
.paper-skin-quiet .paper-priority-mark.is-none { visibility: hidden; }
.paper-skin-quiet .paper-link-wrap { order: 3; }
.paper-skin-quiet .paper-row-action { order: 4; }
.paper-skin-quiet .paper-todo-drag-handle { position: absolute; z-index: 3; right: 28px; }
.paper-skin-quiet .paper-todo-row.is-completed .paper-priority-mark { opacity: 0; }
.paper-surface.paper-skin-quiet .paper-todo-row.is-completed .paper-todo-checkbox { border-color: transparent; background: color-mix(in srgb, var(--paper-ink) 12%, transparent); color: color-mix(in srgb, var(--paper-ink) 68%, var(--paper-base)); }
.paper-skin-quiet .paper-todo-drag-handle,
.paper-skin-quiet .paper-row-action,
.paper-skin-quiet .paper-link-select,
.paper-skin-quiet .paper-link-trigger { opacity: 0; transition: opacity 150ms ease; }
.paper-skin-quiet .paper-todo-row:hover .paper-todo-drag-handle,
.paper-skin-quiet .paper-todo-row:focus-within .paper-todo-drag-handle,
.paper-skin-quiet .paper-todo-row:hover .paper-row-action,
.paper-skin-quiet .paper-todo-row:focus-within .paper-row-action,
.paper-skin-quiet .paper-todo-row:hover .paper-link-select,
.paper-skin-quiet .paper-todo-row:focus-within .paper-link-select,
.paper-skin-quiet .paper-todo-row:hover .paper-link-trigger,
.paper-skin-quiet .paper-todo-row:focus-within .paper-link-trigger { opacity: 0.7; }
.paper-skin-quiet .paper-completed-group { order: 1; display: flex; min-height: 30px; align-items: center; padding: 6px 5px 2px; color: var(--paper-muted); font-size: 10.5px; font-weight: 700; letter-spacing: 0.12em; text-transform: uppercase; }
.paper-skin-quiet .paper-progress-track,
.paper-skin-quiet .paper-todo-footer { display: none; }
.paper-skin-quiet .paper-note-toolbar:not(.is-forced-visible) { min-height: 38px; order: 3; border: 0; background: transparent; }
.paper-skin-quiet .paper-note-toolbar:not(.is-forced-visible) .paper-note-shortcuts { display: inline; margin-right: auto; color: var(--paper-muted); font-size: 10.5px; }
.paper-skin-quiet .paper-note-toolbar:not(.is-forced-visible) .paper-format-button,
.paper-skin-quiet .paper-note-toolbar:not(.is-forced-visible) .paper-toolbar-divider,
.paper-skin-quiet .paper-note-toolbar:not(.is-forced-visible) .paper-text-button { display: none; }
.paper-skin-quiet .paper-quiet-preview-button { display: inline-flex; width: 28px; height: 28px; cursor: pointer; align-items: center; justify-content: center; border-radius: 7px; color: var(--paper-muted); }
.paper-skin-quiet .paper-quiet-preview-button.is-active { background: var(--paper-ink); color: var(--paper-base); }
.paper-skin-quiet .paper-note-editor,
.paper-skin-quiet .paper-note-preview { padding: 14px 20px; }
.paper-skin-quiet .paper-note-footer { display: none; }
.paper-skin-quiet :deep(code) { border-radius: 3px; background: color-mix(in srgb, var(--paper-ink) 7%, transparent); padding: 1px 4px; }
.paper-skin-quiet :deep(blockquote) { border-left: 2px solid color-mix(in srgb, var(--paper-ink) 16%, transparent); }

/* Desk: dense controls, compact rows, and a continuous type spine. */
.paper-skin-desk { padding-left: 5px; box-shadow: 0 10px 24px rgb(15 23 42 / 0.18), 0 0 0 1px var(--paper-hairline); }
.paper-skin-desk::before { position: absolute; z-index: 4; inset: 0 auto 0 0; width: 5px; background: linear-gradient(var(--paper-tint), color-mix(in srgb, var(--paper-tint) 58%, #f8d37a)); content: ''; }
.paper-skin-desk .paper-header { padding-inline: 8px; }
.paper-skin-desk .paper-kind-badge { order: 0; }
.paper-skin-desk .paper-title-input { order: 1; }
.paper-skin-desk .paper-header-meta { order: 2; }
.paper-skin-desk .paper-pin-command { order: 3; }
.paper-skin-desk .paper-overflow-wrap { order: 4; }
.paper-skin-desk .paper-header > .paper-icon-button:not(.paper-pin-command) { order: 5; }
.paper-skin-desk .paper-kind-badge { border-radius: 5px; background: color-mix(in srgb, var(--paper-tint) 13%, transparent); padding: 3px 6px; color: color-mix(in srgb, var(--paper-tint) 72%, var(--paper-ink)); }
.paper-skin-desk .paper-kind-label { display: inline; font-size: 10.5px; font-weight: 700; }
.paper-skin-desk .paper-title-input { font-size: var(--paper-title-font-size); font-weight: 650; }
.paper-skin-desk .paper-todo-entry { min-height: 36px; align-items: center; border-color: var(--paper-hairline); background: color-mix(in srgb, var(--paper-ink) 3%, var(--paper-base)); padding: 3px 8px; }
.paper-skin-desk .paper-todo-entry-input { height: 28px; border: 0; background: transparent; padding: 0 6px; box-shadow: none; font-size: 12px; }
.paper-skin-desk .paper-command-button { width: 26px; height: 26px; border-radius: 5px; }
.paper-skin-desk .paper-todo-filters { display: inline-flex; flex: 0 0 auto; border: 1px solid var(--paper-hairline); border-radius: 5px; padding: 2px; }
.paper-skin-desk .paper-todo-filters button { min-height: 22px; cursor: pointer; border-radius: 3px; padding: 0 7px; color: var(--paper-muted); font-size: 10.5px; }
.paper-skin-desk .paper-todo-filters button.is-active { background: var(--paper-ink); color: var(--paper-base); }
.paper-skin-desk .paper-todo-list { display: flex; flex-direction: column; padding: 0 8px; }
.paper-skin-desk .paper-todo-row { align-items: center; border-bottom: 1px solid color-mix(in srgb, var(--paper-ink) 6%, transparent); border-radius: 0; padding-block: 0; }
.paper-skin-desk .paper-todo-row.is-completed { order: 2; }
.paper-skin-desk .paper-todo-checkbox { width: 15px; height: 15px; margin-top: 0; border-radius: 4px; }
.paper-skin-desk .paper-priority-mark { display: block; width: 3px; height: 18px; flex: 0 0 3px; border-radius: 2px; background: color-mix(in srgb, var(--paper-ink) 12%, transparent); }
.paper-skin-desk .paper-priority-mark.is-high { background: #c2410c; }
.paper-skin-desk .paper-priority-mark.is-medium { background: #d8a13c; }
.paper-skin-desk .paper-todo-text { margin-top: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.paper-skin-desk .paper-todo-drag-handle,
.paper-skin-desk .paper-row-action { height: 26px; margin-top: 0; opacity: 0; }
.paper-skin-desk .paper-todo-drag-handle { position: absolute; z-index: 3; right: 28px; }
.paper-skin-desk .paper-todo-row:hover .paper-todo-drag-handle,
.paper-skin-desk .paper-todo-row:focus-within .paper-todo-drag-handle,
.paper-skin-desk .paper-todo-row:hover .paper-row-action,
.paper-skin-desk .paper-todo-row:focus-within .paper-row-action { opacity: 0.7; }
.paper-skin-desk .paper-link-trigger { max-width: 92px; border: 1px solid var(--paper-hairline); padding: 0 5px; font-size: 9.5px; }
.paper-skin-desk .paper-link-trigger span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.paper-skin-desk .paper-completed-group { order: 1; display: flex; min-height: 28px; align-items: center; border-bottom: 1px solid var(--paper-hairline); color: var(--paper-muted); font-size: 10.5px; font-weight: 700; letter-spacing: 0.04em; }
.paper-skin-desk .paper-completed-group > button:first-child { display: inline-flex; min-height: 26px; cursor: pointer; align-items: center; gap: 5px; }
.paper-skin-desk .paper-completed-clear { margin-left: auto; min-height: 26px; cursor: pointer; color: var(--paper-muted); font-size: 10px; font-weight: 600; }
.paper-skin-desk .paper-completed-clear:hover,
.paper-skin-desk .paper-completed-clear:focus-visible { color: #c2413b; outline: none; }
.paper-skin-desk .paper-todo-footer,
.paper-skin-desk .paper-note-footer { background: color-mix(in srgb, var(--paper-ink) 3%, var(--paper-base)); }
.paper-skin-desk .paper-footer-status,
.paper-skin-desk .paper-footer-icon { display: none; }
.paper-skin-desk .paper-footer-icon.text-rose-600 { display: inline-flex; }
.paper-skin-desk .paper-desk-progress { display: inline-flex; margin-right: auto; align-items: center; gap: 5px; }
.paper-skin-desk .paper-desk-progress i { display: block; width: 64px; height: 4px; overflow: hidden; border-radius: 999px; background: color-mix(in srgb, var(--paper-ink) 14%, transparent); }
.paper-skin-desk .paper-desk-progress b { display: block; height: 100%; border-radius: inherit; background: var(--paper-tint); }
.paper-skin-desk .paper-desk-progress em { color: var(--paper-muted); font-size: 10.5px; font-style: normal; font-weight: 600; font-variant-numeric: tabular-nums; }
.paper-skin-desk .paper-desk-history-button { display: inline-flex; min-height: 22px; cursor: pointer; align-items: center; gap: 4px; border: 1px solid var(--paper-hairline); border-radius: 5px; background: color-mix(in srgb, var(--paper-base) 94%, white); padding: 0 7px; color: var(--paper-ink); font-size: 10.5px; font-weight: 600; }
.paper-skin-desk .paper-desk-history-button:disabled { cursor: default; opacity: 0.35; }
.paper-skin-desk .paper-note-toolbar { min-height: 36px; background: color-mix(in srgb, var(--paper-ink) 3%, var(--paper-base)); }
.paper-skin-desk .paper-note-toolbar .paper-toolbar-divider { display: none; }
.paper-skin-desk .paper-note-toolbar .paper-icon-button,
.paper-skin-desk .paper-outline-toggle { width: 26px; height: 26px; }
.paper-skin-desk .paper-outline-toggle { display: inline-flex; cursor: pointer; align-items: center; justify-content: center; border-radius: 5px; color: var(--paper-muted); }
.paper-skin-desk .paper-outline-toggle:hover,
.paper-skin-desk .paper-outline-toggle:focus-visible { background: color-mix(in srgb, var(--paper-tint) 10%, transparent); color: var(--paper-ink); outline: none; }
.paper-skin-desk .paper-note-toolbar .paper-text-button { min-height: 24px; flex: 0 0 auto; border-radius: 4px; padding-inline: 6px; white-space: nowrap; }
.paper-skin-desk .paper-note-toolbar .paper-mode-button.is-active { background: var(--paper-ink); color: var(--paper-base); }
.paper-skin-desk .paper-note-workspace.has-outline.is-single { grid-template-columns: var(--paper-outline-width, 96px) minmax(0, 1fr); }
.paper-skin-desk .paper-note-workspace.has-outline.is-split { grid-template-columns: var(--paper-outline-width, 96px) repeat(2, minmax(0, 1fr)); }
.paper-skin-desk .paper-note-outline { position: relative; display: flex; min-width: 0; flex-direction: column; gap: 2px; overflow-x: hidden; overflow-y: auto; border-right: 1px solid var(--paper-hairline); background: color-mix(in srgb, var(--paper-ink) 2.5%, var(--paper-base)); padding: 6px 5px; }
.paper-skin-desk .paper-outline-head { display: flex; min-height: 24px; align-items: center; justify-content: space-between; padding-left: 5px; color: var(--paper-muted); font-size: 9px; font-weight: 700; letter-spacing: 0.1em; text-transform: uppercase; }
.paper-skin-desk .paper-outline-head button { display: inline-flex; width: 22px; min-height: 22px; cursor: pointer; align-items: center; justify-content: center; padding: 0; }
.paper-skin-desk .paper-note-outline button { overflow: hidden; min-height: 24px; cursor: pointer; border-radius: 4px; text-align: left; text-overflow: ellipsis; white-space: nowrap; color: var(--paper-muted); font-size: 10px; }
.paper-skin-desk .paper-note-outline button:hover,
.paper-skin-desk .paper-note-outline button:focus-visible { background: color-mix(in srgb, var(--paper-tint) 10%, transparent); color: var(--paper-ink); outline: none; }
.paper-skin-desk .paper-outline-resizer { position: absolute; z-index: 2; top: 0; right: 0; bottom: 0; width: 5px; cursor: col-resize; }
.paper-skin-desk .paper-note-editor,
.paper-skin-desk .paper-note-preview { padding: 10px; }

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
  .paper-icon-button { transition: none; }
}
</style>
