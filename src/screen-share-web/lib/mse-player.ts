import type { H264MediaHello } from '../types';
import {
  EndToEndLatencyTrace,
  monotonicUnixNow,
  sampleServerClock,
  type EndToEndLatencySnapshot,
  type MediaTracePoint,
} from './latency-trace';
import { RollingNumericMetric, type NumericMetricSnapshot } from './metrics';

export type MsePlayerStatus = 'idle' | 'connecting' | 'buffering' | 'ready' | 'reconnecting' | 'unsupported' | 'error' | 'closed';

export interface MsePlayerState {
  status: MsePlayerStatus;
  attempts: number;
  lastError: string | null;
  width: number;
  height: number;
}

export interface MseH264PlayerOptions {
  webSocketFactory?: (url: string) => WebSocket;
  mediaSourceFactory?: () => MediaSource;
  objectUrlFactory?: (source: MediaSource) => string;
  revokeObjectUrl?: (url: string) => void;
  reconnectBaseMs?: number;
  reconnectMaxMs?: number;
  readyTimeoutMs?: number;
  /** Legacy fragment-count ceiling retained for compatibility. */
  maxQueuedSegments?: number;
  maxQueuedBytes?: number;
  maxQueuedDurationMs?: number;
  metricsSampleCapacity?: number;
  metricsEmitIntervalMs?: number;
  now?: () => number;
  nowUnixMs?: () => number;
  fetcher?: typeof fetch;
  clockSyncSampleCount?: number;
  clockSyncIntervalMs?: number;
}

export type MsePlayerStateListener = (state: MsePlayerState) => void;
export type MsePlayerMetricsListener = (metrics: MsePlayerMetricsSnapshot) => void;

export interface MseAppendQueueSnapshot {
  segmentCount: number;
  bytes: number;
  estimatedDurationMs: number;
  peakSegmentCount: number;
  peakBytes: number;
  peakEstimatedDurationMs: number;
  overloadCount: number;
}

export interface MsePlayerMetricsSnapshot {
  capturedAtMs: number;
  mediaWsReceivedMessages: number;
  mediaWsReceivedTextMessages: number;
  mediaWsReceivedBinaryMessages: number;
  mediaWsReceivedBytes: number;
  mediaWsInterArrivalMs: NumericMetricSnapshot;
  appendQueue: MseAppendQueueSnapshot;
  appendBufferDurationMs: NumericMetricSnapshot;
  liveEdgeDistanceMs: NumericMetricSnapshot;
  /**
   * 断线到"新连接的画面重新呈现"的耗时。断线前已缓冲的帧继续呈现不计为恢复，
   * 因此只有 `mediaTime` 超过断线时刻缓冲末端的呈现帧才会记录样本。
   */
  reconnectRecoveryMs: NumericMetricSnapshot;
  unexpectedDisconnectCount: number;
  playbackRate: number;
  hardSeekCount: number;
  initialLiveEdgeSyncCount: number;
  correctiveHardSeekCount: number;
  presentedFrames: number | null;
  droppedFrames: number | null;
  presentationMetricsSupport: 'supported' | 'unsupported';
  presentationTraceSupport: 'supported' | 'unsupported';
  presentationTraceSource: 'expected-display-time' | 'callback-time' | 'unsupported';
  endToEndLatency: EndToEndLatencySnapshot;
}

export interface AppendQueueLimits {
  maxSegments: number;
  maxBytes: number;
  maxEstimatedDurationMs: number;
}

export interface AppendQueueLoad {
  segmentCount: number;
  bytes: number;
  estimatedDurationMs: number;
}

export function appendQueueWouldOverflow(load: AppendQueueLoad, limits: AppendQueueLimits): boolean {
  return load.segmentCount > limits.maxSegments
    || load.bytes > limits.maxBytes
    || load.estimatedDurationMs > limits.maxEstimatedDurationMs;
}

const LIVE_EDGE_TARGET_LATENCY_SECONDS = 0.12;
const LIVE_EDGE_RATE_TOLERANCE_SECONDS = 0.04;
const LIVE_EDGE_MAX_RATE_LATENCY_SECONDS = 0.5;
const LIVE_EDGE_SEEK_LATENCY_SECONDS = 1;
const MAX_LIVE_EDGE_PLAYBACK_RATE = 1.05;
const RETAINED_HISTORY_SECONDS = 2;
const RETAINED_CURRENT_TIME_MARGIN_SECONDS = 0.5;

export interface LiveEdgeAction {
  seekTo: number | null;
  playbackRate: number;
}

