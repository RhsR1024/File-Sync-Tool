import { afterEach, describe, expect, it, vi } from 'vitest';

import { encodeWebCodecsAccessUnit } from './webcodecs-protocol';
import {
  WebCodecsH264Player,
  describeWebCodecsUnavailable,
  type WebCodecsPlayerState,
} from './webcodecs-player';

const HELLO = JSON.stringify({
  v: 1,
  type: 'media.hello',
  transport: 'webcodecs_h264',
  generation: 3,
  codec: 'avc1.42C028',
  description_base64: 'AWQAKP/hAA==',
  width: 1280,
  height: 720,
  fps: 60,
  bitrate_bps: 5_000_000,
});

function accessUnit(
  sequence: bigint,
  options: { key?: boolean; generation?: bigint; discontinuity?: boolean } = {},
): ArrayBuffer {
  const key = options.key ?? false;
  return encodeWebCodecsAccessUnit({
    generation: options.generation ?? 3n,
    sequence,
    timestampUs: sequence * 16_667n,
    durationUs: 16_667,
    key,
    delta: !key,
    discontinuity: options.discontinuity ?? false,
    payload: new Uint8Array([0, 0, 0, 2, key ? 0x65 : 0x41, Number(sequence)]),
  }).buffer;
}

class FakeSocket extends EventTarget {
  readyState: number = WebSocket.OPEN;
  binaryType: BinaryType = 'blob';
  onmessage: ((this: WebSocket, ev: MessageEvent) => unknown) | null = null;
  onerror: ((this: WebSocket, ev: Event) => unknown) | null = null;
  onclose: ((this: WebSocket, ev: CloseEvent) => unknown) | null = null;
  readonly sent: string[] = [];

  receive(data: unknown): void {
    this.onmessage?.call(this as unknown as WebSocket, new MessageEvent('message', { data }));
  }

  close(): void {
    this.readyState = WebSocket.CLOSED;
  }

  send(value: string): void {
    this.sent.push(value);
  }
}

class FakeFrame {
  closed = false;

  constructor(readonly timestamp: number) {}

  close(): void {
    this.closed = true;
  }
}

class FakeDecoder {
  decodeQueueSize = 0;
  state: 'unconfigured' | 'configured' | 'closed' = 'unconfigured';
  readonly decoded: Array<{ type: EncodedVideoChunkType; timestamp: number }> = [];
  resetCount = 0;

  constructor(private readonly init: VideoDecoderInit) {}

  configure(): void {
    this.state = 'configured';
  }

  decode(chunk: EncodedVideoChunk): void {
    this.decoded.push({ type: chunk.type, timestamp: chunk.timestamp });
    this.init.output(new FakeFrame(chunk.timestamp) as unknown as VideoFrame);
  }

  reset(): void {
    this.resetCount += 1;
    this.state = 'unconfigured';
  }

  close(): void {
    this.state = 'closed';
  }
}

class FakeAnimationFrames {
  private nextHandle = 1;
  private readonly callbacks = new Map<number, FrameRequestCallback>();

  request = (callback: FrameRequestCallback): number => {
    const handle = this.nextHandle++;
    this.callbacks.set(handle, callback);
    return handle;
  };

  cancel = (handle: number): void => {
    this.callbacks.delete(handle);
  };

  flush(): void {
    const callbacks = [...this.callbacks.values()];
    this.callbacks.clear();
    for (const callback of callbacks) callback(performance.now());
  }
}

