import { convertFileSrc, invoke } from '@tauri-apps/api/core';

export type PaperKind = 'todo' | 'note';
export type PaperTheme = 'system' | 'light' | 'dark';
export type PaperPalette = 'warm' | 'ink' | 'forest' | 'frost';
export type PaperSkin = 'classic' | 'grain' | 'quiet' | 'desk';
export type TodoVisualSize = 'small' | 'medium' | 'large' | 'xlarge';
export type TextRendering = 'standard' | 'soft' | 'sharp';
export type ImageMarkerVisibility = 'always' | 'editing' | 'hidden';

export interface PaperTodoItem {
  id: string;
  text: string;
  completed: boolean;
  linkedNoteId: string | null;
}

export interface PaperGeometry {
  x: number | null;
  y: number | null;
  width: number;
  height: number;
  monitorName: string | null;
}

export interface PaperDocument {
  id: string;
  kind: PaperKind;
  title: string;
  items: PaperTodoItem[];
  content: string;
  zoom: number;
  pinned: boolean;
  hidden: boolean;
  desktopOpen: boolean;
  geometry: PaperGeometry;
  createdAt: number;
  updatedAt: number;
}

export interface PaperHotkeys {
  showAll: string;
  hideAll: string;
  toggleAll: string;
  newTodo: string;
  newNote: string;
}

export interface PaperTodoSettings {
  launcherEnabled: boolean;
  launcherEdge: 'left' | 'right';
  launcherOffset: number;
  autoCollapseLauncher: boolean;
  theme: PaperTheme;
  palette: PaperPalette;
  paperSkin: PaperSkin;
  todoSize: TodoVisualSize;
  titleMaxLength: number;
  animations: boolean;
  hoverTips: boolean;
  autoClearCompleted: boolean;
  showLinkedNoteTitle: boolean;
  hideFromTaskbar: boolean;
  avoidFullscreen: boolean;
  showNewTodoButton: boolean;
  showNewNoteButton: boolean;
  showExternalOpenButton: boolean;
  externalExtension: string;
  preferPowerShell7: boolean;
  hideScriptWindow: boolean;
  autoCompressImages: boolean;
  fontFamily: string;
  interfaceScale: number;
  noteFontSize: TodoVisualSize;
  todoFontSize: TodoVisualSize;
  titleFontSize: TodoVisualSize;
  noteBold: boolean;
  todoBold: boolean;
  titleBold: boolean;
  textRendering: TextRendering;
  imageMarkerVisibility: ImageMarkerVisibility;
  hotkeys: PaperHotkeys;
}

export interface PaperTodoState {
  version: 1;
  revision: number;
  papers: PaperDocument[];
  settings: PaperTodoSettings;
}

export interface PaperImageAsset {
  id: string;
  path: string;
  width: number;
  height: number;
  bytes: number;
}

const FALLBACK_STORAGE_KEY = 'file-sync-tool.paper-todo.v1';
export const PAPER_TODO_SESSION_ID = createId();
export const MAX_PAPERS = 100;
export const MAX_NOTE_LENGTH = 500_000;