export function lowLatencyAction(
  currentTime: number,
  bufferedStart: number,
  bufferedEnd: number,
  hasSyncedLiveEdge: boolean,
): LiveEdgeAction {
  if (![currentTime, bufferedStart, bufferedEnd].every(Number.isFinite) || bufferedEnd <= bufferedStart) {
    return { seekTo: null, playbackRate: 1 };
  }
  const latency = bufferedEnd - currentTime;
  if (
    !hasSyncedLiveEdge
    || currentTime < bufferedStart
    || currentTime > bufferedEnd
    || latency >= LIVE_EDGE_SEEK_LATENCY_SECONDS
  ) {
    return {
      seekTo: Math.max(bufferedStart, bufferedEnd - LIVE_EDGE_TARGET_LATENCY_SECONDS),
      playbackRate: 1,
    };
  }
  if (latency <= LIVE_EDGE_TARGET_LATENCY_SECONDS + LIVE_EDGE_RATE_TOLERANCE_SECONDS) {
    return { seekTo: null, playbackRate: 1 };
  }
  const rateWindow = LIVE_EDGE_MAX_RATE_LATENCY_SECONDS
    - LIVE_EDGE_TARGET_LATENCY_SECONDS
    - LIVE_EDGE_RATE_TOLERANCE_SECONDS;
  const progress = Math.min(
    1,
    (latency - LIVE_EDGE_TARGET_LATENCY_SECONDS - LIVE_EDGE_RATE_TOLERANCE_SECONDS) / rateWindow,
  );
  return {
    seekTo: null,
    playbackRate: 1 + progress * (MAX_LIVE_EDGE_PLAYBACK_RATE - 1),
  };
}

export function buildMediaWebSocketUrl(reconnect = false): string {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const url = new URL(`${protocol}//${window.location.host}/media/ws`);
  if (reconnect) url.searchParams.set('reconnect', '1');
  return url.toString();
}

export function parseMediaHello(value: unknown): H264MediaHello | null {
  if (typeof value !== 'string') return null;
  try {
    const parsed = JSON.parse(value) as Record<string, unknown>;
    if (
      parsed.v !== 1
      || parsed.type !== 'media.hello'
      || parsed.transport !== 'mse_h264'
      || typeof parsed.generation !== 'number'
      || !Number.isFinite(parsed.generation)
      || typeof parsed.codec !== 'string'
      || !/^avc1\.[0-9A-Fa-f]{6}$/.test(parsed.codec)
      || typeof parsed.mime_type !== 'string'
      || typeof parsed.width !== 'number'
      || typeof parsed.height !== 'number'
      || typeof parsed.fps !== 'number'
      || typeof parsed.bitrate_bps !== 'number'
    ) return null;
    if (parsed.width < 2 || parsed.height < 2 || parsed.fps < 1 || parsed.bitrate_bps < 1) return null;
    return parsed as unknown as H264MediaHello;
  } catch {
    return null;
  }
}

export function supportsMseH264(mimeType: string): boolean {
  return typeof MediaSource !== 'undefined'
    && typeof MediaSource.isTypeSupported === 'function'
    && MediaSource.isTypeSupported(mimeType);
}

export class MseH264Player {
  private readonly options: Required<MseH264PlayerOptions>;
  private readonly listeners = new Set<MsePlayerStateListener>();
  private readonly metricsListeners = new Set<MsePlayerMetricsListener>();
  private video: HTMLVideoElement | null = null;
  private socket: WebSocket | null = null;
  private mediaSource: MediaSource | null = null;
  private sourceBuffer: SourceBuffer | null = null;
  private objectUrl: string | null = null;
  private reconnectTimer: number | null = null;
  private readyTimer: number | null = null;
  private manuallyClosed = true;
  private attempts = 0;
  private generation = 0;
  private queuedSegments: Array<{ bytes: ArrayBuffer; estimatedDurationMs: number }> = [];
  private queuedBytes = 0;
  private queuedEstimatedDurationMs = 0;
  private expectedInitializationSegment = false;
  private frameDurationMs = 0;
  private appendStartedAtMs: number | null = null;
  private hasSyncedLiveEdge = false;
  private mediaWsReceivedMessages = 0;
  private mediaWsReceivedTextMessages = 0;
  private mediaWsReceivedBinaryMessages = 0;
  private mediaWsReceivedBytes = 0;
  private lastMediaWsReceiveAtMs: number | null = null;
  private peakQueuedSegments = 0;
  private peakQueuedBytes = 0;
  private peakQueuedEstimatedDurationMs = 0;
  private appendQueueOverloadCount = 0;
  private interruptionStartedAtMs: number | null = null;
  private recoveryMediaTimeThreshold: number | null = null;
  private unexpectedDisconnectCount = 0;
  private playbackRate = 1;
  private hardSeekCount = 0;
  private initialLiveEdgeSyncCount = 0;
  private correctiveHardSeekCount = 0;
  private lastMetricsEmittedAtMs = Number.NEGATIVE_INFINITY;
  private presentationCallbackId: number | null = null;
  private presentationTraceSource: MsePlayerMetricsSnapshot['presentationTraceSource'] = 'unsupported';
  private clockSyncRun = 0;
  private clockSyncTimer: number | null = null;
  private readonly latencyTrace: EndToEndLatencyTrace;
  private readonly mediaWsInterArrivalMs: RollingNumericMetric;
  private readonly appendBufferDurationMs: RollingNumericMetric;
  private readonly liveEdgeDistanceMs: RollingNumericMetric;
  private readonly reconnectRecoveryMs: RollingNumericMetric;
  private state: MsePlayerState = {
    status: 'idle',
    attempts: 0,
    lastError: null,
    width: 0,
    height: 0,
  };

