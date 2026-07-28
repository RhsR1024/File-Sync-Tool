export const WEBCODECS_AU_MAGIC = 0x46535457; // "FSTW"
export const WEBCODECS_AU_VERSION = 1;
export const WEBCODECS_AU_HEADER_BYTES = 40;
export const WEBCODECS_MAX_ACCESS_UNIT_BYTES = 32 * 1024 * 1024;

export const enum WebCodecsAccessUnitFlag {
  Key = 1 << 0,
  Delta = 1 << 1,
  Discontinuity = 1 << 2,
}

export interface WebCodecsAccessUnit {
  generation: bigint;
  sequence: bigint;
  timestampUs: bigint;
  durationUs: number;
  key: boolean;
  delta: boolean;
  discontinuity: boolean;
  /** One complete AVC-format access unit (4-byte length-prefixed NAL units). */
  payload: Uint8Array;
}

export interface WebCodecsMediaHello {
  v: 1;
  type: 'media.hello';
  transport: 'webcodecs_h264';
  generation: number;
  codec: string;
  description_base64: string;
  width: number;
  height: number;
  fps: number;
  bitrate_bps: number;
}

export interface WebCodecsMediaUnavailable {
  v: 1;
  type: 'media.unavailable';
  generation: number;
  error: string;
}

function asUint8Array(value: ArrayBuffer | ArrayBufferView): Uint8Array {
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
}

export function parseWebCodecsMediaHello(value: unknown): WebCodecsMediaHello | null {
  if (typeof value !== 'string') return null;
  try {
    const parsed = JSON.parse(value) as Record<string, unknown>;
    if (
      parsed.v !== 1
      || parsed.type !== 'media.hello'
      || parsed.transport !== 'webcodecs_h264'
      || typeof parsed.generation !== 'number'
      || !Number.isSafeInteger(parsed.generation)
      || parsed.generation < 1
      || typeof parsed.codec !== 'string'
      || !/^avc1\.[0-9A-Fa-f]{6}$/.test(parsed.codec)
      || typeof parsed.description_base64 !== 'string'
      || parsed.description_base64.length === 0
      || typeof parsed.width !== 'number'
      || !Number.isSafeInteger(parsed.width)
      || parsed.width < 2
      || typeof parsed.height !== 'number'
      || !Number.isSafeInteger(parsed.height)
      || parsed.height < 2
      || typeof parsed.fps !== 'number'
      || !Number.isFinite(parsed.fps)
      || parsed.fps < 1
      || typeof parsed.bitrate_bps !== 'number'
      || !Number.isFinite(parsed.bitrate_bps)
      || parsed.bitrate_bps < 1
    ) return null;
    return parsed as unknown as WebCodecsMediaHello;
  } catch {
    return null;
  }
}

export function parseWebCodecsMediaUnavailable(value: unknown): WebCodecsMediaUnavailable | null {
  if (typeof value !== 'string') return null;
  try {
    const parsed = JSON.parse(value) as Record<string, unknown>;
    if (
      parsed.v !== 1
      || parsed.type !== 'media.unavailable'
      || typeof parsed.generation !== 'number'
      || !Number.isSafeInteger(parsed.generation)
      || parsed.generation < 1
      || typeof parsed.error !== 'string'
      || parsed.error.trim().length === 0
      || parsed.error.length > 4_096
    ) return null;
    return parsed as unknown as WebCodecsMediaUnavailable;
  } catch {
    return null;
  }
}

export function decodeBase64Bytes(value: string): Uint8Array {
  const decoded = atob(value);
  const bytes = new Uint8Array(decoded.length);
  for (let index = 0; index < decoded.length; index += 1) {
    bytes[index] = decoded.charCodeAt(index);
  }
  return bytes;
}

export function parseWebCodecsAccessUnit(
  value: ArrayBuffer | ArrayBufferView,
): WebCodecsAccessUnit | null {
  const bytes = asUint8Array(value);
  if (bytes.byteLength < WEBCODECS_AU_HEADER_BYTES) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (
    view.getUint32(0) !== WEBCODECS_AU_MAGIC
    || view.getUint8(4) !== WEBCODECS_AU_VERSION
    || view.getUint16(6) !== WEBCODECS_AU_HEADER_BYTES
  ) return null;

  const flags = view.getUint8(5);
  const knownFlags = WebCodecsAccessUnitFlag.Key
    | WebCodecsAccessUnitFlag.Delta
    | WebCodecsAccessUnitFlag.Discontinuity;
  if ((flags & ~knownFlags) !== 0) return null;
  const key = (flags & WebCodecsAccessUnitFlag.Key) !== 0;
  const delta = (flags & WebCodecsAccessUnitFlag.Delta) !== 0;
  if (key === delta) return null;

  const generation = view.getBigUint64(8);
  const sequence = view.getBigUint64(16);
  const durationUs = view.getUint32(32);
  const payloadLength = view.getUint32(36);
  if (
    generation === 0n
    || sequence === 0n
    || durationUs === 0
    || payloadLength === 0
    || payloadLength > WEBCODECS_MAX_ACCESS_UNIT_BYTES
    || payloadLength !== bytes.byteLength - WEBCODECS_AU_HEADER_BYTES
  ) return null;

  return {
    generation,
    sequence,
    timestampUs: view.getBigUint64(24),
    durationUs,
    key,
    delta,
    discontinuity: (flags & WebCodecsAccessUnitFlag.Discontinuity) !== 0,
    payload: bytes.subarray(WEBCODECS_AU_HEADER_BYTES),
  };
}

/** Test/server-side helper documenting the wire format in one executable place. */
export function encodeWebCodecsAccessUnit(unit: WebCodecsAccessUnit): Uint8Array {
  if (unit.key === unit.delta) throw new Error('exactly one of key/delta must be set');
  if (unit.payload.byteLength === 0 || unit.payload.byteLength > WEBCODECS_MAX_ACCESS_UNIT_BYTES) {
    throw new Error('invalid access-unit payload length');
  }
  if (unit.generation <= 0n || unit.sequence <= 0n) {
    throw new Error('generation and sequence must be positive');
  }
  if (!Number.isSafeInteger(unit.durationUs) || unit.durationUs <= 0 || unit.durationUs > 0xffff_ffff) {
    throw new Error('invalid access-unit duration');
  }
  const bytes = new Uint8Array(WEBCODECS_AU_HEADER_BYTES + unit.payload.byteLength);
  const view = new DataView(bytes.buffer);
  view.setUint32(0, WEBCODECS_AU_MAGIC);
  view.setUint8(4, WEBCODECS_AU_VERSION);
  let flags = unit.key ? WebCodecsAccessUnitFlag.Key : WebCodecsAccessUnitFlag.Delta;
  if (unit.discontinuity) flags |= WebCodecsAccessUnitFlag.Discontinuity;
  view.setUint8(5, flags);
  view.setUint16(6, WEBCODECS_AU_HEADER_BYTES);
  view.setBigUint64(8, unit.generation);
  view.setBigUint64(16, unit.sequence);
  view.setBigUint64(24, unit.timestampUs);
  view.setUint32(32, unit.durationUs);
  view.setUint32(36, unit.payload.byteLength);
  bytes.set(unit.payload, WEBCODECS_AU_HEADER_BYTES);
  return bytes;
}
