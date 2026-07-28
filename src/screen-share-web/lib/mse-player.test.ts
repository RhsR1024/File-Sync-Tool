import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  MseH264Player,
  appendQueueWouldOverflow,
  buildMediaWebSocketUrl,
  lowLatencyAction,
  parseMediaHello,
  parseMediaTrace,
  supportsMseH264,
} from './mse-player';

const MEDIA_HELLO = JSON.stringify({
  v: 1,
  type: 'media.hello',
  transport: 'mse_h264',
  generation: 3,
  codec: 'avc1.42C028',
  mime_type: 'video/mp4; codecs="avc1.42C028"',
  width: 1920,
  height: 1080,
  fps: 10,
  bitrate_bps: 5_000_000,
});

class FakeSocket extends EventTarget {
  readyState: number = WebSocket.OPEN;
  binaryType: BinaryType = 'blob';

  receive(data: unknown): void {
    this.dispatchEvent(new MessageEvent('message', { data }));
  }

  close(): void {
    this.readyState = WebSocket.CLOSED;
  }
}

class FakeSourceBuffer extends EventTarget {
  mode: AppendMode = 'segments';
  updating = false;
  appended: ArrayBuffer[] = [];
  range: [number, number] | null = null;

  get buffered(): TimeRanges {
    return {
      length: this.range === null ? 0 : 1,
      start: () => this.range?.[0] ?? 0,
      end: () => this.range?.[1] ?? 0,
    };
  }

  appendBuffer(value: ArrayBuffer): void {
    this.appended.push(value);
    this.updating = true;
  }

  finishAppend(range: [number, number] | null = this.range): void {
    this.range = range;
    this.updating = false;
    this.dispatchEvent(new Event('updateend'));
  }

  remove(): void {}

  abort(): void {
    this.updating = false;
  }
}

class FakeMediaSource extends EventTarget {
  readonly sourceBuffer = new FakeSourceBuffer();

  addSourceBuffer(): SourceBuffer {
    return this.sourceBuffer as unknown as SourceBuffer;
  }

  open(): void {
    this.dispatchEvent(new Event('sourceopen'));
  }
}