  constructor(options: MseH264PlayerOptions = {}) {
    this.options = {
      webSocketFactory: options.webSocketFactory ?? ((url) => new WebSocket(url)),
      mediaSourceFactory: options.mediaSourceFactory ?? (() => new MediaSource()),
      objectUrlFactory: options.objectUrlFactory ?? ((source) => URL.createObjectURL(source)),
      revokeObjectUrl: options.revokeObjectUrl ?? ((url) => URL.revokeObjectURL(url)),
      reconnectBaseMs: options.reconnectBaseMs ?? 700,
      reconnectMaxMs: options.reconnectMaxMs ?? 8000,
      readyTimeoutMs: options.readyTimeoutMs ?? 7000,
      // Keep the legacy safety ceiling until M0 can replace this frame-count
      // heuristic with measured queued duration and bytes. A 30 fps, two-second
      // startup GOP can already contain 60 fragments before SourceBuffer opens.
      maxQueuedSegments: options.maxQueuedSegments ?? 180,
      // A startup snapshot includes the init segment plus a two-second GOP. The
      // duration ceiling deliberately leaves one second for event-loop jitter.
      maxQueuedBytes: options.maxQueuedBytes ?? 32 * 1024 * 1024,
      maxQueuedDurationMs: options.maxQueuedDurationMs ?? 3000,
      metricsSampleCapacity: options.metricsSampleCapacity ?? 512,
      metricsEmitIntervalMs: options.metricsEmitIntervalMs ?? 1000,
      now: options.now ?? (() => performance.now()),
      nowUnixMs: options.nowUnixMs ?? monotonicUnixNow,
      fetcher: options.fetcher ?? ((input, init) => globalThis.fetch(input, init)),
      clockSyncSampleCount: options.clockSyncSampleCount ?? 4,
      clockSyncIntervalMs: options.clockSyncIntervalMs ?? 30_000,
    };
    this.latencyTrace = new EndToEndLatencyTrace(this.options.metricsSampleCapacity);
    this.mediaWsInterArrivalMs = new RollingNumericMetric(this.options.metricsSampleCapacity);
    this.appendBufferDurationMs = new RollingNumericMetric(this.options.metricsSampleCapacity);
    this.liveEdgeDistanceMs = new RollingNumericMetric(this.options.metricsSampleCapacity);
    this.reconnectRecoveryMs = new RollingNumericMetric(this.options.metricsSampleCapacity);
  }

  onState(listener: MsePlayerStateListener): () => void {
    this.listeners.add(listener);
    listener({ ...this.state });
    return () => this.listeners.delete(listener);
  }

  onMetrics(listener: MsePlayerMetricsListener): () => void {
    this.metricsListeners.add(listener);
    const snapshot = this.getMetrics();
    this.lastMetricsEmittedAtMs = snapshot.capturedAtMs;
    listener(snapshot);
    return () => this.metricsListeners.delete(listener);
  }

