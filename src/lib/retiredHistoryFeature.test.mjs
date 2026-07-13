import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const tauriSource = readFileSync(new URL('./tauri.ts', import.meta.url), 'utf8');
const backendMainSource = readFileSync(new URL('../../src-tauri/src/main.rs', import.meta.url), 'utf8');
const scannerSource = readFileSync(new URL('../../src-tauri/src/scanner.rs', import.meta.url), 'utf8');

test('retired sync history has no frontend or backend command surface', () => {
  assert.doesNotMatch(tauriSource, /HistoryEntry|HistoryStore|getHistory|clearHistory/);
  assert.doesNotMatch(backendMainSource, /history::get_history|history::clear_history/);
  assert.doesNotMatch(scannerSource, /add_history_entry|HistoryEntry/);
});