function createHarness(maxDecodeQueueSize = 4, overrides: { nowUnixMs?: () => number } = {}) {
  const socket = new FakeSocket();
  const decoders: FakeDecoder[] = [];
  const rendered: FakeFrame[] = [];
  const states: WebCodecsPlayerState[] = [];
  const animationFrames = new FakeAnimationFrames();
  const canvas = document.createElement('canvas');
  vi.spyOn(canvas, 'getContext').mockReturnValue({
    drawImage: (frame: FakeFrame) => rendered.push(frame),
  } as unknown as CanvasRenderingContext2D);
  const player = new WebCodecsH264Player({
    availability: () => ({ available: true, reasons: [] }),
    webSocketUrlFactory: () => 'wss://viewer.test/media/webcodecs/ws',
    webSocketFactory: () => socket as unknown as WebSocket,
    decoderSupport: async () => true,
    decoderFactory: (init) => {
      const decoder = new FakeDecoder(init);
      decoders.push(decoder);
      return decoder;
    },
    chunkFactory: (init) => ({
      type: init.type,
      timestamp: init.timestamp,
      duration: init.duration,
      byteLength: (init.data as Uint8Array).byteLength,
      copyTo: () => undefined,
    }) as unknown as EncodedVideoChunk,
    fetcher: vi.fn(async () => new Response(JSON.stringify({ server_unix_ms: Date.now() }))) as typeof fetch,
    clockSyncSampleCount: 1,
    ...(overrides.nowUnixMs ? { nowUnixMs: overrides.nowUnixMs } : {}),
    maxDecodeQueueSize,
    requestAnimationFrame: animationFrames.request,
    cancelAnimationFrame: animationFrames.cancel,
    onStateChange: (state) => states.push(state),
  });
  player.start(canvas);
  return { player, socket, decoders, rendered, states, canvas, animationFrames };
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe('WebCodecs H.264 player', () => {
  it('reports secure-context and WSS requirements explicitly', () => {
    const description = describeWebCodecsUnavailable({
      available: false,
      reasons: ['secure-context-required', 'wss-required'],
    });
    expect(description).toContain('secure browser context');
    expect(description).toContain('HTTPS page and WSS');

    const canvas = document.createElement('canvas');
    vi.spyOn(canvas, 'getContext').mockReturnValue({} as CanvasRenderingContext2D);
    const player = new WebCodecsH264Player({
      availability: () => ({ available: false, reasons: ['secure-context-required'] }),
    });
    player.start(canvas);
    expect(player.getState()).toMatchObject({ status: 'unsupported' });
  });

  it('drops deltas before IDR, submits each full AU once, renders and closes frames', async () => {
    const { player, socket, decoders, rendered, canvas, animationFrames } = createHarness();
    socket.receive(HELLO);
    socket.receive(accessUnit(1n));
    socket.receive(accessUnit(2n, { key: true }));
    await settle();

    expect(canvas.width).toBe(1280);
    expect(canvas.height).toBe(720);
    await vi.waitFor(() => expect(decoders[0].decoded).toEqual([{ type: 'key', timestamp: 33_334 }]));
    animationFrames.flush();
    expect(decoders[0].resetCount).toBe(1);
    expect(rendered).toHaveLength(1);
    expect(rendered[0].closed).toBe(true);
    expect(player.getState().status).toBe('ready');
    expect(player.getMetrics()).toMatchObject({
      receivedAccessUnits: 2,
      submittedAccessUnits: 1,
      renderedFrames: 1,
      droppedBeforeKeyframe: 1,
      keyframeRequests: 1,
    });
    expect(socket.sent).toEqual([JSON.stringify({ v: 1, type: 'media.keyframe.request' })]);
  });

  it('resets on generation changes and rejects stale or mismatched sequences', async () => {
    const { player, socket, decoders } = createHarness();
    socket.receive(HELLO);
    socket.receive(accessUnit(5n, { key: true }));
    socket.receive(accessUnit(5n));
    socket.receive(accessUnit(6n, { generation: 4n }));
    await settle();

    await vi.waitFor(() => expect(decoders[0].decoded).toHaveLength(1));
    expect(player.getMetrics()).toMatchObject({
      droppedStaleSequence: 1,
      droppedGenerationMismatch: 1,
    });
  });

  it('drops an overloaded delta and waits for a fresh IDR', async () => {
    const { player, socket, decoders } = createHarness(2);
    socket.receive(HELLO);
    socket.receive(accessUnit(1n, { key: true }));
    await settle();
    decoders[0].decodeQueueSize = 2;
    socket.receive(accessUnit(2n));
    socket.receive(accessUnit(3n));
    socket.receive(accessUnit(4n, { key: true }));
    await settle();

    await vi.waitFor(() => expect(decoders[0].decoded.map((chunk) => chunk.type)).toEqual(['key', 'key']));
    expect(player.getMetrics()).toMatchObject({
      droppedOverloadDelta: 1,
      droppedBeforeKeyframe: 1,
      submittedAccessUnits: 2,
      keyframeRequests: 1,
    });
    expect(socket.sent).toEqual([JSON.stringify({ v: 1, type: 'media.keyframe.request' })]);
  });

  it('counts only the latest decoded frame before one animation paint', async () => {
    const { player, socket, rendered, animationFrames } = createHarness();
    socket.receive(HELLO);
    socket.receive(accessUnit(1n, { key: true }));
    socket.receive(accessUnit(2n));
    await vi.waitFor(() => expect(rendered).toHaveLength(2));

    expect(rendered.every((frame) => frame.closed)).toBe(true);
    expect(player.getMetrics()).toMatchObject({
      submittedAccessUnits: 2,
      renderedFrames: 0,
      droppedBeforePresentation: 1,
    });
    animationFrames.flush();
    expect(player.getMetrics()).toMatchObject({
      renderedFrames: 1,
      droppedBeforePresentation: 1,
    });
  });

  it('correlates media.trace with canvas presentation metrics', async () => {
    let now = 1_000;
    const socket = new FakeSocket();
    const animationFrames = new FakeAnimationFrames();
    const canvas = document.createElement('canvas');
    let drawCount = 0;
    vi.spyOn(canvas, 'getContext').mockReturnValue({
      drawImage: () => { drawCount += 1; },
    } as unknown as CanvasRenderingContext2D);
    const player = new WebCodecsH264Player({
      availability: () => ({ available: true, reasons: [] }),
      webSocketUrlFactory: () => 'wss://viewer.test/media/webcodecs/ws',
      webSocketFactory: () => socket as unknown as WebSocket,
      decoderSupport: async () => true,
      decoderFactory: (init) => new FakeDecoder(init),
      chunkFactory: (init) => ({ ...init }) as unknown as EncodedVideoChunk,
      nowUnixMs: () => now,
      fetcher: vi.fn(async () => new Response(JSON.stringify({ server_unix_ms: 1_000 }))) as typeof fetch,
      clockSyncSampleCount: 1,
      requestAnimationFrame: animationFrames.request,
      cancelAnimationFrame: animationFrames.cancel,
    });
    player.start(canvas);
    await vi.waitFor(() => expect(player.getMetrics().endToEndLatency.clock.sampleCount).toBe(1));
    socket.receive(HELLO);
    socket.receive(JSON.stringify({
      v: 1,
      type: 'media.trace',
      generation: 3,
      sequence: 1,
      capture_sequence: 10,
      captured_at_unix_ms: 1_000,
      timestamp_us: 16_667,
      duration_us: 16_667,
      visible_input_sequence: null,
      input_applied_at_server_unix_ms: null,
    }));
    now = 1_025;
    socket.receive(accessUnit(1n, { key: true }));
    await vi.waitFor(() => expect(drawCount).toBe(1));
    animationFrames.flush();

    await vi.waitFor(() => expect(player.getMetrics().endToEndLatency.captureToDisplayMs.last).toBe(25));
  });

  it('classifies socket failures as retryable without making codec errors retryable', async () => {
    const { player, socket } = createHarness();
    socket.onerror?.call(socket as unknown as WebSocket, new Event('error'));
    expect(player.getState()).toMatchObject({
      status: 'error',
      failureKind: 'retryable',
    });

    socket.receive(JSON.stringify({
      v: 1,
      type: 'media.unavailable',
      generation: 3,
      error: 'hardware encoder stopped',
    }));
    await settle();
    expect(player.getState()).toMatchObject({
      status: 'error',
      failureKind: 'fatal',
      lastError: 'H.264 media unavailable: hardware encoder stopped',
    });
  });

  it('times reconnect recovery from the disconnect to the next painted frame', async () => {
    vi.useFakeTimers();
    let nowUnixMs = 10_000;
    const { player, socket, decoders, animationFrames } = createHarness(4, { nowUnixMs: () => nowUnixMs });
    socket.receive(HELLO);
    socket.receive(accessUnit(1n, { key: true }));
    await settle();
    animationFrames.flush();
    expect(player.getMetrics()).toMatchObject({
      unexpectedDisconnectCount: 0,
      reconnectRecoveryMs: { sampleCount: 0 },
    });

    nowUnixMs = 11_000;
    socket.onclose?.call(socket as unknown as WebSocket, new CloseEvent('close'));
    expect(player.getMetrics().unexpectedDisconnectCount).toBe(1);
    // 解码/绘制之前还没有恢复，样本必须保持为空。
    expect(player.getMetrics().reconnectRecoveryMs.sampleCount).toBe(0);

    // 让重连定时器真正建立新连接，再由新连接送出第一帧。
    await vi.advanceTimersByTimeAsync(1_000);
    socket.receive(HELLO);
    socket.receive(accessUnit(2n, { key: true }));
    await settle();
    await settle();
    expect(decoders.at(-1)?.decoded).toHaveLength(1);
    nowUnixMs = 11_620;
    animationFrames.flush();

    expect(player.getMetrics().reconnectRecoveryMs).toMatchObject({ sampleCount: 1, last: 620 });

    socket.receive(accessUnit(3n));
    await settle();
    nowUnixMs = 11_700;
    animationFrames.flush();
    expect(player.getMetrics().reconnectRecoveryMs.sampleCount).toBe(1);
    player.stop();
  });

  it('periodically recalibrates the server clock and stops the timer on close', async () => {
    vi.useFakeTimers();
    let nowUnixMs = 1_000;
    const fetcher = vi.fn(async () => new Response(
      JSON.stringify({ server_unix_ms: nowUnixMs + 10 }),
    )) as typeof fetch;
    const socket = new FakeSocket();
    const canvas = document.createElement('canvas');
    vi.spyOn(canvas, 'getContext').mockReturnValue({
      drawImage: () => undefined,
    } as unknown as CanvasRenderingContext2D);
    const player = new WebCodecsH264Player({
      availability: () => ({ available: true, reasons: [] }),
      webSocketUrlFactory: () => 'wss://viewer.test/media/webcodecs/ws',
      webSocketFactory: () => socket as unknown as WebSocket,
      decoderSupport: async () => true,
      fetcher,
      nowUnixMs: () => nowUnixMs,
      clockSyncSampleCount: 1,
      clockSyncIntervalMs: 1_000,
    });
    player.start(canvas);
    await settle();
    expect(fetcher).toHaveBeenCalledTimes(1);

    nowUnixMs = 2_000;
    await vi.advanceTimersByTimeAsync(1_000);
    await settle();
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(player.getMetrics().endToEndLatency.clock.sampleCount).toBe(2);

    player.stop();
    await vi.advanceTimersByTimeAsync(2_000);
    expect(fetcher).toHaveBeenCalledTimes(2);
  });
});