  getMetrics(): MsePlayerMetricsSnapshot {
    const playbackQuality = this.video?.getVideoPlaybackQuality?.();
    const droppedFrames = playbackQuality?.droppedVideoFrames ?? null;
    const presentedFrames = playbackQuality
      ? Math.max(0, playbackQuality.totalVideoFrames - playbackQuality.droppedVideoFrames)
      : null;
    return {
      capturedAtMs: this.options.now(),
      mediaWsReceivedMessages: this.mediaWsReceivedMessages,
      mediaWsReceivedTextMessages: this.mediaWsReceivedTextMessages,
      mediaWsReceivedBinaryMessages: this.mediaWsReceivedBinaryMessages,
      mediaWsReceivedBytes: this.mediaWsReceivedBytes,
      mediaWsInterArrivalMs: this.mediaWsInterArrivalMs.snapshot(),
      appendQueue: {
        segmentCount: this.queuedSegments.length,
        bytes: this.queuedBytes,
        estimatedDurationMs: this.queuedEstimatedDurationMs,
        peakSegmentCount: this.peakQueuedSegments,
        peakBytes: this.peakQueuedBytes,
        peakEstimatedDurationMs: this.peakQueuedEstimatedDurationMs,
        overloadCount: this.appendQueueOverloadCount,
      },
      appendBufferDurationMs: this.appendBufferDurationMs.snapshot(),
      liveEdgeDistanceMs: this.liveEdgeDistanceMs.snapshot(),
      reconnectRecoveryMs: this.reconnectRecoveryMs.snapshot(),
      unexpectedDisconnectCount: this.unexpectedDisconnectCount,
      playbackRate: this.playbackRate,
      hardSeekCount: this.hardSeekCount,
      initialLiveEdgeSyncCount: this.initialLiveEdgeSyncCount,
      correctiveHardSeekCount: this.correctiveHardSeekCount,
      // These are browser decoder/presentation counters, never inferred from
      // append operations. Chrome/Edge expose them through the standard API.
      presentedFrames,
      droppedFrames,
      presentationMetricsSupport: playbackQuality ? 'supported' : 'unsupported',
      presentationTraceSupport: typeof this.video?.requestVideoFrameCallback === 'function'
        ? 'supported'
        : 'unsupported',
      presentationTraceSource: typeof this.video?.requestVideoFrameCallback === 'function'
        ? this.presentationTraceSource
        : 'unsupported',
      endToEndLatency: this.latencyTrace.snapshot(),
    };
  }

  recordInputTrace(sequence: number, occurredAtClientUnixMs: number): void {
    this.latencyTrace.recordInput(sequence, occurredAtClientUnixMs);
    this.emitMetrics();
  }

  recordInputQueueAcknowledged(sequence: number, acknowledgedAtClientUnixMs: number): void {
    this.latencyTrace.recordInputQueueAcknowledged(sequence, acknowledgedAtClientUnixMs);
    this.emitMetrics();
  }

  start(video: HTMLVideoElement): void {
    this.stop(false);
    this.video = video;
    this.video.muted = true;
    this.video.playsInline = true;
    this.video.autoplay = true;
    this.video.addEventListener('loadeddata', this.handleVideoReady);
    this.video.addEventListener('playing', this.handleVideoReady);
    this.video.addEventListener('error', this.handleVideoError);
    this.startPresentationTrace(this.video);
    this.manuallyClosed = false;
    this.attempts = 0;
    const clockSyncRun = ++this.clockSyncRun;
    void this.synchronizeClock(clockSyncRun);
    this.connect();
  }

  reconnect(): void {
    if (!this.video) return;
    this.closeSocket();
    this.clearReconnectTimer();
    this.manuallyClosed = false;
    this.connect();
  }

  stop(markClosed = true): void {
    this.manuallyClosed = true;
    this.clearReconnectTimer();
    this.clearReadyTimer();
    this.clockSyncRun += 1;
    this.clearClockSyncTimer();
    this.stopPresentationTrace();
    this.closeSocket();
    this.cleanupMediaSource();
    if (this.video) {
      this.video.removeEventListener('loadeddata', this.handleVideoReady);
      this.video.removeEventListener('playing', this.handleVideoReady);
      this.video.removeEventListener('error', this.handleVideoError);
    }
    this.video = null;
    this.generation = 0;
    this.resetQueue();
    this.expectedInitializationSegment = false;
    this.frameDurationMs = 0;
    this.hasSyncedLiveEdge = false;
    // 主动停止不是断线，未完成的恢复计时直接作废而不是留到下次会话。
    this.clearInterruption();
    if (markClosed) this.setState({ status: 'closed', lastError: null });
    this.emitMetrics();
  }

  private connect(): void {
    if (this.manuallyClosed || !this.video) return;
    const socket = this.options.webSocketFactory(buildMediaWebSocketUrl(this.attempts > 0));
    this.socket = socket;
    socket.binaryType = 'arraybuffer';
    this.setState({
      status: this.attempts > 0 ? 'reconnecting' : 'connecting',
      attempts: this.attempts,
      lastError: null,
    });
    socket.addEventListener('message', (event) => {
      if (this.socket !== socket) return;
      this.handleMessage(event.data);
    });
    socket.addEventListener('close', () => {
      if (this.socket !== socket) return;
      this.socket = null;
      this.clearReadyTimer();
      if (!this.manuallyClosed) this.scheduleReconnect('H.264 media connection closed');
    });
    socket.addEventListener('error', () => {
      if (this.socket !== socket || this.manuallyClosed) return;
      this.setState({ status: 'reconnecting', lastError: 'H.264 media connection error' });
    });
  }

