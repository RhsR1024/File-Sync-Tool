import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

const pageSource = readFileSync(
  fileURLToPath(new URL('./PaperTodoWindowPage.vue', import.meta.url)),
  'utf8',
);
const backendSource = readFileSync(
  fileURLToPath(new URL('../../src-tauri/src/paper_todo.rs', import.meta.url)),
  'utf8',
);
const styleSource = readFileSync(
  fileURLToPath(new URL('../style.css', import.meta.url)),
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
