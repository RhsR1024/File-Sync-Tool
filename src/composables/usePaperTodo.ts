import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { computed, ref } from 'vue';

import {
  MAX_PAPERS,
  PAPER_TODO_SESSION_ID,
  createDefaultState,
  createPaper,
  deletePaperDocument,
  loadPaperTodoState,
  savePaperDocument,
  savePaperOrder,
  savePaperTodoSettings,
  type PaperDocument,
  type PaperKind,
  type PaperTodoSettings,
  type PaperTodoState,
} from '@/lib/paperTodo';

interface PaperTodoChangedEvent {
  revision: number;
  paperId: string | null;
  source: string | null;
}

interface PaperHistory {
  undo: PaperDocument[];
  redo: PaperDocument[];
}

const state = ref<PaperTodoState>(createDefaultState());
const loading = ref(false);
const loaded = ref(false);
const error = ref('');
const savingIds = ref(new Set<string>());
const histories = new Map<string, PaperHistory>();
const saveTimers = new Map<string, ReturnType<typeof setTimeout>>();
const pendingPaperSaves = new Map<string, Promise<void>>();
let settingsTimer: ReturnType<typeof setTimeout> | null = null;
let initPromise: Promise<void> | null = null;
let unlisten: UnlistenFn | null = null;

function clonePaper(paper: PaperDocument): PaperDocument {
  return JSON.parse(JSON.stringify(paper)) as PaperDocument;
}

function cloneSettings(settings: PaperTodoSettings): PaperTodoSettings {
  return JSON.parse(JSON.stringify(settings)) as PaperTodoSettings;
}

function historyFor(id: string): PaperHistory {
  let history = histories.get(id);
  if (!history) {
    history = { undo: [], redo: [] };
    histories.set(id, history);
  }
  return history;
}

function pushHistory(paper: PaperDocument): void {
  const history = historyFor(paper.id);
  history.undo.push(clonePaper(paper));
  if (history.undo.length > 50) history.undo.shift();
  history.redo = [];
}

async function persistPaper(paper: PaperDocument): Promise<void> {
  savingIds.value = new Set(savingIds.value).add(paper.id);
  try {
    state.value.revision = await savePaperDocument(clonePaper(paper));
    error.value = '';
  } catch (reason) {
    error.value = String(reason);
  } finally {
    const next = new Set(savingIds.value);
    next.delete(paper.id);
    savingIds.value = next;
  }
}

function enqueuePaperSave(paper: PaperDocument): Promise<void> {
  const snapshot = clonePaper(paper);
  const previous = pendingPaperSaves.get(paper.id) ?? Promise.resolve();
  const pending = previous.then(() => persistPaper(snapshot));
  pendingPaperSaves.set(paper.id, pending);
  void pending.finally(() => {
    if (pendingPaperSaves.get(paper.id) === pending) {
      pendingPaperSaves.delete(paper.id);
    }
  });
  return pending;
}

function schedulePaperSave(paper: PaperDocument, immediate = false): void {
  const current = saveTimers.get(paper.id);
  if (current) clearTimeout(current);
  if (immediate) {
    saveTimers.delete(paper.id);
    void enqueuePaperSave(paper);
    return;
  }
  saveTimers.set(paper.id, setTimeout(() => {
    saveTimers.delete(paper.id);
    void enqueuePaperSave(paper);
  }, 280));
}

async function refreshFromDisk(): Promise<void> {
  const next = await loadPaperTodoState();
  if (next.revision >= state.value.revision) state.value = next;
}

async function initialize(): Promise<void> {
  if (loaded.value) return;
  if (initPromise) return initPromise;
  loading.value = true;
  initPromise = (async () => {
    try {
      state.value = await loadPaperTodoState();
      if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window && !unlisten) {
        unlisten = await listen<PaperTodoChangedEvent>('paper-todo-changed', (event) => {
          if (event.payload.source === PAPER_TODO_SESSION_ID) return;
          void refreshFromDisk();
        });
      }
      loaded.value = true;
      error.value = '';
    } catch (reason) {
      error.value = String(reason);
    } finally {
      loading.value = false;
      initPromise = null;
    }
  })();
  return initPromise;
}