function createId(): string {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function createDefaultSettings(): PaperTodoSettings {
  return {
    launcherEnabled: true,
    launcherEdge: 'right',
    launcherOffset: 35,
    autoCollapseLauncher: false,
    theme: 'system',
    palette: 'warm',
    paperSkin: 'classic',
    todoSize: 'medium',
    titleMaxLength: 20,
    animations: true,
    hoverTips: true,
    autoClearCompleted: false,
    showLinkedNoteTitle: true,
    hideFromTaskbar: true,
    avoidFullscreen: true,
    showNewTodoButton: true,
    showNewNoteButton: true,
    showExternalOpenButton: true,
    externalExtension: '.md',
    preferPowerShell7: true,
    hideScriptWindow: true,
    autoCompressImages: true,
    fontFamily: 'system-ui',
    interfaceScale: 100,
    noteFontSize: 'medium',
    todoFontSize: 'medium',
    titleFontSize: 'medium',
    noteBold: false,
    todoBold: false,
    titleBold: true,
    textRendering: 'standard',
    imageMarkerVisibility: 'editing',
    hotkeys: {
      showAll: '',
      hideAll: '',
      toggleAll: 'Ctrl+Shift+Space',
      newTodo: 'Ctrl+Shift+T',
      newNote: 'Ctrl+Shift+N',
    },
  };
}

export function createPaper(kind: PaperKind, title?: string): PaperDocument {
  const now = Date.now();
  return {
    id: createId(),
    kind,
    title: title ?? (kind === 'todo' ? '待办纸' : '笔记纸'),
    items: [],
    content: '',
    zoom: 100,
    pinned: true,
    hidden: false,
    desktopOpen: false,
    geometry: {
      x: null,
      y: null,
      width: 380,
      height: 520,
      monitorName: null,
    },
    createdAt: now,
    updatedAt: now,
  };
}

export function createDefaultState(): PaperTodoState {
  return {
    version: 1,
    revision: 0,
    // Seed only a genuinely fresh profile. `normalizePaperTodoState` preserves
    // an explicit empty `papers` array, so users can still delete both papers
    // and keep the launcher at zero items.
    papers: [createPaper('todo'), createPaper('note')],
    settings: createDefaultSettings(),
  };
}

function finiteNumber(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

export function normalizePaperTodoState(value: unknown): PaperTodoState {
  const fallback = createDefaultState();
  if (!value || typeof value !== 'object') return fallback;
  const input = value as Partial<PaperTodoState>;
  const settings = {
    ...fallback.settings,
    ...(input.settings && typeof input.settings === 'object' ? input.settings : {}),
    hotkeys: {
      ...fallback.settings.hotkeys,
      ...(input.settings?.hotkeys && typeof input.settings.hotkeys === 'object'
        ? input.settings.hotkeys
        : {}),
    },
  } as PaperTodoSettings;
  const settingsRecord = settings as PaperTodoSettings & Record<string, unknown>;
  // Remove settings from the retired per-paper capsule implementation when an
  // older profile is loaded, so the next save does not preserve dead options.
  delete settingsRecord.hideLinkedNoteCapsules;
  delete settingsRecord.capsuleMode;
  delete settingsRecord.autoDockCapsules;
  delete settingsRecord.autoHideDockedCapsules;
  delete settingsRecord.rememberExpandedPosition;
  delete settingsRecord.capsuleFontSize;
  delete settingsRecord.capsuleBold;
  settings.titleMaxLength = Math.min(20, Math.max(2, finiteNumber(settings.titleMaxLength, 20)));
  settings.interfaceScale = Math.min(120, Math.max(80, finiteNumber(settings.interfaceScale, 100)));
  settings.launcherOffset = Math.min(100, Math.max(0, finiteNumber(settings.launcherOffset, 35)));
  settings.launcherEdge = settings.launcherEdge === 'left' ? 'left' : 'right';
  const skins: PaperSkin[] = ['classic', 'grain', 'quiet', 'desk'];
  settings.paperSkin = skins.includes(settings.paperSkin) ? settings.paperSkin : 'classic';

  const papers = Array.isArray(input.papers)
    ? input.papers.slice(0, MAX_PAPERS).flatMap((candidate) => {
        if (!candidate || typeof candidate !== 'object') return [];
        const raw = candidate as Partial<PaperDocument>;
        if (raw.kind !== 'todo' && raw.kind !== 'note') return [];
        const base = createPaper(raw.kind);
        const paper: PaperDocument = {
          ...base,
          ...raw,
          id: typeof raw.id === 'string' && raw.id ? raw.id : base.id,
          title: typeof raw.title === 'string' ? raw.title.slice(0, 20) : base.title,
          items: Array.isArray(raw.items)
            ? raw.items.flatMap((item) => {
                if (!item || typeof item !== 'object') return [];
                const todo = item as Partial<PaperTodoItem>;
                if (typeof todo.text !== 'string' || !todo.text.trim()) return [];
                return [{
                  id: typeof todo.id === 'string' && todo.id ? todo.id : createId(),
                  text: todo.text.slice(0, 2_000),
                  completed: Boolean(todo.completed),
                  linkedNoteId: typeof todo.linkedNoteId === 'string' ? todo.linkedNoteId : null,
                }];
              })
            : [],
          content: typeof raw.content === 'string' ? raw.content.slice(0, MAX_NOTE_LENGTH) : '',
          zoom: Math.min(200, Math.max(50, finiteNumber(raw.zoom, 100))),
          geometry: {
            ...base.geometry,
            ...(raw.geometry && typeof raw.geometry === 'object' ? raw.geometry : {}),
          },
          createdAt: finiteNumber(raw.createdAt, base.createdAt),
          updatedAt: finiteNumber(raw.updatedAt, base.updatedAt),
        };
        delete (paper as PaperDocument & Record<string, unknown>).collapsed;
        delete (paper.geometry as PaperGeometry & Record<string, unknown>).dockEdge;
        return [paper];
      })
    : [];

  return {
    version: 1,
    revision: Math.max(0, finiteNumber(input.revision, 0)),
    papers: Array.isArray(input.papers) ? papers : fallback.papers,
    settings,
  };
}

const LIST_PREFIX = /^\s*(?:[-*+]\s+|\d+[.)]\s+|\[[ xX]\]\s*)/;

export function splitTodoPaste(text: string): string[] {
  return text
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((line) => line.replace(LIST_PREFIX, '').trim())
    .filter(Boolean)
    .map((line) => line.slice(0, 2_000));
}

export function createTodoItem(text: string): PaperTodoItem {
  return { id: createId(), text: text.trim().slice(0, 2_000), completed: false, linkedNoteId: null };
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function loadFallback(): PaperTodoState {
  try {
    const raw = localStorage.getItem(FALLBACK_STORAGE_KEY);
    return raw ? normalizePaperTodoState(JSON.parse(raw)) : createDefaultState();
  } catch {
    return createDefaultState();
  }
}

function saveFallback(mutator: (state: PaperTodoState) => void): void {
  const state = loadFallback();
  mutator(state);
  state.revision += 1;
  localStorage.setItem(FALLBACK_STORAGE_KEY, JSON.stringify(state));
}

export async function loadPaperTodoState(): Promise<PaperTodoState> {
  if (!isTauriRuntime()) return loadFallback();
  return normalizePaperTodoState(await invoke<unknown>('paper_todo_load'));
}

export async function savePaperDocument(paper: PaperDocument): Promise<number> {
  const next = { ...paper, updatedAt: Date.now() };
  if (!isTauriRuntime()) {
    saveFallback((state) => {
      const index = state.papers.findIndex((item) => item.id === next.id);
      if (index >= 0) state.papers[index] = next;
      else if (state.papers.length < MAX_PAPERS) state.papers.push(next);
    });
    return loadFallback().revision;
  }
  return invoke<number>('paper_todo_save_paper', { paper: next, source: PAPER_TODO_SESSION_ID });
}

export async function deletePaperDocument(id: string): Promise<number> {
  if (!isTauriRuntime()) {
    saveFallback((state) => { state.papers = state.papers.filter((paper) => paper.id !== id); });
    return loadFallback().revision;
  }
  return invoke<number>('paper_todo_delete_paper', { id, source: PAPER_TODO_SESSION_ID });
}

export async function closePaperWindow(id: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke('paper_todo_close_window', { id });
}

export async function savePaperTodoSettings(settings: PaperTodoSettings): Promise<number> {
  if (!isTauriRuntime()) {
    saveFallback((state) => { state.settings = settings; });
    return loadFallback().revision;
  }
  return invoke<number>('paper_todo_save_settings', { settings, source: PAPER_TODO_SESSION_ID });
}

export async function savePaperOrder(ids: string[]): Promise<number> {
  if (!isTauriRuntime()) {
    saveFallback((state) => {
      const order = new Map(ids.map((id, index) => [id, index]));
      state.papers.sort((a, b) => (order.get(a.id) ?? ids.length) - (order.get(b.id) ?? ids.length));
    });
    return loadFallback().revision;
  }
  return invoke<number>('paper_todo_save_order', { ids, source: PAPER_TODO_SESSION_ID });
}

export function movePaperId(
  ids: string[],
  sourceId: string,
  targetId: string,
  side: 'before' | 'after',
): string[] {
  if (sourceId === targetId || !ids.includes(sourceId) || !ids.includes(targetId)) return [...ids];
  const orderedIds = ids.filter((id) => id !== sourceId);
  let insertAt = orderedIds.indexOf(targetId);
  if (side === 'after') insertAt += 1;
  orderedIds.splice(insertAt, 0, sourceId);
  return orderedIds;
}

export async function openPaperWindow(paper: PaperDocument, settings: PaperTodoSettings): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke('paper_todo_open_window', { paper, settings });
}

export async function setPaperWindowPinned(id: string, pinned: boolean): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke('paper_todo_set_window_pinned', { id, pinned });
}

