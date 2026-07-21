import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  buildMediaWebSocketUrl,
  lowLatencySeekTarget,
  parseMediaHello,
  supportsMseH264,
} from './mse-player';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('screen share MSE player helpers', () => {
  it('builds initial and reconnect media socket URLs', () => {
    expect(new URL(buildMediaWebSocketUrl()).pathname).toBe('/media/ws');
    expect(new URL(buildMediaWebSocketUrl()).searchParams.has('reconnect')).toBe(false);
    expect(new URL(buildMediaWebSocketUrl(true)).searchParams.get('reconnect')).toBe('1');
  });

  it('accepts only a complete H.264 media hello', () => {
    const message = JSON.stringify({
      v: 1,
      type: 'media.hello',
      transport: 'mse_h264',
      generation: 3,
      codec: 'avc1.42C028',
      mime_type: 'video/mp4; codecs="avc1.42C028"',
      width: 1920,
      height: 1080,
      fps: 15,
      bitrate_bps: 5_000_000,
    });
    expect(parseMediaHello(message)).toMatchObject({ generation: 3, width: 1920, height: 1080 });
    expect(parseMediaHello(message.replace('avc1.42C028', 'h264'))).toBeNull();
    expect(parseMediaHello('{}')).toBeNull();
  });

  it('delegates codec capability checks to MediaSource', () => {
    const check = vi.fn().mockReturnValue(true);
    vi.stubGlobal('MediaSource', class {
      static isTypeSupported = check;
    });
    expect(supportsMseH264('video/mp4; codecs="avc1.42C028"')).toBe(true);
    expect(check).toHaveBeenCalledWith('video/mp4; codecs="avc1.42C028"');
  });

  it('keeps playback close to the live edge for remote-control feedback', () => {
    expect(lowLatencySeekTarget(4, 5, 6)).toBeCloseTo(5.95);
    expect(lowLatencySeekTarget(5.5, 5, 6)).toBeCloseTo(5.95);
    expect(lowLatencySeekTarget(5.82, 5, 6)).toBeNull();
    expect(lowLatencySeekTarget(Number.NaN, 5, 6)).toBeNull();
  });
});