function addPaper(kind: PaperKind): PaperDocument | null {
  if (state.value.papers.length >= MAX_PAPERS) {
    error.value = `便签数量不能超过 ${MAX_PAPERS} 张`;
    return null;
  }
  const paper = createPaper(kind);
  state.value.papers.push(paper);
  schedulePaperSave(paper, true);
  return paper;
}

async function removePaper(id: string): Promise<void> {
  const timer = saveTimers.get(id);
  if (timer) clearTimeout(timer);
  saveTimers.delete(id);
  await pendingPaperSaves.get(id);
  state.value.papers = state.value.papers.filter((paper) => paper.id !== id);
  histories.delete(id);
  try {
    state.value.revision = await deletePaperDocument(id);
  } catch (reason) {
    error.value = String(reason);
    await refreshFromDisk();
  }
}

function updatePaper(
  id: string,
  mutator: (paper: PaperDocument) => void,
  options: { history?: boolean; immediate?: boolean } = {},
): void {
  const paper = state.value.papers.find((candidate) => candidate.id === id);
  if (!paper) return;
  if (options.history) pushHistory(paper);
  mutator(paper);
  paper.updatedAt = Date.now();
  schedulePaperSave(paper, options.immediate);
}

function undoPaper(id: string): void {
  const index = state.value.papers.findIndex((paper) => paper.id === id);
  if (index < 0) return;
  const history = historyFor(id);
  const previous = history.undo.pop();
  if (!previous) return;
  history.redo.push(clonePaper(state.value.papers[index]));
  state.value.papers[index] = previous;
  schedulePaperSave(previous, true);
}

function redoPaper(id: string): void {
  const index = state.value.papers.findIndex((paper) => paper.id === id);
  if (index < 0) return;
  const history = historyFor(id);
  const next = history.redo.pop();
  if (!next) return;
  history.undo.push(clonePaper(state.value.papers[index]));
  state.value.papers[index] = next;
  schedulePaperSave(next, true);
}

function canUndo(id: string): boolean {
  return Boolean(histories.get(id)?.undo.length);
}

function canRedo(id: string): boolean {
  return Boolean(histories.get(id)?.redo.length);
}

function updateSettings(mutator: (settings: PaperTodoSettings) => void): void {
  mutator(state.value.settings);
  if (settingsTimer) clearTimeout(settingsTimer);
  settingsTimer = setTimeout(async () => {
    settingsTimer = null;
    try {
      state.value.revision = await savePaperTodoSettings(cloneSettings(state.value.settings));
      error.value = '';
    } catch (reason) {
      error.value = String(reason);
    }
  }, 300);
}

async function reorderPapers(ids: string[]): Promise<void> {
  const order = new Map(ids.map((id, index) => [id, index]));
  state.value.papers.sort((left, right) =>
    (order.get(left.id) ?? ids.length) - (order.get(right.id) ?? ids.length));
  state.value.revision = await savePaperOrder(ids);
}

async function flush(): Promise<void> {
  const pending = [...saveTimers.keys()];
  for (const id of pending) {
    const timer = saveTimers.get(id);
    if (timer) clearTimeout(timer);
    saveTimers.delete(id);
    const paper = state.value.papers.find((candidate) => candidate.id === id);
    if (paper) void enqueuePaperSave(paper);
  }
  await Promise.all([...pendingPaperSaves.values()]);
  if (settingsTimer) {
    clearTimeout(settingsTimer);
    settingsTimer = null;
    state.value.revision = await savePaperTodoSettings(cloneSettings(state.value.settings));
  }
}

export function usePaperTodo() {
  return {
    state,
    papers: computed(() => state.value.papers),
    settings: computed(() => state.value.settings),
    loading,
    loaded,
    error,
    savingIds,
    initialize,
    refreshFromDisk,
    addPaper,
    removePaper,
    updatePaper,
    undoPaper,
    redoPaper,
    canUndo,
    canRedo,
    updateSettings,
    reorderPapers,
    flush,
  };
}