  private handleMessage(data: unknown): void {
    this.recordMediaReceive(data);
    if (typeof data === 'string') {
      const hello = parseMediaHello(data);
      if (hello) {
        this.handleHello(hello);
        return;
      }
      const trace = parseMediaTrace(data);
      if (trace) {
        if (trace.generation === this.generation) {
          this.latencyTrace.addMediaTrace(trace);
          this.emitMetrics();
        }
        return;
      }
      try {
        const message = JSON.parse(data) as Record<string, unknown>;
        if (message.type === 'media.unavailable') {
          this.clearReadyTimer();
          this.setState({
            status: 'reconnecting',
            lastError: typeof message.error === 'string' ? message.error : 'H.264 media unavailable',
          });
        }
      } catch {
        this.fail('Invalid H.264 media message');
      }
      return;
    }
    if (data instanceof ArrayBuffer) {
      this.enqueueSegment(data);
      return;
    }
    if (data instanceof Blob) {
      const estimatedDurationMs = this.consumeSegmentEstimatedDuration();
      void data.arrayBuffer()
        .then((buffer) => this.enqueueSegment(buffer, estimatedDurationMs))
        .catch(() => this.fail('Invalid H.264 media segment'));
    }
  }

  private handleHello(hello: H264MediaHello): void {
    if (!supportsMseH264(hello.mime_type)) {
      this.fail(`Unsupported H.264 media type: ${hello.mime_type}`, true);
      return;
    }
    if (hello.generation === this.generation && this.mediaSource) return;
    this.generation = hello.generation;
    this.latencyTrace.resetGeneration(hello.generation);
    // 新 MediaSource 会重建时间线，断线时刻的 mediaTime 边界不再可比；
    // 此时任何呈现帧都必然来自新连接。
    this.recoveryMediaTimeThreshold = null;
    this.resetQueue();
    this.expectedInitializationSegment = true;
    this.frameDurationMs = 1000 / hello.fps;
    this.cleanupMediaSource();
    let mediaSource: MediaSource;
    try {
      mediaSource = this.options.mediaSourceFactory();
    } catch {
      this.fail('MediaSource initialization failed');
      return;
    }
    this.mediaSource = mediaSource;
    this.setState({
      status: 'buffering',
      lastError: null,
      width: hello.width,
      height: hello.height,
    });
    mediaSource.addEventListener('sourceopen', () => {
      if (this.mediaSource !== mediaSource) return;
      try {
        const sourceBuffer = mediaSource.addSourceBuffer(hello.mime_type);
        sourceBuffer.mode = 'segments';
        sourceBuffer.addEventListener('updateend', this.handleUpdateEnd);
        sourceBuffer.addEventListener('error', this.handleSourceBufferError);
        this.sourceBuffer = sourceBuffer;
        this.pumpQueue();
      } catch {
        this.fail('H.264 SourceBuffer initialization failed');
      }
    }, { once: true });
    const objectUrl = this.options.objectUrlFactory(mediaSource);
    this.objectUrl = objectUrl;
    if (this.video) this.video.src = objectUrl;
    this.clearReadyTimer();
    this.readyTimer = window.setTimeout(() => {
      this.readyTimer = null;
      if (this.state.status !== 'ready') this.fail('H.264 first frame timed out');
    }, this.options.readyTimeoutMs);
  }

  private enqueueSegment(segment: ArrayBuffer, estimatedDurationMs = this.consumeSegmentEstimatedDuration()): void {
    if (!this.mediaSource || this.generation === 0) return;
    const nextLoad = {
      segmentCount: this.queuedSegments.length + 1,
      bytes: this.queuedBytes + segment.byteLength,
      estimatedDurationMs: this.queuedEstimatedDurationMs + estimatedDurationMs,
    };
    if (appendQueueWouldOverflow(nextLoad, {
      maxSegments: this.options.maxQueuedSegments,
      maxBytes: this.options.maxQueuedBytes,
      maxEstimatedDurationMs: this.options.maxQueuedDurationMs,
    })) {
      this.appendQueueOverloadCount += 1;
      this.emitMetrics();
      this.fail('H.264 append queue fell behind');
      return;
    }
    this.queuedSegments.push({ bytes: segment, estimatedDurationMs });
    this.queuedBytes = nextLoad.bytes;
    this.queuedEstimatedDurationMs = nextLoad.estimatedDurationMs;
    this.peakQueuedSegments = Math.max(this.peakQueuedSegments, nextLoad.segmentCount);
    this.peakQueuedBytes = Math.max(this.peakQueuedBytes, nextLoad.bytes);
    this.peakQueuedEstimatedDurationMs = Math.max(
      this.peakQueuedEstimatedDurationMs,
      nextLoad.estimatedDurationMs,
    );
    this.emitMetrics();
    this.pumpQueue();
  }