function createVideo(): HTMLVideoElement {
  const video = document.createElement('video');
  vi.spyOn(video, 'play').mockResolvedValue();
  return video;
}

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
    expect(parseMediaHello(MEDIA_HELLO)).toMatchObject({ generation: 3, width: 1920, height: 1080 });
    expect(parseMediaHello(MEDIA_HELLO.replace('avc1.42C028', 'h264'))).toBeNull();
    expect(parseMediaHello('{}')).toBeNull();
  });

  it('parses a media trace sidecar without treating other text messages as media', () => {
    expect(parseMediaTrace(JSON.stringify({
      v: 1,
      type: 'media.trace',
      generation: 3,
      sequence: 9,
      capture_sequence: 81,
      captured_at_unix_ms: 1120,
      timestamp_us: 500_000,
      duration_us: 33_333,
      visible_input_sequence: 7,
      input_applied_at_server_unix_ms: 1115,
    }))).toEqual({
      generation: 3,
      sequence: 9,
      captureSequence: 81,
      capturedAtServerUnixMs: 1120,
      mediaTimeUs: 500_000,
      durationUs: 33_333,
      visibleInputSequence: 7,
      inputAppliedAtServerUnixMs: 1115,
    });
    expect(parseMediaTrace(MEDIA_HELLO)).toBeNull();
    expect(parseMediaTrace('{"v":1,"type":"media.trace"}')).toBeNull();
    expect(parseMediaTrace(JSON.stringify({
      v: 1,
      type: 'media.trace',
      generation: 3,
      sequence: 9,
      capture_sequence: 81,
      captured_at_unix_ms: 1120,
      timestamp_us: 500_000,
      duration_us: 33_333,
      visible_input_sequence: 7,
      input_applied_at_server_unix_ms: null,
    }))).toBeNull();
  });

  it('delegates codec capability checks to MediaSource', () => {
    const check = vi.fn().mockReturnValue(true);
    vi.stubGlobal('MediaSource', class {
      static isTypeSupported = check;
    });
    expect(supportsMseH264('video/mp4; codecs="avc1.42C028"')).toBe(true);
    expect(check).toHaveBeenCalledWith('video/mp4; codecs="avc1.42C028"');
  });

  it('seeks on initial sync, outside the buffer, and severe live-edge drift', () => {
    expect(lowLatencyAction(5.8, 5, 6, false)).toEqual({ seekTo: 5.88, playbackRate: 1 });
    expect(lowLatencyAction(4, 5, 6, true)).toEqual({ seekTo: 5.88, playbackRate: 1 });
    expect(lowLatencyAction(5, 5, 6, true)).toEqual({ seekTo: 5.88, playbackRate: 1 });
  });

  it('catches up smoothly during steady-state drift and restores normal speed at target', () => {
    const moderate = lowLatencyAction(5.7, 5, 6, true);
    expect(moderate.seekTo).toBeNull();
    expect(moderate.playbackRate).toBeGreaterThan(1);
    expect(moderate.playbackRate).toBeLessThan(1.05);

    expect(lowLatencyAction(5.5, 5, 6, true)).toEqual({ seekTo: null, playbackRate: 1.05 });
    expect(lowLatencyAction(5.2, 5, 6, true)).toEqual({ seekTo: null, playbackRate: 1.05 });
    expect(lowLatencyAction(5.88, 5, 6, true)).toEqual({ seekTo: null, playbackRate: 1 });
    expect(lowLatencyAction(Number.NaN, 5, 6, true)).toEqual({ seekTo: null, playbackRate: 1 });
  });

  it('keeps a two-second startup snapshot within duration and byte limits', () => {
    expect(appendQueueWouldOverflow({
      segmentCount: 121,
      bytes: 8 * 1024 * 1024,
      estimatedDurationMs: 2000,
    }, {
      maxSegments: 180,
      maxBytes: 32 * 1024 * 1024,
      maxEstimatedDurationMs: 3000,
    })).toBe(false);
    expect(appendQueueWouldOverflow({
      segmentCount: 181,
      bytes: 8 * 1024 * 1024,
      estimatedDurationMs: 2000,
    }, {
      maxSegments: 180,
      maxBytes: 32 * 1024 * 1024,
      maxEstimatedDurationMs: 3000,
    })).toBe(true);
    expect(appendQueueWouldOverflow({
      segmentCount: 120,
      bytes: 33 * 1024 * 1024,
      estimatedDurationMs: 2000,
    }, {
      maxSegments: 180,
      maxBytes: 32 * 1024 * 1024,
      maxEstimatedDurationMs: 3000,
    })).toBe(true);
    expect(appendQueueWouldOverflow({
      segmentCount: 120,
      bytes: 8 * 1024 * 1024,
      estimatedDurationMs: 3001,
    }, {
      maxSegments: 180,
      maxBytes: 32 * 1024 * 1024,
      maxEstimatedDurationMs: 3000,
    })).toBe(true);
  });

  it('measures media receive, append cost, queue load, live edge, and hard seeks', () => {
    let now = 0;
    const socket = new FakeSocket();
    const mediaSource = new FakeMediaSource();
    vi.stubGlobal('MediaSource', class {
      static isTypeSupported = () => true;
    });
    const player = new MseH264Player({
      now: () => now,
      metricsEmitIntervalMs: 0,
      webSocketFactory: () => socket as unknown as WebSocket,
      mediaSourceFactory: () => mediaSource as unknown as MediaSource,
      objectUrlFactory: () => 'blob:test',
      revokeObjectUrl: vi.fn(),
    });
    const metricsListener = vi.fn();
    player.onMetrics(metricsListener);
    const video = createVideo();
    player.start(video);

    socket.receive(MEDIA_HELLO);
    mediaSource.open();
    now = 2;
    socket.receive(new ArrayBuffer(4)); // init segment: bytes, but zero media duration
    now = 3;
    socket.receive(new ArrayBuffer(10));
    now = 4;
    socket.receive(new ArrayBuffer(12));

    let metrics = player.getMetrics();
    expect(metrics.mediaWsReceivedMessages).toBe(4);
    expect(metrics.mediaWsReceivedBinaryMessages).toBe(3);
    expect(metrics.mediaWsReceivedBytes).toBeGreaterThanOrEqual(26);
    expect(metrics.appendQueue).toMatchObject({
      segmentCount: 2,
      bytes: 22,
      estimatedDurationMs: 200,
      peakSegmentCount: 2,
    });

    now = 7;
    mediaSource.sourceBuffer.finishAppend();
    mediaSource.sourceBuffer.range = [5, 6];
    video.currentTime = 5.7;
    now = 11;
    mediaSource.sourceBuffer.finishAppend([5, 6]);

    metrics = player.getMetrics();
    expect(metrics.appendBufferDurationMs).toMatchObject({ sampleCount: 2, last: 4, p50: 4, p95: 5 });
    expect(metrics.liveEdgeDistanceMs.last).toBeCloseTo(300);
    expect(metrics).toMatchObject({
      hardSeekCount: 1,
      initialLiveEdgeSyncCount: 1,
      correctiveHardSeekCount: 0,
      presentedFrames: null,
      droppedFrames: null,
      presentationMetricsSupport: 'unsupported',
    });
    expect(metricsListener).toHaveBeenCalled();
    expect(metricsListener.mock.lastCall?.[0]).toMatchObject({ hardSeekCount: 1 });
  });

  it('reports real browser presentation and dropped-frame counters when available', () => {
    const socket = new FakeSocket();
    const mediaSource = new FakeMediaSource();
    const video = createVideo();
    Object.defineProperty(video, 'getVideoPlaybackQuality', {
      configurable: true,
      value: () => ({
        creationTime: 1,
        totalVideoFrames: 125,
        droppedVideoFrames: 5,
        corruptedVideoFrames: 0,
      }),
    });
    const player = new MseH264Player({
      webSocketFactory: () => socket as unknown as WebSocket,
      mediaSourceFactory: () => mediaSource as unknown as MediaSource,
      objectUrlFactory: () => 'blob:test',
      revokeObjectUrl: vi.fn(),
    });
    player.start(video);

    expect(player.getMetrics()).toMatchObject({
      presentedFrames: 120,
      droppedFrames: 5,
      presentationMetricsSupport: 'supported',
    });
  });

  it('uses clock sync and requestVideoFrameCallback for capture-to-display metrics', async () => {
    const socket = new FakeSocket();
    const mediaSource = new FakeMediaSource();
    const video = createVideo();
    let nowUnixMs = 1000;
    const frameCallbacks: Array<(
      now: number,
      metadata: { mediaTime: number; expectedDisplayTime?: number },
    ) => void> = [];
    Object.defineProperty(video, 'requestVideoFrameCallback', {
      configurable: true,
      value: vi.fn((callback) => {
        frameCallbacks.push(callback);
        return 1;
      }),
    });
    Object.defineProperty(video, 'cancelVideoFrameCallback', {
      configurable: true,
      value: vi.fn(),
    });
    vi.stubGlobal('MediaSource', class {
      static isTypeSupported = () => true;
    });
    const player = new MseH264Player({
      nowUnixMs: () => nowUnixMs,
      now: () => 100,
      clockSyncSampleCount: 1,
      fetcher: vi.fn().mockResolvedValue(new Response(
        JSON.stringify({ server_unix_ms: 1010 }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )),
      webSocketFactory: () => socket as unknown as WebSocket,
      mediaSourceFactory: () => mediaSource as unknown as MediaSource,
      objectUrlFactory: () => 'blob:test',
      revokeObjectUrl: vi.fn(),
    });
    player.start(video);
    await vi.waitFor(() => expect(player.getMetrics().endToEndLatency.clock.sampleCount).toBe(1));
    socket.receive(MEDIA_HELLO);
    socket.receive(JSON.stringify({
      v: 1,
      type: 'media.trace',
      generation: 3,
      sequence: 9,
      capture_sequence: 81,
      captured_at_unix_ms: 1120,
      timestamp_us: 500_000,
      duration_us: 33_333,
    }));

    nowUnixMs = 1200;
    expect(frameCallbacks).toHaveLength(1);
    frameCallbacks[0]?.(100, { mediaTime: 0.5, expectedDisplayTime: 105 });

    expect(player.getMetrics()).toMatchObject({
      presentationTraceSupport: 'supported',
      presentationTraceSource: 'expected-display-time',
      endToEndLatency: {
        captureToDisplayMs: { sampleCount: 1, last: 95 },
        pendingMediaTracePoints: 0,
      },
    });
    player.stop();
  });

  it('times reconnect recovery from the disconnect to the first frame of the new connection', () => {
    let now = 0;
    const socket = new FakeSocket();
    const mediaSource = new FakeMediaSource();
    const video = createVideo();
    const frameCallbacks: Array<(
      now: number,
      metadata: { mediaTime: number; expectedDisplayTime?: number },
    ) => void> = [];
    Object.defineProperty(video, 'requestVideoFrameCallback', {
      configurable: true,
      value: vi.fn((callback) => {
        frameCallbacks.push(callback);
        return frameCallbacks.length;
      }),
    });
    Object.defineProperty(video, 'cancelVideoFrameCallback', { configurable: true, value: vi.fn() });
    // 断线时刻已缓冲到 6 秒；这些帧继续呈现不算恢复。
    Object.defineProperty(video, 'buffered', {
      configurable: true,
      value: { length: 1, start: () => 0, end: () => 6 } as unknown as TimeRanges,
    });
    vi.stubGlobal('MediaSource', class {
      static isTypeSupported = () => true;
    });
    const player = new MseH264Player({
      now: () => now,
      nowUnixMs: () => 1_000 + now,
      metricsEmitIntervalMs: 0,
      clockSyncSampleCount: 0,
      webSocketFactory: () => socket as unknown as WebSocket,
      mediaSourceFactory: () => mediaSource as unknown as MediaSource,
      objectUrlFactory: () => 'blob:test',
      revokeObjectUrl: vi.fn(),
    });
    player.start(video);
    socket.receive(MEDIA_HELLO);
    mediaSource.open();

    expect(player.getMetrics()).toMatchObject({
      unexpectedDisconnectCount: 0,
      reconnectRecoveryMs: { sampleCount: 0 },
    });

    now = 1_000;
    socket.dispatchEvent(new Event('close'));
    expect(player.getMetrics().unexpectedDisconnectCount).toBe(1);

    now = 1_200;
    frameCallbacks.at(-1)?.(now, { mediaTime: 5.5 });
    expect(player.getMetrics().reconnectRecoveryMs.sampleCount).toBe(0);

    now = 1_450;
    frameCallbacks.at(-1)?.(now, { mediaTime: 6.2 });
    expect(player.getMetrics().reconnectRecoveryMs).toMatchObject({ sampleCount: 1, last: 450 });

    // 稳态呈现不会重复计入恢复样本。
    now = 1_600;
    frameCallbacks.at(-1)?.(now, { mediaTime: 6.5 });
    expect(player.getMetrics().reconnectRecoveryMs.sampleCount).toBe(1);
    player.stop();
  });

  it('does not count a manual stop as a disconnect recovery', () => {
    const socket = new FakeSocket();
    const mediaSource = new FakeMediaSource();
    const video = createVideo();
    vi.stubGlobal('MediaSource', class {
      static isTypeSupported = () => true;
    });
    const player = new MseH264Player({
      clockSyncSampleCount: 0,
      webSocketFactory: () => socket as unknown as WebSocket,
      mediaSourceFactory: () => mediaSource as unknown as MediaSource,
      objectUrlFactory: () => 'blob:test',
      revokeObjectUrl: vi.fn(),
    });
    player.start(video);
    socket.receive(MEDIA_HELLO);
    player.stop();
    socket.dispatchEvent(new Event('close'));

    expect(player.getMetrics()).toMatchObject({
      unexpectedDisconnectCount: 0,
      reconnectRecoveryMs: { sampleCount: 0 },
    });
  });

  it('periodically recalibrates the server clock and stops the timer on close', async () => {
    vi.useFakeTimers();
    let nowUnixMs = 1_000;
    const fetcher = vi.fn(async () => new Response(
      JSON.stringify({ server_unix_ms: nowUnixMs + 10 }),
    )) as typeof fetch;
    const player = new MseH264Player({
      nowUnixMs: () => nowUnixMs,
      clockSyncSampleCount: 1,
      clockSyncIntervalMs: 1_000,
      fetcher,
      webSocketFactory: () => new FakeSocket() as unknown as WebSocket,
      mediaSourceFactory: () => new FakeMediaSource() as unknown as MediaSource,
      objectUrlFactory: () => 'blob:test',
      revokeObjectUrl: vi.fn(),
    });
    player.start(createVideo());
    await vi.advanceTimersByTimeAsync(0);
    expect(fetcher).toHaveBeenCalledTimes(1);

    nowUnixMs = 2_000;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(player.getMetrics().endToEndLatency.clock.sampleCount).toBe(2);

    player.stop();
    await vi.advanceTimersByTimeAsync(2_000);
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it('enforces configurable duration and byte queue ceilings while retaining the legacy count ceiling', () => {
    const socket = new FakeSocket();
    const mediaSource = new FakeMediaSource();
    vi.stubGlobal('MediaSource', class {
      static isTypeSupported = () => true;
    });
    const player = new MseH264Player({
      maxQueuedSegments: 10,
      maxQueuedBytes: 1024,
      maxQueuedDurationMs: 150,
      webSocketFactory: () => socket as unknown as WebSocket,
      mediaSourceFactory: () => mediaSource as unknown as MediaSource,
      objectUrlFactory: () => 'blob:test',
      revokeObjectUrl: vi.fn(),
    });
    let status = '';
    player.onState((state) => { status = state.status; });
    player.start(createVideo());
    socket.receive(MEDIA_HELLO);
    socket.receive(new ArrayBuffer(4));
    socket.receive(new ArrayBuffer(4));
    socket.receive(new ArrayBuffer(4));

    expect(status).toBe('error');
    expect(player.getMetrics().appendQueue.overloadCount).toBe(1);
  });
});
