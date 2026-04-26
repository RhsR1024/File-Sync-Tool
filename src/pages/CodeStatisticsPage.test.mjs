import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'CodeStatisticsPage.vue'), 'utf8');

test('code statistics relies on the explicit path inputs instead of a second generic project chooser', () => {
  assert.match(pageSource, /旧版本代码路径/);
  assert.match(pageSource, /新版本代码路径/);
  assert.doesNotMatch(pageSource, /codeStatistics\.empty\.noProject/);
  assert.doesNotMatch(pageSource, /codeStatistics\.empty\.actionLoad/);
  assert.doesNotMatch(pageSource, /@action="browseNew"/);
});