  private pumpQueue(): void {
    const sourceBuffer = this.sourceBuffer;
    if (!sourceBuffer || sourceBuffer.updating) return;
    const next = this.queuedSegments.shift();
    if (next) {
      this.queuedBytes -= next.bytes.byteLength;
      this.queuedEstimatedDurationMs = Math.max(
        0,
        this.queuedEstimatedDurationMs - next.estimatedDurationMs,
      );
      try {
        this.appendStartedAtMs = this.options.now();
        sourceBuffer.appendBuffer(next.bytes);
        this.emitMetrics();
      } catch {
        this.appendStartedAtMs = null;
        this.fail('H.264 append failed');
      }
      return;
    }
    this.syncLiveEdge();
    this.trimBuffer();
  }

  private syncLiveEdge(): void {
    const video = this.video;
    const sourceBuffer = this.sourceBuffer;
    if (!video || !sourceBuffer || sourceBuffer.buffered.length === 0) return;
    const index = sourceBuffer.buffered.length - 1;
    const start = sourceBuffer.buffered.start(index);
    const end = sourceBuffer.buffered.end(index);
    this.liveEdgeDistanceMs.add(Math.max(0, (end - video.currentTime) * 1000));
    const action = lowLatencyAction(video.currentTime, start, end, this.hasSyncedLiveEdge);
    if (action.seekTo !== null) {
      this.hardSeekCount += 1;
      if (this.hasSyncedLiveEdge) this.correctiveHardSeekCount += 1;
      else this.initialLiveEdgeSyncCount += 1;
      video.currentTime = action.seekTo;
      this.hasSyncedLiveEdge = true;
    }
    if (video.playbackRate !== action.playbackRate) video.playbackRate = action.playbackRate;
    this.playbackRate = action.playbackRate;
    this.emitMetrics();
    void video.play().catch(() => undefined);
  }

  private trimBuffer(): void {
    const sourceBuffer = this.sourceBuffer;
    const video = this.video;
    if (!sourceBuffer || !video || sourceBuffer.updating || sourceBuffer.buffered.length === 0) return;
    const start = sourceBuffer.buffered.start(0);
    const end = sourceBuffer.buffered.end(sourceBuffer.buffered.length - 1);
    const removeUntil = Math.min(
      video.currentTime - RETAINED_CURRENT_TIME_MARGIN_SECONDS,
      end - RETAINED_HISTORY_SECONDS,
    );
    if (removeUntil > start + 0.25) {
      try {
        sourceBuffer.remove(start, removeUntil);
      } catch {
        this.fail('H.264 buffer trim failed');
      }
    }
  }

  private readonly handleUpdateEnd = () => {
    if (this.appendStartedAtMs !== null) {
      this.appendBufferDurationMs.add(Math.max(0, this.options.now() - this.appendStartedAtMs));
      this.appendStartedAtMs = null;
    }
    this.syncLiveEdge();
    this.pumpQueue();
  };

  private readonly handleSourceBufferError = () => {
    this.fail('H.264 SourceBuffer error');
  };

  private readonly handleVideoReady = () => {
    if (!this.video || this.video.videoWidth <= 0 || this.video.videoHeight <= 0) return;
    this.clearReadyTimer();
    this.attempts = 0;
    this.setState({
      status: 'ready',
      attempts: 0,
      lastError: null,
      width: this.video.videoWidth,
      height: this.video.videoHeight,
    });
  };

  private readonly handleVideoError = () => {
    this.fail('H.264 video decode error');
  };

  private startPresentationTrace(video: HTMLVideoElement): void {
    if (typeof video.requestVideoFrameCallback !== 'function') return;
    this.presentationCallbackId = video.requestVideoFrameCallback((now, metadata) => {
      this.presentationCallbackId = null;
      if (this.video !== video || this.manuallyClosed) return;
      const expectedDisplayTime = metadata.expectedDisplayTime;
      const expectedDisplayDelta = Number.isFinite(expectedDisplayTime)
        ? expectedDisplayTime - now
        : Number.NaN;
      const usesExpectedDisplayTime = Number.isFinite(expectedDisplayDelta)
        && Math.abs(expectedDisplayDelta) <= 1_000;
      this.presentationTraceSource = usesExpectedDisplayTime
        ? 'expected-display-time'
        : 'callback-time';
      const presentedAtUnixMs = this.options.nowUnixMs()
        + (usesExpectedDisplayTime ? expectedDisplayDelta : 0);
      this.latencyTrace.recordPresented(metadata.mediaTime, presentedAtUnixMs);
      this.recordRecoveryIfPending(metadata.mediaTime);
      this.emitMetrics();
      this.startPresentationTrace(video);
    });
  }

