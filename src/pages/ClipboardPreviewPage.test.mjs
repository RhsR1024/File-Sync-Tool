import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { JSDOM } from 'jsdom';

const __dirname = dirname(fileURLToPath(import.meta.url));
const imagePreviewPageSource = readFileSync(
  join(__dirname, '../../public/clipboard-image-preview.html'),
  'utf8',
);
const textPreviewPageSource = readFileSync(
  join(__dirname, '../../public/clipboard-text-preview.html'),
  'utf8',
);
const previewBackendSource = readFileSync(
  join(__dirname, '../../src-tauri/src/clipboard/preview.rs'),
  'utf8',
);
const previewCommandsSource = readFileSync(
  join(__dirname, '../../src-tauri/src/clipboard/commands.rs'),
  'utf8',
);
const hoverPreviewComposableSource = readFileSync(
  join(__dirname, '../composables/useHoverPreview.ts'),
  'utf8',
);
const tauriApiSource = readFileSync(
  join(__dirname, '../lib/tauri.ts'),
  'utf8',
);
const mainSource = readFileSync(join(__dirname, '../../src-tauri/src/main.rs'), 'utf8');
const tauriConfig = JSON.parse(
  readFileSync(join(__dirname, '../../src-tauri/tauri.conf.json'), 'utf8'),
);
const clipboardPanelCapability = JSON.parse(
  readFileSync(join(__dirname, '../../src-tauri/capabilities/clipboard-panel.json'), 'utf8'),
);
const clipboardPreviewCapability = JSON.parse(
  readFileSync(join(__dirname, '../../src-tauri/capabilities/clipboard-preview.json'), 'utf8'),
);

async function createImagePreviewDom({
  zoomStep = 10,
  clientWidth = 460,
  clientHeight = 360,
  naturalWidth = 800,
  naturalHeight = 600,
} = {}) {
  const dom = new JSDOM(imagePreviewPageSource, {
    runScripts: 'dangerously',
    resources: 'usable',
    pretendToBeVisual: true,
    beforeParse(window) {
      window.__TAURI_INTERNALS__ = {
        convertFileSrc: (path) => path,
        invoke: async (command) => {
          if (command === 'cb_get_image_preview_payload') {
            return {
              id: 1,
              image_path: 'C:/preview.png',
              zoom_step: zoomStep,
              source_app: 'Test',
            };
          }
          if (command === 'cb_toggle_preview_fullscreen') {
            return false;
          }
          return null;
        },
        transformCallback: (callback) => callback,
      };
    },
  });

  await new Promise((resolve) => dom.window.setTimeout(resolve, 0));

  const contentEl = dom.window.document.getElementById('content');
  const imageEl = dom.window.document.getElementById('image');
  assert.ok(contentEl);
  assert.ok(imageEl);

  Object.defineProperty(contentEl, 'clientWidth', { configurable: true, get: () => clientWidth });
  Object.defineProperty(contentEl, 'clientHeight', { configurable: true, get: () => clientHeight });
  Object.defineProperty(imageEl, 'naturalWidth', { configurable: true, get: () => naturalWidth });
  Object.defineProperty(imageEl, 'naturalHeight', { configurable: true, get: () => naturalHeight });
  contentEl.setPointerCapture = () => {};
  contentEl.releasePointerCapture = () => {};
  contentEl.scrollLeft = 0;
  contentEl.scrollTop = 0;
  imageEl.dispatchEvent(new dom.window.Event('load'));

  return {
    dom,
    contentEl,
    imageEl,
    zoomLabelEl: dom.window.document.getElementById('zoom-label'),
  };
}

