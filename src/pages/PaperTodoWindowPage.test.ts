import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

const pageSource = readFileSync(
  resolve(process.cwd(), 'src/pages/PaperTodoWindowPage.vue'),
  'utf8',
);
const backendSource = readFileSync(
  resolve(process.cwd(), 'src-tauri/src/paper_todo.rs'),
  'utf8',
);
const styleSource = readFileSync(
  resolve(process.cwd(), 'src/style.css'),
  'utf8',
);
const paperSource = readFileSync(
  resolve(process.cwd(), 'src/components/paper-todo/PaperTodoPaper.vue'),
  'utf8',
);
const launcherSource = readFileSync(
  resolve(process.cwd(), 'src/pages/PaperTodoLauncherPage.vue'),
  'utf8',
);
const mainSource = readFileSync(
  resolve(process.cwd(), 'src-tauri/src/main.rs'),
  'utf8',
);

describe('paper todo standalone window lifecycle', () => {
  it('keeps the native window hidden until the paper route is ready', () => {
    expect(backendSource).toMatch(/\.visible\(false\)/);
    expect(pageSource).toMatch(/await currentWindow\.show\(\)/);
  });

  it('uses a transparent document canvas and exposes a closeable error state', () => {
    expect(pageSource).toContain("const PAPER_WINDOW_CLASS = 'paper-todo-window'");
    expect(styleSource).toMatch(/html\.paper-todo-window,[\s\S]*background:\s*transparent/);
    expect(pageSource).toMatch(/v-else-if="store\.error\.value"/);
    expect(pageSource).toMatch(/@click="getCurrentWindow\(\)\.close\(\)"/);
  });

  it('creates runtime windows away from the WebView callback thread', () => {
    expect(backendSource).toMatch(/pub async fn paper_todo_create_paper[\s\S]*?spawn_blocking/);
    expect(backendSource).toMatch(/pub async fn paper_todo_open_window[\s\S]*?spawn_blocking/);
    expect(backendSource).toMatch(/pub async fn paper_todo_set_all_windows[\s\S]*?spawn_blocking/);
    expect(mainSource).toContain('paper_todo::dispatch_background(app.clone(), "newTodo")');
    expect(mainSource).toContain('paper_todo::dispatch_background(app.clone(), "newNote")');
  });

  it('keeps deletion non-modal and closes only after persistence returns', () => {
    const deleteCommand = backendSource.slice(
      backendSource.indexOf('pub fn paper_todo_delete_paper'),
      backendSource.indexOf('pub fn paper_todo_save_settings'),
    );
    expect(deleteCommand).not.toContain('.close()');
    expect(paperSource).not.toContain("window.confirm(t('paperTodo.confirmDeletePaper'))");
    expect(paperSource).toContain('await store.removePaper(id)');
  });

  it('provides dedicated drag handles for the edge launcher and paper window', () => {
    expect(launcherSource).toContain('launcher-drag-handle');
    expect(launcherSource).toContain('await getCurrentWindow().startDragging()');
    expect(launcherSource).toContain('await savePaperLauncherPosition()');
    expect(paperSource).toContain('paper-window-drag-handle');
    expect(paperSource).toContain("edge: 'nearest'");
  });
});
