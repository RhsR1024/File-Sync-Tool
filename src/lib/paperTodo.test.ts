import { describe, expect, it } from 'vitest';

import {
  createDefaultSettings,
  createPaper,
  createTodoItem,
  isPowerPaper,
  movePaperId,
  normalizePaperTodoState,
  powerScriptBody,
  splitTodoPaste,
} from './paperTodo';

describe('paper todo core data', () => {
  it('cleans common list prefixes from multi-line paste', () => {
    expect(splitTodoPaste('- first\n2. second\n[ ] third\n\n fourth')).toEqual([
      'first', 'second', 'third', 'fourth',
    ]);
  });

  it('keeps an intentionally empty paper list after normalization', () => {
    const state = normalizePaperTodoState({ version: 1, revision: 8, papers: [], settings: {} });
    expect(state.papers).toHaveLength(0);
    expect(state.revision).toBe(8);
    expect(state.settings.hotkeys.newTodo).toBe('Ctrl+Shift+T');
  });

  it('caps unsafe titles, note content, and todo item size', () => {
    const state = normalizePaperTodoState({
      papers: [{ kind: 'note', title: 'x'.repeat(100), content: 'x'.repeat(600_000), items: [] }],
      settings: { titleMaxLength: 999, interfaceScale: 1 },
    });
    expect(state.papers[0].title).toHaveLength(20);
    expect(state.papers[0].content).toHaveLength(500_000);
    expect(state.settings.titleMaxLength).toBe(20);
    expect(state.settings.interfaceScale).toBe(80);
  });

  it('recognizes script capsules and removes only the marker line', () => {
    const note = createPaper('note');
    note.content = '!power\nWrite-Output "ok"';
    expect(isPowerPaper(note)).toBe(true);
    expect(powerScriptBody(note.content)).toBe('Write-Output "ok"');
    expect(isPowerPaper({ ...note, content: 'plain text' })).toBe(false);
  });

  it('creates stable todo defaults and paper settings', () => {
    expect(createTodoItem(' next ')).toMatchObject({ text: 'next', completed: false, linkedNoteId: null });
    const settings = createDefaultSettings();
    expect(settings.hotkeys.toggleAll).toBe('Ctrl+Shift+Space');
    expect(settings.launcherEnabled).toBe(false);
    expect(settings.launcherEdge).toBe('right');
    expect(settings.launcherDocked).toBe(true);
    expect(settings.launcherMonitor).toBe('');
    expect(settings.launcherX).toBe(100);
    expect(settings.autoCollapseLauncher).toBe(false);
    expect(settings.paperSkin).toBe('classic');
  });

  it('normalizes persisted free launcher placement without breaking legacy settings', () => {
    const legacy = normalizePaperTodoState({ papers: [], settings: { launcherEdge: 'left' } });
    expect(legacy.settings.launcherDocked).toBe(true);
    expect(legacy.settings.launcherX).toBe(0);

    const free = normalizePaperTodoState({
      papers: [],
      settings: {
        launcherDocked: false,
        launcherMonitor: '\\\\.\\DISPLAY2',
        launcherX: 42.5,
        launcherOffset: 63,
      },
    });
    expect(free.settings).toMatchObject({
      launcherDocked: false,
      launcherMonitor: '\\\\.\\DISPLAY2',
      launcherX: 42.5,
      launcherOffset: 63,
    });
  });

  it('keeps the edge launcher enabled for existing papers unless explicitly disabled', () => {
    const paper = createPaper('note');
    expect(normalizePaperTodoState({ papers: [paper], settings: {} }).settings.launcherEnabled).toBe(true);
    expect(normalizePaperTodoState({ papers: [], settings: {} }).settings.launcherEnabled).toBe(false);
    expect(normalizePaperTodoState({
      papers: [paper],
      settings: { launcherEnabled: false },
    }).settings.launcherEnabled).toBe(false);
  });

  it('defaults old and invalid skin settings to classic', () => {
    expect(normalizePaperTodoState({ papers: [], settings: {} }).settings.paperSkin).toBe('classic');
    expect(normalizePaperTodoState({ papers: [], settings: { paperSkin: 'unknown' } }).settings.paperSkin).toBe('classic');
    expect(normalizePaperTodoState({ papers: [], settings: { paperSkin: 'quiet' } }).settings.paperSkin).toBe('quiet');
  });

  it('starts a fresh profile with one todo and one note paper', () => {
    const state = normalizePaperTodoState(null);
    const papers = state.papers;
    expect(papers).toHaveLength(2);
    expect(papers.map((paper) => paper.kind)).toEqual(['todo', 'note']);
    expect(papers.every((paper) => !paper.desktopOpen)).toBe(true);
    expect(state.settings.launcherEnabled).toBe(false);
  });

  it('drops legacy per-paper capsule state during normalization', () => {
    const state = normalizePaperTodoState({
      papers: [{
        ...createPaper('note'),
        collapsed: true,
        geometry: { ...createPaper('note').geometry, dockEdge: 'left' },
      }],
      settings: {
        capsuleMode: true,
        autoDockCapsules: true,
        autoHideDockedCapsules: true,
      },
    });
    expect(state.papers[0]).not.toHaveProperty('collapsed');
    expect(state.papers[0].geometry).not.toHaveProperty('dockEdge');
    expect(state.settings).not.toHaveProperty('capsuleMode');
    expect(state.settings.autoCollapseLauncher).toBe(false);
  });

  it('moves a dragged paper before or after the target without dropping ids', () => {
    const ids = ['todo-1', 'note-1', 'todo-2', 'note-2'];
    expect(movePaperId(ids, 'todo-2', 'note-1', 'before')).toEqual([
      'todo-1', 'todo-2', 'note-1', 'note-2',
    ]);
    expect(movePaperId(ids, 'todo-1', 'todo-2', 'after')).toEqual([
      'note-1', 'todo-2', 'todo-1', 'note-2',
    ]);
    expect(movePaperId(ids, 'missing', 'todo-1', 'before')).toEqual(ids);
  });
});