  private stopPresentationTrace(): void {
    const video = this.video;
    if (
      this.presentationCallbackId !== null
      && typeof video?.cancelVideoFrameCallback === 'function'
    ) {
      video.cancelVideoFrameCallback(this.presentationCallbackId);
    }
    this.presentationCallbackId = null;
    this.presentationTraceSource = 'unsupported';
  }

  private async synchronizeClock(run: number): Promise<void> {
    const sampleCount = Math.min(16, Math.max(0, Math.floor(this.options.clockSyncSampleCount)));
    for (let index = 0; index < sampleCount; index += 1) {
      try {
        const sample = await sampleServerClock(
          this.options.fetcher,
          this.options.nowUnixMs,
        );
        if (run !== this.clockSyncRun || this.manuallyClosed) return;
        this.latencyTrace.addClockSample(sample);
        this.emitMetrics();
      } catch {
        // Media playback remains usable if the diagnostic clock endpoint is
        // temporarily unreachable; the periodic calibration will retry.
        break;
      }
    }
    if (run === this.clockSyncRun && !this.manuallyClosed && sampleCount > 0) {
      this.clearClockSyncTimer();
      const interval = Math.max(1_000, this.options.clockSyncIntervalMs);
      this.clockSyncTimer = window.setTimeout(() => {
        this.clockSyncTimer = null;
        void this.synchronizeClock(run);
      }, interval);
    }
  }

  private clearClockSyncTimer(): void {
    if (this.clockSyncTimer !== null) window.clearTimeout(this.clockSyncTimer);
    this.clockSyncTimer = null;
  }

  /**
   * 断线时刻的缓冲末端。断线后这些帧还会继续呈现，超过它的呈现帧才来自新连接，
   * 因此它是"恢复"的判定边界而不是简单的下一帧。
   */
  private bufferedEndOrNull(): number | null {
    const video = this.video;
    if (!video) return null;
    try {
      const buffered = video.buffered;
      return buffered && buffered.length > 0 ? buffered.end(buffered.length - 1) : null;
    } catch {
      return null;
    }
  }

  private beginInterruption(): void {
    if (this.interruptionStartedAtMs !== null) return;
    this.interruptionStartedAtMs = this.options.now();
    this.recoveryMediaTimeThreshold = this.bufferedEndOrNull();
    this.unexpectedDisconnectCount += 1;
  }

  private recordRecoveryIfPending(mediaTime: number): void {
    const startedAtMs = this.interruptionStartedAtMs;
    if (startedAtMs === null) return;
    const threshold = this.recoveryMediaTimeThreshold;
    if (threshold !== null && Number.isFinite(mediaTime) && mediaTime < threshold) return;
    this.reconnectRecoveryMs.add(this.options.now() - startedAtMs);
    this.interruptionStartedAtMs = null;
    this.recoveryMediaTimeThreshold = null;
  }

  private clearInterruption(): void {
    this.interruptionStartedAtMs = null;
    this.recoveryMediaTimeThreshold = null;
  }

  private scheduleReconnect(error: string): void {
    if (this.manuallyClosed || this.reconnectTimer !== null) return;
    this.beginInterruption();
    this.attempts += 1;
    const delay = Math.min(
      this.options.reconnectMaxMs,
      this.options.reconnectBaseMs * 2 ** Math.min(this.attempts - 1, 5),
    );
    this.setState({ status: 'reconnecting', attempts: this.attempts, lastError: error });
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }

  private fail(error: string, unsupported = false): void {
    this.clearReadyTimer();
    this.closeSocket();
    this.cleanupMediaSource();
    this.manuallyClosed = true;
    // 终止性失败不会再恢复，挂起的计时不能等到下次连接才结算。
    this.clearInterruption();
    this.setState({
      status: unsupported ? 'unsupported' : 'error',
      lastError: error,
    });
    this.emitMetrics();
  }

  private closeSocket(): void {
    const socket = this.socket;
    this.socket = null;
    if (socket && socket.readyState < WebSocket.CLOSING) socket.close();
  }

