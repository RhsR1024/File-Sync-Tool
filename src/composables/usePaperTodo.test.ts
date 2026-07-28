import { describe, expect, it } from 'vitest';

import { usePaperTodo } from './usePaperTodo';
import { createDefaultState, createPaper, createTodoItem } from '@/lib/paperTodo';

describe('paper todo reactive history', () => {
  it('records history from Vue reactive papers without cloning errors', async () => {
    const store = usePaperTodo();
    store.state.value = createDefaultState();
    store.state.value.papers.push(createPaper('todo'));
    const id = store.state.value.papers[0].id;

    expect(() => store.updatePaper(id, (paper) => {
      paper.items.push(createTodoItem('component action'));
    }, { history: true })).not.toThrow();

    expect(store.state.value.papers[0].items[0].text).toBe('component action');
    expect(store.canUndo(id)).toBe(true);
    await store.flush();
  });
});