export async function createDesktopPaper(kind: PaperKind): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke('paper_todo_create_paper', { kind });
}

/**
 * `capsuleWidth` is the logical width the collapsed master capsule needs for
 * its own label. Pass `null` while expanding: the capsule reads a different
 * label then, and the backend keeps the last collapsed measurement.
 */
export async function setPaperLauncherExpanded(
  expanded: boolean,
  itemCount = 0,
  capsuleWidth: number | null = null,
): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke('paper_todo_set_launcher_expanded', { expanded, itemCount, capsuleWidth });
}

/**
 * Run the native drag loop that slides the launcher along its display edge.
 * Resolves to `true` once the press actually travelled, and `false` when it was
 * a plain click the caller should turn into an expand/collapse.
 */
export async function dragPaperLauncher(): Promise<boolean> {
  if (!isTauriRuntime()) return false;
  return invoke<boolean>('paper_todo_drag_launcher');
}

export async function openPaperTodoSettings(): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke('paper_todo_open_settings');
}

export async function setAllPaperWindows(action: 'show' | 'hide' | 'toggle'): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke('paper_todo_set_all_windows', { action });
}

export async function importPaperImage(source: 'file' | 'clipboard', autoCompress: boolean): Promise<PaperImageAsset | null> {
  if (!isTauriRuntime()) return null;
  return invoke<PaperImageAsset | null>('paper_todo_import_image', { source, autoCompress });
}

