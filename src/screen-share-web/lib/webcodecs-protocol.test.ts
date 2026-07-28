import { describe, expect, it } from 'vitest';

import {
  WEBCODECS_AU_HEADER_BYTES,
  encodeWebCodecsAccessUnit,
  parseWebCodecsAccessUnit,
  parseWebCodecsMediaHello,
  parseWebCodecsMediaUnavailable,
} from './webcodecs-protocol';

describe('WebCodecs H.264 wire protocol', () => {
  it('round-trips one complete AVCC access unit and its timing metadata', () => {
    // Two length-prefixed NAL units in one access unit. The protocol must not
    // split these into separate EncodedVideoChunk instances.
    const payload = new Uint8Array([0, 0, 0, 2, 0x67, 0x64, 0, 0, 0, 3, 0x65, 1, 2]);
    const encoded = encodeWebCodecsAccessUnit({
      generation: 7n,
      sequence: 99n,
      timestampUs: 1_234_567n,
      durationUs: 16_667,
      key: true,
      delta: false,
      discontinuity: true,
      payload,
    });

    expect(encoded.byteLength).toBe(WEBCODECS_AU_HEADER_BYTES + payload.byteLength);
    expect(parseWebCodecsAccessUnit(encoded)).toEqual({
      generation: 7n,
      sequence: 99n,
      timestampUs: 1_234_567n,
      durationUs: 16_667,
      key: true,
      delta: false,
      discontinuity: true,
      payload,
    });
  });

  it('rejects truncated, length-mismatched and ambiguous frames', () => {
    expect(parseWebCodecsAccessUnit(new Uint8Array(39))).toBeNull();
    const encoded = encodeWebCodecsAccessUnit({
      generation: 1n,
      sequence: 1n,
      timestampUs: 0n,
      durationUs: 33_333,
      key: false,
      delta: true,
      discontinuity: false,
      payload: new Uint8Array([0, 0, 0, 1, 0x41]),
    });
    new DataView(encoded.buffer).setUint32(36, 100);
    expect(parseWebCodecsAccessUnit(encoded)).toBeNull();
    new DataView(encoded.buffer).setUint32(36, 5);
    new DataView(encoded.buffer).setUint8(5, 3);
    expect(parseWebCodecsAccessUnit(encoded)).toBeNull();
    new DataView(encoded.buffer).setUint8(5, 1);
    new DataView(encoded.buffer).setUint32(32, 0);
    expect(parseWebCodecsAccessUnit(encoded)).toBeNull();
    new DataView(encoded.buffer).setUint32(32, 33_333);
    new DataView(encoded.buffer).setBigUint64(8, 0n);
    expect(parseWebCodecsAccessUnit(encoded)).toBeNull();
    new DataView(encoded.buffer).setBigUint64(8, 1n);
    new DataView(encoded.buffer).setBigUint64(16, 0n);
    expect(parseWebCodecsAccessUnit(encoded)).toBeNull();
    expect(() => encodeWebCodecsAccessUnit({
      generation: 1n,
      sequence: 1n,
      timestampUs: 0n,
      durationUs: 0,
      key: true,
      delta: false,
      discontinuity: false,
      payload: new Uint8Array([0, 0, 0, 1, 0x65]),
    })).toThrow('duration');
  });

  it('validates the decoder configuration hello', () => {
    expect(parseWebCodecsMediaHello(JSON.stringify({
      v: 1,
      type: 'media.hello',
      transport: 'webcodecs_h264',
      generation: 2,
      codec: 'avc1.42C028',
      description_base64: 'AWQAKP/hAA==',
      width: 1920,
      height: 1080,
      fps: 60,
      bitrate_bps: 8_000_000,
    }))).toMatchObject({ generation: 2, codec: 'avc1.42C028' });
    expect(parseWebCodecsMediaHello('{"transport":"mse_h264"}')).toBeNull();
  });

  it('validates an encoder-unavailable control message', () => {
    expect(parseWebCodecsMediaUnavailable(JSON.stringify({
      v: 1,
      type: 'media.unavailable',
      generation: 4,
      error: 'hardware encoder stopped',
    }))).toEqual({
      v: 1,
      type: 'media.unavailable',
      generation: 4,
      error: 'hardware encoder stopped',
    });
    expect(parseWebCodecsMediaUnavailable(JSON.stringify({
      v: 1,
      type: 'media.unavailable',
      generation: 0,
      error: '',
    }))).toBeNull();
  });
});
