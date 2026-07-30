import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'ScreenShareAnnotationBarPage.vue'), 'utf8');
const routerSource = readFileSync(join(__dirname, '..', 'router', 'index.ts'), 'utf8');
const messagesSource = readFileSync(join(__dirname, '..', 'locales', 'messages.ts'), 'utf8');

test('the annotation bar is reachable as its own chromeless window route', () => {
  assert.match(routerSource, /path: '\/screen-share-annotation-bar'/);
  assert.match(
    routerSource,
    /'\/screen-share-annotation-bar'[\s\S]*?ScreenShareAnnotationBarPage\.vue[\s\S]*?noLayout: true/,
  );
});

test('the bar derives its state from the shared annotation document', () => {
  // The visibility and undo-target rules are unit tested in
  // src/screen-share-web/lib/annotation-bar.test.ts; the page must not fork them.
  assert.match(pageSource, /annotationBarView\(documentState\.value, dismissedAtCount\.value\)/);
  assert.match(pageSource, /carryDismissal\(count, dismissedAtCount\.value\)/);
  assert.doesNotMatch(pageSource, /shapes\.filter/);
});

test('the bar listens to the same annotation event as the desktop overlay', () => {
  assert.match(pageSource, /listen<unknown>\('screen-share-annotation-state'/);
  // Stale revisions for the same session must never roll the bar backwards.
  assert.match(pageSource, /next\.sourceEpoch < current\.sourceEpoch/);
  assert.match(pageSource, /next\.revision < current\.revision/);
});

test('visibility goes through Rust so the bar never steals focus', () => {
  assert.match(pageSource, /screenShareSetAnnotationBarVisible\(visible\)/);
  assert.doesNotMatch(pageSource, /getCurrentWindow\(\)\.show\(\)/);
  assert.doesNotMatch(pageSource, /setFocus/);
});

test('the bar clears through the existing annotation commands', () => {
  assert.match(pageSource, /screenShareRemoveAnnotation\(latest\)/);
  assert.match(pageSource, /screenShareClearAnnotations\(\)/);
});

test('the bar copy is defined in both locales', () => {
  for (const key of [
    'annotationBarCount',
    'annotationBarUndo',
    'annotationBarClear',
    'annotationBarHide',
    'annotationBarFailed',
  ]) {
    assert.match(pageSource, new RegExp(`tools\\.screenShare\\.${key}`));
    const occurrences = messagesSource.match(new RegExp(`${key}:`, 'g')) ?? [];
    assert.equal(occurrences.length, 2, `${key} must exist in en-US and zh-CN`);
  }
});
