import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const shareWebDir = join(__dirname, '..');
const mainSource = readFileSync(join(shareWebDir, 'main.ts'), 'utf8');
const themePath = join(__dirname, 'dialog-theme.css');

const themedDialogs = [
  'UploadDialog.vue',
  'CreateDirectoryDialog.vue',
  'NewTextDialog.vue',
  'ImagePreviewDialog.vue',
  'RenameDialog.vue',
];

test('file share dialogs use the shared light dialog theme', () => {
  assert.match(mainSource, /import '\.\/components\/dialog-theme\.css';/);
  assert.equal(existsSync(themePath), true, 'shared dialog theme should exist');

  const themeSource = readFileSync(themePath, 'utf8');
  assert.match(themeSource, /\.dialog-card,\s*\.preview-card/);
  assert.match(themeSource, /background:\s*var\(--surface\)/);
  assert.match(themeSource, /color:\s*var\(--text\)/);
  assert.match(themeSource, /background:\s*rgba\(34,\s*42,\s*58,\s*0\.34\)/);

  for (const fileName of themedDialogs) {
    const source = readFileSync(join(__dirname, fileName), 'utf8');
    assert.doesNotMatch(source, /background:\s*rgba\((?:3,\s*8,\s*15|7,\s*14,\s*24|6,\s*12,\s*21|4,\s*9,\s*16)/);
    assert.doesNotMatch(source, /color:\s*#(?:eff7ff|dbe7f3|d3e1ef|cde0f3|95abc0)/i);
  }
});