test('clipboard preview backend uses standalone HTML preview windows instead of hash-route pages', () => {
  assert.match(previewBackendSource, /clipboard-image-preview\.html/);
  assert.match(previewBackendSource, /clipboard-text-preview\.html/);
  assert.doesNotMatch(previewBackendSource, /#\/clipboard-preview\/image/);
  assert.doesNotMatch(previewBackendSource, /#\/clipboard-preview\/text/);
});

test('clipboard preview windows are prewarmed on startup so first hover does not create them', () => {
  assert.match(
    previewBackendSource,
    /pub fn ensure_preview_windows[\s\S]*ensure_preview_window\(/,
  );
  assert.match(mainSource, /clipboard::preview::ensure_preview_windows\(app\)\?/);
});

test('clipboard panel capability remains scoped to the Alt+C panel window only', () => {
  assert.deepEqual(clipboardPanelCapability.windows, ['clipboard-panel']);
});

test('clipboard preview capability is isolated to the standalone preview windows', () => {
  assert.deepEqual(
    clipboardPreviewCapability.windows,
    ['clipboard-image-preview', 'clipboard-text-preview'],
  );
  assert.ok(
    clipboardPreviewCapability.permissions.includes('core:event:allow-listen'),
    'expected preview windows to be able to subscribe to backend update events',
  );
});

test('tauri keeps the global bridge disabled so the preview migration does not affect the clipboard panel window', () => {
  assert.notEqual(tauriConfig.app?.withGlobalTauri, true);
});

test('clipboard image preview page listens for update and clear events and resolves image assets through Tauri internals', () => {
  assert.match(imagePreviewPageSource, /image-preview-update/);
  assert.match(imagePreviewPageSource, /image-preview-clear/);
  assert.match(imagePreviewPageSource, /__TAURI_INTERNALS__/);
  assert.match(imagePreviewPageSource, /convertFileSrc/);
});

test('clipboard preview pages stay interactive so hover and scroll handlers can keep previews alive', () => {
  assert.match(imagePreviewPageSource, /pointer-events:\s*auto/);
  assert.match(textPreviewPageSource, /pointer-events:\s*auto/);
  assert.doesNotMatch(imagePreviewPageSource, /cursor:\s*pointer/);
});

test('clipboard preview pages keep the window surface transparent and clip visuals inside the rounded shell', () => {
  for (const source of [imagePreviewPageSource, textPreviewPageSource]) {
    assert.match(source, /html,\s*body\s*\{[\s\S]*background:\s*transparent/);
    assert.match(source, /body\s*\{[^}]*background:\s*transparent/);
    assert.match(source, /\.shell\s*\{[\s\S]*border-radius:\s*22px/);
    assert.match(source, /\.shell\s*\{[\s\S]*overflow:\s*hidden/);
    assert.doesNotMatch(source, /body\s*\{[^}]*linear-gradient/);
    assert.doesNotMatch(source, /body\s*\{[^}]*radial-gradient/);
  }
});

test('clipboard text preview page listens for update and clear events through Tauri internals', () => {
  assert.match(textPreviewPageSource, /text-preview-update/);
  assert.match(textPreviewPageSource, /text-preview-clear/);
  assert.match(textPreviewPageSource, /__TAURI_INTERNALS__/);
});

test('clipboard text preview content can be partially selected and copied', () => {
  assert.match(textPreviewPageSource, /\.text\s*\{[\s\S]*user-select:\s*text/);
  assert.match(textPreviewPageSource, /\.text\s*\{[\s\S]*cursor:\s*text/);
});

test('clipboard preview backend keeps standalone preview windows interactive instead of click-through', () => {
  assert.match(
    previewBackendSource,
    /show_preview_window[\s\S]*set_ignore_cursor_events\(false\)/,
  );
  assert.doesNotMatch(
    previewBackendSource,
    /show_preview_window[\s\S]*set_ignore_cursor_events\(true\)/,
  );
});

test('clipboard preview windows are shown without activating the Alt+C panel away', () => {
  assert.match(
    previewBackendSource,
    /WebviewWindowBuilder::new[\s\S]*\.focused\(false\)/,
  );
  assert.match(
    previewBackendSource,
    /show_preview_window[\s\S]*show_preview_without_focus\(&window,\s*&panel\)/,
  );
});

test('clipboard preview backend keeps preview z-order behind the Alt+C panel', () => {
  assert.match(previewBackendSource, /show_preview_without_focus[\s\S]*restack_preview_behind_panel/);
  assert.match(previewBackendSource, /restack_preview_behind_panel[\s\S]*let insert_after = panel/);
  assert.match(previewBackendSource, /SetWindowPos[\s\S]*insert_after/);
});

test('clipboard preview focus detection uses the native foreground HWND instead of tauri focus state', () => {
  assert.match(
    previewBackendSource,
    /fn foreground_root_hwnd\(\)[\s\S]*GetForegroundWindow/,
  );
  assert.match(
    previewBackendSource,
    /pub fn preview_window_is_focused[\s\S]*foreground_root_hwnd\(\)/,
  );
  assert.doesNotMatch(
    previewBackendSource,
    /pub fn preview_window_is_focused[\s\S]*window\.is_focused\(\)/,
  );
});

test('clipboard preview backend keeps delayed debug snapshots for post-show diagnostics', () => {
  assert.match(previewBackendSource, /fn schedule_debug_snapshots/);
  assert.match(previewBackendSource, /for delay_ms in \[50_u64,\s*300,\s*1000\]/);
});

test('clipboard hover preview show and hide requests are token guarded against stale async shows', () => {
  assert.match(hoverPreviewComposableSource, /activePreviewToken/);
  assert.match(
    hoverPreviewComposableSource,
    /showImagePreview\(target\.id,\s*token\)/,
  );
  assert.match(
    hoverPreviewComposableSource,
    /hidePreview\(token\)/,
  );
  assert.match(
    tauriApiSource,
    /showImagePreview:\s*\(id: number,\s*token: number\)/,
  );
  assert.match(
    previewCommandsSource,
    /cb_show_image_preview[\s\S]*token: Option<u64>/,
  );
  assert.match(previewBackendSource, /cancel_preview_token/);
});

test('clipboard preview backend exposes a fullscreen toggle and an orphan-dismiss debounce shared with the panel', () => {
  assert.match(previewBackendSource, /pub fn toggle_preview_fullscreen/);
  assert.match(previewBackendSource, /pub fn schedule_dismiss_if_orphaned/);
  assert.match(previewBackendSource, /pub fn attach_preview_dismiss_handlers/);
  assert.match(mainSource, /attach_preview_dismiss_handlers\(/);
  assert.match(mainSource, /schedule_dismiss_if_orphaned\(/);
});

test('clipboard preview pages emit hover events so the panel can keep the preview alive while interacted with', () => {
  for (const source of [imagePreviewPageSource, textPreviewPageSource]) {
    assert.match(source, /clipboard-preview-mouse-enter/);
    assert.match(source, /clipboard-preview-mouse-leave/);
    assert.match(source, /plugin:event\|emit/);
  }
});

test('clipboard image preview page renders the picture fit-to-window by default and offers a fullscreen control', () => {
  assert.match(imagePreviewPageSource, /object-fit: contain/);
  assert.match(imagePreviewPageSource, /max-width: 100%/);
  assert.match(imagePreviewPageSource, /max-height: 100%/);
  assert.match(imagePreviewPageSource, /id="fullscreen"/);
  assert.match(imagePreviewPageSource, /cb_toggle_preview_fullscreen/);
});

test('clipboard image preview keeps fit-to-window as the minimum zoom level', () => {
  assert.match(imagePreviewPageSource, /const MIN_SCALE = FIT_SCALE/);
  assert.match(imagePreviewPageSource, /zoomOutBtn\.disabled = scale <= FIT_SCALE/);
});

test('clipboard image preview supports drag-to-pan when zoomed in', () => {
  assert.match(imagePreviewPageSource, /cursor:\s*grab/);
  assert.match(imagePreviewPageSource, /cursor:\s*grabbing/);
  assert.match(imagePreviewPageSource, /classList\.toggle\('can-pan'/);
  assert.match(imagePreviewPageSource, /classList\.toggle\('is-panning'/);
  assert.match(imagePreviewPageSource, /contentEl\.addEventListener\('pointerdown'/);
  assert.match(imagePreviewPageSource, /contentEl\.scrollLeft\s*=/);
  assert.match(imagePreviewPageSource, /contentEl\.scrollTop\s*=/);
});

test('clipboard image preview zooms through layout size so drag-to-pan moves the real scroll area', () => {
  assert.match(imagePreviewPageSource, /function getScaledImageSize/);
  assert.match(imagePreviewPageSource, /function getImageLayoutMetrics/);
  assert.match(imagePreviewPageSource, /function getScrollTargetForScale/);
  assert.match(imagePreviewPageSource, /imageEl\.style\.width = `\$\{imageMetrics\.imageWidth}px`/);
  assert.match(imagePreviewPageSource, /imageEl\.style\.height = `\$\{imageMetrics\.imageHeight}px`/);
  assert.doesNotMatch(imagePreviewPageSource, /imageEl\.style\.transform = `scale\(\$\{scale\}\)`/);
});

test('clipboard image preview hides scrollbars while keeping drag-pan available', () => {
  assert.match(imagePreviewPageSource, /\.content\s*\{[\s\S]*scrollbar-width:\s*none/);
  assert.match(imagePreviewPageSource, /\.content\s*\{[\s\S]*-ms-overflow-style:\s*none/);
  assert.match(imagePreviewPageSource, /\.content::-webkit-scrollbar\s*\{[\s\S]*display:\s*none/);
  assert.doesNotMatch(imagePreviewPageSource, /Ctrl\+wheel/);
});

test('clipboard image preview uses the Windows-like 10% multiplicative wheel zoom curve without ctrl', async () => {
  const { dom, contentEl, imageEl, zoomLabelEl } = await createImagePreviewDom({ zoomStep: 25 });
  assert.equal(zoomLabelEl?.textContent, '100%');

  const wheelUp = new dom.window.WheelEvent('wheel', {
    bubbles: true,
    cancelable: true,
    deltaY: -120,
  });
  contentEl.dispatchEvent(wheelUp);

  assert.equal(zoomLabelEl?.textContent, '110%');
  assert.equal(imageEl.style.width, '471px');
  assert.equal(imageEl.style.height, '353px');

  contentEl.dispatchEvent(new dom.window.WheelEvent('wheel', {
    bubbles: true,
    cancelable: true,
    deltaY: -120,
  }));

  assert.equal(zoomLabelEl?.textContent, '121%');
  assert.equal(imageEl.style.width, '518px');
  assert.equal(imageEl.style.height, '388px');

  const wheelDown = new dom.window.WheelEvent('wheel', {
    bubbles: true,
    cancelable: true,
    deltaY: 120,
  });
  contentEl.dispatchEvent(wheelDown);

  assert.equal(zoomLabelEl?.textContent, '110%');
  assert.equal(imageEl.style.width, '471px');
  assert.equal(imageEl.style.height, '353px');

  contentEl.dispatchEvent(new dom.window.WheelEvent('wheel', {
    bubbles: true,
    cancelable: true,
    deltaY: 120,
  }));

  assert.equal(zoomLabelEl?.textContent, '100%');
  assert.equal(imageEl.style.width, '428px');
  assert.equal(imageEl.style.height, '321px');
});

test('clipboard image preview caps plain-wheel zoom at 600%', async () => {
  const { dom, contentEl, zoomLabelEl } = await createImagePreviewDom({ zoomStep: 25 });

  for (let index = 0; index < 80; index += 1) {
    const wheelUp = new dom.window.WheelEvent('wheel', {
      bubbles: true,
      cancelable: true,
      deltaY: -120,
    });
    contentEl.dispatchEvent(wheelUp);
  }

  assert.equal(zoomLabelEl?.textContent, '600%');
});

test('clipboard image preview keeps the current zoom when the preview window gains focus', async () => {
  const { dom, contentEl, zoomLabelEl } = await createImagePreviewDom({ zoomStep: 25 });

  contentEl.dispatchEvent(new dom.window.WheelEvent('wheel', {
    bubbles: true,
    cancelable: true,
    deltaY: -120,
  }));
  assert.equal(zoomLabelEl?.textContent, '110%');

  dom.window.dispatchEvent(new dom.window.Event('focus'));
  await new Promise((resolve) => dom.window.setTimeout(resolve, 0));

  assert.equal(zoomLabelEl?.textContent, '110%');
});

test('clipboard image preview anchors wheel zoom to the mouse position instead of the image center', async () => {
  const { dom, contentEl } = await createImagePreviewDom({ zoomStep: 25 });
  contentEl.getBoundingClientRect = () => ({
    x: 100,
    y: 50,
    left: 100,
    top: 50,
    right: 560,
    bottom: 410,
    width: 460,
    height: 360,
    toJSON() { return this; },
  });

  contentEl.dispatchEvent(new dom.window.WheelEvent('wheel', {
    bubbles: true,
    cancelable: true,
    deltaY: -120,
    clientX: 520,
    clientY: 180,
  }));

  assert.ok(
    contentEl.scrollLeft > 35,
    `expected zoom near the right edge to keep the pointer anchor, got scrollLeft=${contentEl.scrollLeft}`,
  );
});

test('clipboard image preview keeps the image centered on non-overflow axes while zooming portrait images', async () => {
  const { dom, contentEl, imageEl } = await createImagePreviewDom({
    zoomStep: 25,
    naturalWidth: 600,
    naturalHeight: 1200,
  });
  contentEl.getBoundingClientRect = () => ({
    x: 100,
    y: 50,
    left: 100,
    top: 50,
    right: 560,
    bottom: 410,
    width: 460,
    height: 360,
    toJSON() { return this; },
  });

  contentEl.dispatchEvent(new dom.window.WheelEvent('wheel', {
    bubbles: true,
    cancelable: true,
    deltaY: -120,
    clientX: 314,
    clientY: 210,
  }));

  assert.equal(imageEl.style.marginLeft, '124px');
  assert.equal(imageEl.style.marginRight, '124px');
});

test('clipboard image preview does not animate width or height during wheel zoom', () => {
  assert.doesNotMatch(imagePreviewPageSource, /transition:\s*width 80ms ease-out,\s*height 80ms ease-out/);
});

test('clipboard preview commands log request entry so native diagnostics can distinguish missing hovers from failed window creation', () => {
  assert.match(previewCommandsSource, /command:show-image/);
  assert.match(previewCommandsSource, /command:show-text/);
  assert.match(previewCommandsSource, /command:hide/);
  assert.match(previewCommandsSource, /command:show-image:(ok|error)/);
  assert.match(previewCommandsSource, /command:show-text:(ok|error)/);
});

test('clipboard preview backend logs early-exit reasons and prepared window placement for diagnostics', () => {
  assert.match(previewBackendSource, /request:image/);
  assert.match(previewBackendSource, /request:text/);
  assert.match(previewBackendSource, /prepare:/);
  assert.match(previewBackendSource, /error stage=/);
  assert.match(previewBackendSource, /preview_stage_ok\(target_label,\s*token,\s*"after-show"\)/);
});
