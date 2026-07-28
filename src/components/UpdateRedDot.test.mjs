import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(__dirname, 'UpdateRedDot.vue'), 'utf8');

test('update badge variants keep an accessible label and reduced-motion fallback', () => {
  assert.match(source, /'halo' \| 'bounce' \| 'radar' \| 'shimmer'/);
  assert.match(source, /variant === 'halo'/);
  assert.match(source, /variant === 'bounce'/);
  assert.match(source, /variant === 'radar'/);
  assert.match(source, /<ArrowUp/);
  assert.match(source, />\s*NEW\s*</);
  assert.match(source, /radar-core/);
  assert.match(source, /shimmerLabel/);
  assert.match(source, /role="status"[^>]*:aria-label="t\('sidebar\.updateAvailable'\)"/);
  assert.match(source, /@media \(prefers-reduced-motion: reduce\)/);
});
