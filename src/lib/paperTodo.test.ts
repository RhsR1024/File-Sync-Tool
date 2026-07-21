import { describe, expect, it } from 'vitest';

import {
  createDefaultSettings,
  createPaper,
  createTodoItem,
  isPowerPaper,
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
    expect(settings.launcherEnabled).toBe(true);
    expect(settings.launcherEdge).toBe('right');
  });
});
