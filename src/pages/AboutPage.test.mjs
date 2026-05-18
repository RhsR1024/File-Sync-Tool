import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'AboutPage.vue'), 'utf8');

test('about update banner summarizes long changelogs before expanding details', () => {
  assert.match(pageSource, /const showLatestChangelog = ref\(false\)/);
  assert.match(pageSource, /latestEntry\.value\?\.changelog\.slice\(0,\s*3\)/);
  assert.match(pageSource, /const visibleLatestChangelog = computed/);
  assert.match(pageSource, /about\.changelogSummary/);
  assert.match(pageSource, /about\.showAllChangelog/);
  assert.match(pageSource, /about\.hideChangelog/);
  assert.match(pageSource, /max-h-\[46vh\][^']*overflow-y-auto/);
  assert.doesNotMatch(
    pageSource,
    /v-for="\([^"]+\) in latestEntry\.changelog"/,
    'new-version banner should not render the full changelog by default',
  );
});
