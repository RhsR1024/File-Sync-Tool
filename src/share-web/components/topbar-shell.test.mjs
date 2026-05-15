import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const shareWebDir = join(__dirname, '..');

test('file share top bar matches tools hub share branding and keeps only the left divider', () => {
  const topBarSource = readFileSync(join(__dirname, 'TopBar.vue'), 'utf8');
  const iconsSource = readFileSync(join(__dirname, 'icons.ts'), 'utf8');
  const styleSource = readFileSync(join(shareWebDir, 'style.css'), 'utf8');

  assert.match(topBarSource, /<Icon name="share" \/>/);
  assert.doesNotMatch(topBarSource, />FS</);
  assert.match(iconsSource, /\bshare:\s*\{/);

  const topbarBlock = styleSource.match(/\.topbar\s*\{[\s\S]*?\n\}/)?.[0] ?? '';
  const brandBlocks = Array.from(styleSource.matchAll(/\.brand\s*\{[\s\S]*?\n\}/g), (match) => match[0]);
  const brandMarkBlock = styleSource.match(/\.brand-mark\s*\{[\s\S]*?\n\}/)?.[0] ?? '';

  assert.doesNotMatch(topbarBlock, /border-bottom/);
  assert.ok(
    brandBlocks.some((block) => /border-right:\s*1px solid var\(--border\)/.test(block)),
    'brand area should carry the only top divider as a left-column vertical line',
  );
  assert.match(brandMarkBlock, /linear-gradient\(135deg,\s*#06b6d4 0%,\s*#0d9488 100%\)/);
  assert.match(styleSource, /\.topbar-actions\s*\{/);
});
