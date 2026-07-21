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
});
