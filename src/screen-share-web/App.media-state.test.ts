import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const appSource = readFileSync(resolve(process.cwd(), 'src/screen-share-web/App.vue'), 'utf8');

describe('screen share viewer media visibility', () => {
  it('keeps replacement images hidden until the new frame loads', () => {
    expect(appSource).toMatch(/function setImageSource\([^)]*\)\s*\{[\s\S]*?imageReady\.value = false;[\s\S]*?imageSource\.value = source;/);
    expect(appSource).toMatch(/function markImageLoaded\(\)\s*\{[\s\S]*?imageReady\.value = true;/);
    expect(appSource).toContain(":class=\"{ 'is-image-error': !imageReady }\"");
    expect(appSource).toContain('v-if="statusActive && !showPrimaryMedia && !imageReady"');
  });

  it('does not expose broken-image alternative text during reconnects', () => {
    expect(appSource).toMatch(/<img[\s\S]*?alt=""[\s\S]*?aria-hidden="true"/);
  });

  it('does not reconnect media for annotation-only document revisions', () => {
    expect(appSource).toMatch(/const mediaStateChanged = incoming\.sessionId !== current\.sessionId\s*\|\| incoming\.sourceEpoch !== current\.sourceEpoch;/);
    expect(appSource).toContain('if (mediaStateChanged) startLiveStream();');
  });

  it('keeps automatic MJPEG fallback without legacy polling, pause, or freeze controls', () => {
    expect(appSource).toContain('setImageSource(`/stream?reconnect=1&t=${Date.now()}`)');
    expect(appSource).not.toContain('/stream?single=1');
    expect(appSource).not.toContain('/snapshot/');
    expect(appSource).not.toContain('refreshRateMs');
    expect(appSource).not.toContain('toggleLocalPause');
    expect(appSource).not.toContain('shared_freeze_enabled');
    expect(appSource).not.toContain("message.type === 'view.state'");
    expect(appSource).not.toContain("sessionClient.send('view.freeze')");
    expect(appSource).not.toContain("sessionClient.send('view.resume')");
  });

  it('aborts the MJPEG request when a primary player takes over', () => {
    // Dropping the <img> via v-if does not cancel a multipart response, so the
    // host would keep a phantom MJPEG consumer and run the JPEG encoder for the
    // whole session on top of H.264. Clearing `src` is the actual abort.
    expect(appSource).toMatch(/function stopMjpegStream\(\)\s*\{[\s\S]*?mjpegConnected\.value = false;[\s\S]*?imageReady\.value = false;[\s\S]*?screenImage\.value\?\.removeAttribute\('src'\);/);
    for (const handler of ['handleH264PlayerState', 'handleWebCodecsPlayerState', 'handleWebRtcPlayerState']) {
      const body = appSource.slice(appSource.indexOf(`function ${handler}(`));
      const readyBranch = body.slice(0, body.indexOf('nextTick(updateGeometry);'));
      expect(readyBranch, handler).toContain('stopMjpegStream();');
    }
  });

  it('forwards DOM pointer timestamps to the session transport', () => {
    expect(appSource).toContain("sessionClient.send('input.pointer_move', point, eventOccurredAtMs)");
    expect(appSource).toContain("sessionClient.send('input.pointer_button', payload, eventOccurredAtMs)");
  });
});
