import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const appSource = readFileSync(resolve(process.cwd(), 'src/screen-share-web/App.vue'), 'utf8');

describe('screen share viewer media visibility', () => {
  it('keeps replacement images hidden until the new frame loads', () => {
    expect(appSource).toMatch(/function setImageSource\([^)]*\)\s*\{[\s\S]*?imageReady\.value = false;[\s\S]*?imageSource\.value = source;/);
    expect(appSource).toMatch(/function markImageLoaded\(\)\s*\{[\s\S]*?imageReady\.value = true;/);
    expect(appSource).toContain(":class=\"{ 'is-image-error': !imageReady }\"");
    expect(appSource).toContain('v-if="statusActive && !showH264Video && !imageReady"');
  });

  it('does not expose broken-image alternative text during reconnects', () => {
    expect(appSource).toMatch(/<img[\s\S]*?alt=""[\s\S]*?aria-hidden="true"/);
  });

  it('does not reconnect media for annotation-only document revisions', () => {
    expect(appSource).toMatch(/const mediaStateChanged = incoming\.sessionId !== current\.sessionId[\s\S]*?incoming\.frozenFrameId !== current\.frozenFrameId;/);
    expect(appSource).toContain('if (!localPaused.value && mediaStateChanged)');
  });
});