  private cleanupMediaSource(): void {
    const sourceBuffer = this.sourceBuffer;
    this.sourceBuffer = null;
    if (sourceBuffer) {
      sourceBuffer.removeEventListener('updateend', this.handleUpdateEnd);
      sourceBuffer.removeEventListener('error', this.handleSourceBufferError);
      try {
        if (sourceBuffer.updating) sourceBuffer.abort();
      } catch (error) {
        void error;
      }
    }
    this.mediaSource = null;
    this.hasSyncedLiveEdge = false;
    this.appendStartedAtMs = null;
    this.playbackRate = 1;
    if (this.video && this.video.playbackRate !== 1) this.video.playbackRate = 1;
    if (this.objectUrl) {
      this.options.revokeObjectUrl(this.objectUrl);
      this.objectUrl = null;
    }
    this.resetQueue();
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer !== null) window.clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
  }

  private clearReadyTimer(): void {
    if (this.readyTimer !== null) window.clearTimeout(this.readyTimer);
    this.readyTimer = null;
  }

  private setState(patch: Partial<MsePlayerState>): void {
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener({ ...this.state });
  }

  private recordMediaReceive(data: unknown): void {
    const receivedAtMs = this.options.now();
    if (this.lastMediaWsReceiveAtMs !== null) {
      this.mediaWsInterArrivalMs.add(Math.max(0, receivedAtMs - this.lastMediaWsReceiveAtMs));
    }
    this.lastMediaWsReceiveAtMs = receivedAtMs;
    this.mediaWsReceivedMessages += 1;
    if (typeof data === 'string') {
      this.mediaWsReceivedTextMessages += 1;
      this.mediaWsReceivedBytes += new TextEncoder().encode(data).byteLength;
    } else if (data instanceof ArrayBuffer) {
      this.mediaWsReceivedBinaryMessages += 1;
      this.mediaWsReceivedBytes += data.byteLength;
    } else if (data instanceof Blob) {
      this.mediaWsReceivedBinaryMessages += 1;
      this.mediaWsReceivedBytes += data.size;
    }
    this.emitMetrics();
  }

  private consumeSegmentEstimatedDuration(): number {
    if (this.expectedInitializationSegment) {
      this.expectedInitializationSegment = false;
      return 0;
    }
    return this.frameDurationMs;
  }

  private resetQueue(): void {
    this.queuedSegments = [];
    this.queuedBytes = 0;
    this.queuedEstimatedDurationMs = 0;
  }

  private emitMetrics(): void {
    if (this.metricsListeners.size === 0) return;
    const now = this.options.now();
    if (now - this.lastMetricsEmittedAtMs < this.options.metricsEmitIntervalMs) return;
    const snapshot = this.getMetrics();
    this.lastMetricsEmittedAtMs = snapshot.capturedAtMs;
    for (const listener of this.metricsListeners) listener(snapshot);
  }
}

export function parseMediaTrace(value: unknown): MediaTracePoint | null {
  if (typeof value !== 'string') return null;
  try {
    const parsed = JSON.parse(value) as Record<string, unknown>;
    const visibleInputSequence = parsed.visible_input_sequence;
    const inputAppliedAtServerUnixMs = parsed.input_applied_at_server_unix_ms;
    const hasVisibleInput = visibleInputSequence !== null && visibleInputSequence !== undefined;
    const hasAppliedTime = inputAppliedAtServerUnixMs !== null
      && inputAppliedAtServerUnixMs !== undefined;
    if (
      parsed.v !== 1
      || parsed.type !== 'media.trace'
      || typeof parsed.generation !== 'number'
      || !Number.isSafeInteger(parsed.generation)
      || typeof parsed.sequence !== 'number'
      || !Number.isSafeInteger(parsed.sequence)
      || typeof parsed.capture_sequence !== 'number'
      || !Number.isSafeInteger(parsed.capture_sequence)
      || typeof parsed.captured_at_unix_ms !== 'number'
      || !Number.isFinite(parsed.captured_at_unix_ms)
      || typeof parsed.timestamp_us !== 'number'
      || !Number.isFinite(parsed.timestamp_us)
      || typeof parsed.duration_us !== 'number'
      || !Number.isFinite(parsed.duration_us)
      || parsed.duration_us <= 0
      || hasVisibleInput !== hasAppliedTime
      || (hasVisibleInput && (
        typeof visibleInputSequence !== 'number'
        || !Number.isSafeInteger(visibleInputSequence)
        || visibleInputSequence <= 0
      ))
      || (hasAppliedTime && (
        typeof inputAppliedAtServerUnixMs !== 'number'
        || !Number.isFinite(inputAppliedAtServerUnixMs)
      ))
    ) return null;
    return {
      generation: parsed.generation,
      sequence: parsed.sequence,
      captureSequence: parsed.capture_sequence,
      capturedAtServerUnixMs: parsed.captured_at_unix_ms,
      mediaTimeUs: parsed.timestamp_us,
      durationUs: parsed.duration_us,
      ...(hasVisibleInput ? {
        visibleInputSequence: visibleInputSequence as number,
        inputAppliedAtServerUnixMs: inputAppliedAtServerUnixMs as number,
      } : {}),
    };
  } catch {
    return null;
  }
}