export async function resolvePaperAssets(ids: string[]): Promise<Record<string, string>> {
  if (!isTauriRuntime() || ids.length === 0) return {};
  const paths = await invoke<Record<string, string>>('paper_todo_resolve_assets', { ids });
  return Object.fromEntries(Object.entries(paths).map(([id, path]) => [id, paperAssetUrl(path)]));
}

export function paperAssetUrl(path: string): string {
  return isTauriRuntime() ? convertFileSrc(path) : path;
}

export async function openPaperNoteExternally(paper: PaperDocument, extension: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke('paper_todo_open_external', { paper, extension });
}

export async function runPaperScript(paper: PaperDocument, settings: PaperTodoSettings): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke('paper_todo_run_script', {
    paperId: paper.id,
    content: paper.content,
    preferPowerShell7: settings.preferPowerShell7,
    hidden: settings.hideScriptWindow,
  });
}

export async function exportPaperTodoData(): Promise<string | null> {
  return isTauriRuntime() ? invoke<string | null>('paper_todo_export') : null;
}

export async function importPaperTodoData(): Promise<PaperTodoState | null> {
  return isTauriRuntime()
    ? normalizePaperTodoState(await invoke<unknown | null>('paper_todo_import'))
    : null;
}

export async function cleanPaperTodoAssets(): Promise<number> {
  return isTauriRuntime() ? invoke<number>('paper_todo_clean_assets') : 0;
}

export function isPowerPaper(paper: PaperDocument): boolean {
  if (paper.kind !== 'note') return false;
  return /^\s*!(?:p|power|pf|powerf)\s*(?:\r?\n|$)/i.test(paper.content);
}

export function powerScriptBody(content: string): string {
  return content.replace(/^\s*!(?:p|power|pf|powerf)\s*(?:\r?\n|$)/i, '');
}
