import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'ScreenShareAnnotationBarPage.vue'), 'utf8');
const routerSource = readFileSync(join(__dirname, '..', 'router', 'index.ts'), 'utf8');
const messagesSource = readFileSync(join(__dirname, '..', 'locales', 'messages.ts'), 'utf8');
const capabilitySource = readFileSync(
  join(__dirname, '..', '..', 'src-tauri', 'capabilities', 'screen-share-annotation-bar.json'),
  'utf8',
);
const rustSource = readFileSync(
  join(__dirname, '..', '..', 'src-tauri', 'src', 'screenshare.rs'),
  'utf8',
);

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
  assert.match(pageSource, /function requestBarVisibility\(visible: boolean\)/);
  assert.match(pageSource, /visibilitySync = visibilitySync\.then/);
  assert.match(pageSource, /screenShareSetAnnotationBarVisible\(visible\)/);
  assert.match(pageSource, /await annotationBarWindow\.hide\(\)/);
  assert.match(capabilitySource, /core:window:allow-hide/);
  assert.match(pageSource, /void requestBarVisibility\(false\)/);
  assert.match(pageSource, /await requestBarVisibility\(shouldShow\.value\)/);
  assert.match(pageSource, /v-if="shouldShow"[\s\S]*?class="bar-shell"/);
  assert.doesNotMatch(pageSource, /if \(shouldShow\.value\).*SetAnnotationBarVisible/);
  assert.doesNotMatch(pageSource, /getCurrentWindow\(\)\.show\(\)/);
  assert.doesNotMatch(pageSource, /setFocus/);
});

test('the bar can be moved without losing button interaction or snapping back', () => {
  assert.match(pageSource, /class="bar-shell"[\s\S]*?@mousedown="startDragging"/);
  assert.match(pageSource, /\(event\.target as Element\)\.closest\('button'\)/);
  assert.match(pageSource, /\.bar-shell[\s\S]*?cursor: move/);
  assert.match(pageSource, /annotationBarWindow\.startDragging\(\)/);
  assert.doesNotMatch(pageSource, /GripVertical/);
  assert.match(capabilitySource, /core:window:allow-start-dragging/);
  assert.doesNotMatch(
    rustSource.match(/fn sync_annotation_bar_window[\s\S]*?\n}/)?.[0] ?? '',
    /set_position/,
  );
});

test('the complete count has enough room and the bar has no shadow', () => {
  assert.match(rustSource, /ANNOTATION_BAR_LOGICAL_WIDTH: f64 = 440\.0/);
  assert.match(rustSource, /\.shadow\(false\)/);
  assert.doesNotMatch(pageSource, /box-shadow/);
  assert.doesNotMatch(pageSource, /text-overflow/);
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
    'annotationBarMove',
    'annotationBarFailed',
  ]) {
    assert.match(pageSource, new RegExp(`tools\\.screenShare\\.${key}`));
    const occurrences = messagesSource.match(new RegExp(`${key}:`, 'g')) ?? [];
    assert.equal(occurrences.length, 2, `${key} must exist in en-US and zh-CN`);
  }
});
