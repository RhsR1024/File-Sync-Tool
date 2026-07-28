import {
  decodeBase64Bytes,
  parseWebCodecsAccessUnit,
  parseWebCodecsMediaHello,
  parseWebCodecsMediaUnavailable,
  type WebCodecsMediaHello,
} from './webcodecs-protocol';
import {
  EndToEndLatencyTrace,
  monotonicUnixNow,
  sampleServerClock,
  type EndToEndLatencySnapshot,
  type MediaTracePoint,
} from './latency-trace';
import { RollingNumericMetric, type NumericMetricSnapshot } from './metrics';

export type WebCodecsPlayerStatus =
  | 'idle'
  | 'connecting'
  | 'waiting-keyframe'
  | 'ready'
  | 'reconnecting'
  | 'unsupported'
  | 'error'
  | 'closed';

export type WebCodecsFailureKind = 'retryable' | 'fatal';

export interface WebCodecsPlayerState {
  status: WebCodecsPlayerStatus;
  failureKind: WebCodecsFailureKind | null;
  attempts: number;
  lastError: string | null;
  width: number;
  height: number;
}

export interface WebCodecsAvailability {
  available: boolean;
  reasons: Array<'secure-context-required' | 'wss-required' | 'video-decoder-unavailable' | 'websocket-unavailable'>;
}

export interface WebCodecsPlayerMetrics {
  receivedAccessUnits: number;
  receivedBytes: number;
  submittedAccessUnits: number;
  renderedFrames: number;
  droppedBeforePresentation: number;
  droppedBeforeKeyframe: number;
  droppedOverloadDelta: number;
  droppedStaleSequence: number;
  droppedGenerationMismatch: number;
  invalidMessages: number;
  discontinuities: number;
  decoderErrors: number;
  keyframeRequests: number;
  lastSequence: string | null;
  decodeQueueSize: number;
  /**
   * 断线到重连后第一帧重新呈现的耗时。该路径没有播放缓冲，重连后呈现的帧
   * 必然来自新连接，因此不需要 MSE 那样的 mediaTime 边界判定。
   */
  reconnectRecoveryMs: NumericMetricSnapshot;
  unexpectedDisconnectCount: number;
  /** Canvas has no presented-frame callback; rAF is a bounded pre-paint proxy. */
  presentationTraceSource: 'animation-frame-pre-paint-proxy';
  endToEndLatency: EndToEndLatencySnapshot;
}

interface DecoderLike {
  readonly decodeQueueSize: number;
  readonly state: 'unconfigured' | 'configured' | 'closed';
  configure(config: VideoDecoderConfig): void;
  decode(chunk: EncodedVideoChunk): void;
  reset(): void;
  close(): void;
}

export interface WebCodecsPlayerOptions {
  webSocketFactory?: (url: string) => WebSocket;
  webSocketUrlFactory?: (reconnect: boolean) => string;
  decoderFactory?: (init: VideoDecoderInit) => DecoderLike;
  chunkFactory?: (init: EncodedVideoChunkInit) => EncodedVideoChunk;
  decoderSupport?: (config: VideoDecoderConfig) => Promise<boolean>;
  availability?: () => WebCodecsAvailability;
  reconnectBaseMs?: number;
  reconnectMaxMs?: number;
  maxDecodeQueueSize?: number;
  metricsSampleCapacity?: number;
  nowUnixMs?: () => number;
  fetcher?: typeof fetch;
  clockSyncSampleCount?: number;
  clockSyncIntervalMs?: number;
  requestAnimationFrame?: (callback: FrameRequestCallback) => number;
  cancelAnimationFrame?: (handle: number) => void;
  onStateChange?: (state: WebCodecsPlayerState) => void;
}

const DEFAULT_RECONNECT_BASE_MS = 700;
const DEFAULT_RECONNECT_MAX_MS = 8_000;
const DEFAULT_MAX_DECODE_QUEUE_SIZE = 4;

export function inspectWebCodecsAvailability(): WebCodecsAvailability {
  const reasons: WebCodecsAvailability['reasons'] = [];
  if (globalThis.isSecureContext !== true) reasons.push('secure-context-required');
  if (typeof globalThis.location !== 'undefined' && globalThis.location.protocol !== 'https:') {
    reasons.push('wss-required');
  }
  if (typeof globalThis.VideoDecoder === 'undefined') reasons.push('video-decoder-unavailable');
  if (typeof globalThis.WebSocket === 'undefined') reasons.push('websocket-unavailable');
  return { available: reasons.length === 0, reasons };
}

export function describeWebCodecsUnavailable(availability: WebCodecsAvailability): string {
  const labels: Record<WebCodecsAvailability['reasons'][number], string> = {
    'secure-context-required': 'WebCodecs requires a secure browser context',
    'wss-required': 'WebCodecs media requires an HTTPS page and WSS transport',
    'video-decoder-unavailable': 'VideoDecoder is unavailable in this browser',
    'websocket-unavailable': 'WebSocket is unavailable in this browser',
  };
  return availability.reasons.map((reason) => labels[reason]).join('; ');
}

export function buildWebCodecsWssUrl(reconnect = false): string {
  if (window.location.protocol !== 'https:') {
    throw new Error('WebCodecs prototype requires HTTPS/WSS');
  }
  const url = new URL(`wss://${window.location.host}/media/webcodecs/ws`);
  if (reconnect) url.searchParams.set('reconnect', '1');
  return url.toString();
}

function decoderConfig(hello: WebCodecsMediaHello): VideoDecoderConfig {
  const description = decodeBase64Bytes(hello.description_base64);
  return {
    codec: hello.codec,
    codedWidth: hello.width,
    codedHeight: hello.height,
    description,
    optimizeForLatency: true,
    hardwareAcceleration: 'prefer-hardware',
  };
}

function emptyMetrics(
  endToEndLatency: EndToEndLatencySnapshot,
  reconnectRecoveryMs: NumericMetricSnapshot,
): WebCodecsPlayerMetrics {
  return {
    receivedAccessUnits: 0,
    receivedBytes: 0,
    submittedAccessUnits: 0,
    renderedFrames: 0,
    droppedBeforePresentation: 0,
    droppedBeforeKeyframe: 0,
    droppedOverloadDelta: 0,
    droppedStaleSequence: 0,
    droppedGenerationMismatch: 0,
    invalidMessages: 0,
    discontinuities: 0,
    decoderErrors: 0,
    keyframeRequests: 0,
    lastSequence: null,
    decodeQueueSize: 0,
    reconnectRecoveryMs,
    unexpectedDisconnectCount: 0,
    presentationTraceSource: 'animation-frame-pre-paint-proxy',
    endToEndLatency,
  };
}

export class WebCodecsH264Player {
  private readonly options: Required<Omit<WebCodecsPlayerOptions, 'onStateChange'>> & Pick<WebCodecsPlayerOptions, 'onStateChange'>;
  private readonly listeners = new Set<(state: WebCodecsPlayerState) => void>();
  private readonly latencyTrace: EndToEndLatencyTrace;
  private canvas: HTMLCanvasElement | null = null;
  private context: CanvasRenderingContext2D | null = null;
  private socket: WebSocket | null = null;
  private decoder: DecoderLike | null = null;
  private activeConfig: VideoDecoderConfig | null = null;
  private reconnectTimer: number | null = null;
  private clockSyncTimer: number | null = null;
  private clockSyncRun = 0;
  private manuallyClosed = true;
  private connectionEpoch = 0;
  private attempts = 0;
  private generation: bigint | null = null;
  private lastSequence: bigint | null = null;
  private awaitingKeyframe = true;
  private keyframeRequestOutstanding = false;
  private presentationFrame: number | null = null;
  private pendingPresentationMediaTimeSeconds: number | null = null;
  private messageChain = Promise.resolve();
  private interruptionStartedAtMs: number | null = null;
  private reconnectRecoveryMs: RollingNumericMetric;
  private metrics: WebCodecsPlayerMetrics;
  private state: WebCodecsPlayerState = {
    status: 'idle', failureKind: null, attempts: 0, lastError: null, width: 0, height: 0,
  };

  constructor(options: WebCodecsPlayerOptions = {}) {
    this.options = {
      webSocketFactory: options.webSocketFactory ?? ((url) => new WebSocket(url)),
      webSocketUrlFactory: options.webSocketUrlFactory ?? buildWebCodecsWssUrl,
      decoderFactory: options.decoderFactory ?? ((init) => new VideoDecoder(init)),
      chunkFactory: options.chunkFactory ?? ((init) => new EncodedVideoChunk(init)),
      decoderSupport: options.decoderSupport
        ?? (async (config) => (await VideoDecoder.isConfigSupported(config)).supported === true),
      availability: options.availability ?? inspectWebCodecsAvailability,
      reconnectBaseMs: options.reconnectBaseMs ?? DEFAULT_RECONNECT_BASE_MS,
      reconnectMaxMs: options.reconnectMaxMs ?? DEFAULT_RECONNECT_MAX_MS,
      maxDecodeQueueSize: options.maxDecodeQueueSize ?? DEFAULT_MAX_DECODE_QUEUE_SIZE,
      metricsSampleCapacity: options.metricsSampleCapacity ?? 512,
      nowUnixMs: options.nowUnixMs ?? monotonicUnixNow,
      fetcher: options.fetcher ?? ((input, init) => globalThis.fetch(input, init)),
      clockSyncSampleCount: options.clockSyncSampleCount ?? 4,
      clockSyncIntervalMs: options.clockSyncIntervalMs ?? 30_000,
      requestAnimationFrame: options.requestAnimationFrame
        ?? ((callback) => window.requestAnimationFrame(callback)),
      cancelAnimationFrame: options.cancelAnimationFrame
        ?? ((handle) => window.cancelAnimationFrame(handle)),
      onStateChange: options.onStateChange,
    };
    this.latencyTrace = new EndToEndLatencyTrace(this.options.metricsSampleCapacity);
    this.reconnectRecoveryMs = new RollingNumericMetric(this.options.metricsSampleCapacity);
    this.metrics = emptyMetrics(this.latencyTrace.snapshot(), this.reconnectRecoveryMs.snapshot());
  }

  onState(listener: (state: WebCodecsPlayerState) => void): () => void {
    this.listeners.add(listener);
    listener(this.getState());
    return () => this.listeners.delete(listener);
  }

  getState(): WebCodecsPlayerState {
    return { ...this.state };
  }

  getMetrics(): WebCodecsPlayerMetrics {
    return {
      ...this.metrics,
      decodeQueueSize: this.decoder?.decodeQueueSize ?? 0,
      reconnectRecoveryMs: this.reconnectRecoveryMs.snapshot(),
      endToEndLatency: this.latencyTrace.snapshot(),
    };
  }

  recordInputTrace(sequence: number, occurredAtClientUnixMs: number): void {
    this.latencyTrace.recordInput(sequence, occurredAtClientUnixMs);
  }

  recordInputQueueAcknowledged(sequence: number, acknowledgedAtClientUnixMs: number): void {
    this.latencyTrace.recordInputQueueAcknowledged(sequence, acknowledgedAtClientUnixMs);
  }

  start(canvas: HTMLCanvasElement): void {
    this.stop(false);
    this.canvas = canvas;
    this.context = canvas.getContext('2d', { alpha: false });
    if (!this.context) {
      this.setState('error', '2D canvas rendering is unavailable', 'fatal');
      return;
    }
    const availability = this.options.availability();
    if (!availability.available) {
      this.setState('unsupported', describeWebCodecsUnavailable(availability), 'fatal');
      return;
    }
    this.manuallyClosed = false;
    // 新会话从零开始统计，上一次会话的恢复样本不能混入。
    this.interruptionStartedAtMs = null;
    this.reconnectRecoveryMs = new RollingNumericMetric(this.options.metricsSampleCapacity);
    this.metrics = emptyMetrics(this.latencyTrace.snapshot(), this.reconnectRecoveryMs.snapshot());
    const clockSyncRun = ++this.clockSyncRun;
    void this.synchronizeClock(clockSyncRun);
    this.connect(false);
  }

  stop(emitClosed = true): void {
    const wasStarted = !this.manuallyClosed || this.socket !== null || this.decoder !== null;
    this.manuallyClosed = true;
    this.connectionEpoch += 1;
    this.clockSyncRun += 1;
    this.clearClockSyncTimer();
    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    const socket = this.socket;
    this.socket = null;
    if (socket && socket.readyState < WebSocket.CLOSING) socket.close();
    this.closeDecoder();
    this.cancelPendingPresentation();
    this.canvas = null;
    this.context = null;
    this.generation = null;
    this.lastSequence = null;
    this.awaitingKeyframe = true;
    this.keyframeRequestOutstanding = false;
    // 主动停止不是断线；挂起的恢复计时作废而不是留到下次连接结算。
    this.interruptionStartedAtMs = null;
    if (emitClosed && wasStarted) this.setState('closed', null);
  }

  private connect(reconnect: boolean): void {
    if (this.manuallyClosed) return;
    const epoch = ++this.connectionEpoch;
    this.attempts += 1;
    this.setState(reconnect ? 'reconnecting' : 'connecting', null);
    let socket: WebSocket;
    try {
      socket = this.options.webSocketFactory(this.options.webSocketUrlFactory(reconnect));
    } catch (error) {
      this.setState(
        'error',
        error instanceof Error ? error.message : String(error),
        'retryable',
      );
      this.scheduleReconnect();
      return;
    }
    socket.binaryType = 'arraybuffer';
    this.socket = socket;
    this.messageChain = Promise.resolve();
    socket.onmessage = (event) => {
      this.messageChain = this.messageChain
        .then(() => this.handleMessage(event.data, epoch))
        .catch((error: unknown) => this.failConnection(error, epoch));
    };
    socket.onerror = () => {
      if (epoch === this.connectionEpoch) {
        this.setState('error', 'WebCodecs media WebSocket failed', 'retryable');
      }
    };
    socket.onclose = () => {
      if (epoch !== this.connectionEpoch) return;
      this.socket = null;
      this.closeDecoder();
      this.cancelPendingPresentation();
      this.generation = null;
      this.lastSequence = null;
      this.awaitingKeyframe = true;
      this.keyframeRequestOutstanding = false;
      if (this.manuallyClosed) return;
      this.scheduleReconnect();
    };
  }

  private async handleMessage(data: unknown, epoch: number): Promise<void> {
    if (epoch !== this.connectionEpoch || this.manuallyClosed) return;
    if (typeof data === 'string') {
      const hello = parseWebCodecsMediaHello(data);
      if (hello) {
        await this.configureDecoder(hello, epoch);
        return;
      }
      const trace = parseMediaTrace(data);
      if (trace) {
        this.latencyTrace.addMediaTrace(trace);
        return;
      }
      const unavailable = parseWebCodecsMediaUnavailable(data);
      if (unavailable) {
        throw new Error(`H.264 media unavailable: ${unavailable.error}`);
      }
      this.metrics.invalidMessages += 1;
      throw new Error('Invalid WebCodecs media control message');
    }
    const bytes = data instanceof Blob ? await data.arrayBuffer() : data;
    if (!(bytes instanceof ArrayBuffer) && !ArrayBuffer.isView(bytes)) {
      this.metrics.invalidMessages += 1;
      return;
    }
    const unit = parseWebCodecsAccessUnit(bytes);
    if (!unit) {
      this.metrics.invalidMessages += 1;
      if (this.awaitingKeyframe) this.requestKeyframe();
      return;
    }
    this.metrics.receivedAccessUnits += 1;
    this.metrics.receivedBytes += unit.payload.byteLength;
    if (this.generation === null || unit.generation !== this.generation || !this.decoder) {
      this.metrics.droppedGenerationMismatch += 1;
      return;
    }
    if (this.lastSequence !== null && unit.sequence <= this.lastSequence) {
      this.metrics.droppedStaleSequence += 1;
      return;
    }
    if (this.lastSequence !== null && unit.sequence !== this.lastSequence + 1n) {
      this.awaitingKeyframe = true;
      this.metrics.discontinuities += 1;
      this.requestKeyframe();
    }
    this.lastSequence = unit.sequence;
    this.metrics.lastSequence = unit.sequence.toString();
    if (unit.discontinuity) {
      this.awaitingKeyframe = true;
      this.metrics.discontinuities += 1;
      this.requestKeyframe();
    }
    if (this.awaitingKeyframe && unit.delta) {
      this.metrics.droppedBeforeKeyframe += 1;
      this.requestKeyframe();
      this.setState('waiting-keyframe', null);
      return;
    }
    if (this.decoder.decodeQueueSize >= this.options.maxDecodeQueueSize && unit.delta) {
      // Dropping a predictive frame invalidates any following delta chain. Wait
      // for an IDR rather than presenting subtly corrupted output.
      this.metrics.droppedOverloadDelta += 1;
      this.awaitingKeyframe = true;
      this.requestKeyframe();
      this.setState('waiting-keyframe', 'Decoder queue overloaded; waiting for an IDR');
      return;
    }
    if (unit.key && this.awaitingKeyframe) {
      this.decoder.reset();
      if (!this.activeConfig) throw new Error('Missing active decoder configuration');
      this.decoder.configure(this.activeConfig);
      this.awaitingKeyframe = false;
      this.keyframeRequestOutstanding = false;
    }
    const timestamp = Number(unit.timestampUs);
    if (!Number.isSafeInteger(timestamp)) throw new Error('WebCodecs timestamp exceeds safe integer range');
    this.decoder.decode(this.options.chunkFactory({
      type: unit.key ? 'key' : 'delta',
      timestamp,
      duration: unit.durationUs,
      data: unit.payload,
    }));
    this.metrics.submittedAccessUnits += 1;
  }

  private async configureDecoder(hello: WebCodecsMediaHello, epoch: number): Promise<void> {
    const config = decoderConfig(hello);
    if (!await this.options.decoderSupport(config)) {
      throw new Error(`WebCodecs does not support ${hello.codec}`);
    }
    if (epoch !== this.connectionEpoch || this.manuallyClosed) return;
    this.closeDecoder();
    this.cancelPendingPresentation();
    this.activeConfig = config;
    this.generation = BigInt(hello.generation);
    this.latencyTrace.resetGeneration(hello.generation);
    this.lastSequence = null;
    this.awaitingKeyframe = true;
    this.keyframeRequestOutstanding = false;
    if (this.canvas) {
      this.canvas.width = hello.width;
      this.canvas.height = hello.height;
    }
    this.decoder = this.options.decoderFactory({
      output: (frame) => this.renderFrame(frame, epoch),
      error: (error) => {
        this.metrics.decoderErrors += 1;
        this.failConnection(error, epoch);
      },
    });
    this.decoder.configure(config);
    this.setDimensions(hello.width, hello.height);
    this.setState('waiting-keyframe', null);
  }

  private renderFrame(frame: VideoFrame, epoch: number): void {
    try {
      if (epoch !== this.connectionEpoch || this.manuallyClosed || !this.context || !this.canvas) return;
      this.context.drawImage(frame, 0, 0, this.canvas.width, this.canvas.height);
      if (this.pendingPresentationMediaTimeSeconds !== null) {
        this.metrics.droppedBeforePresentation += 1;
      }
      this.pendingPresentationMediaTimeSeconds = frame.timestamp / 1_000_000;
      if (this.presentationFrame === null) {
        this.presentationFrame = this.options.requestAnimationFrame(() => {
          this.presentationFrame = null;
          const mediaTimeSeconds = this.pendingPresentationMediaTimeSeconds;
          this.pendingPresentationMediaTimeSeconds = null;
          if (mediaTimeSeconds === null || this.manuallyClosed) return;
          this.metrics.renderedFrames += 1;
          this.latencyTrace.recordPresented(mediaTimeSeconds, this.options.nowUnixMs());
          this.recordRecoveryIfPending();
        });
      }
      this.setState('ready', null);
    } finally {
      // VideoFrame retains decoder/GPU resources until explicitly closed.
      frame.close();
    }
  }

  private closeDecoder(): void {
    if (this.decoder && this.decoder.state !== 'closed') this.decoder.close();
    this.decoder = null;
    this.activeConfig = null;
  }

  private cancelPendingPresentation(): void {
    if (this.presentationFrame !== null) {
      this.options.cancelAnimationFrame(this.presentationFrame);
      this.presentationFrame = null;
    }
    if (this.pendingPresentationMediaTimeSeconds !== null) {
      this.metrics.droppedBeforePresentation += 1;
      this.pendingPresentationMediaTimeSeconds = null;
    }
  }

  private requestKeyframe(): void {
    if (this.keyframeRequestOutstanding) return;
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify({ v: 1, type: 'media.keyframe.request' }));
    this.keyframeRequestOutstanding = true;
    this.metrics.keyframeRequests += 1;
  }

  private failConnection(
    error: unknown,
    epoch: number,
    failureKind: WebCodecsFailureKind = 'fatal',
  ): void {
    if (epoch !== this.connectionEpoch || this.manuallyClosed) return;
    // 终止性失败不会再恢复，挂起的计时不能等到下次连接才结算。
    if (failureKind === 'fatal') {
      this.manuallyClosed = true;
      this.interruptionStartedAtMs = null;
    }
    this.setState(
      'error',
      error instanceof Error ? error.message : String(error),
      failureKind,
    );
    const socket = this.socket;
    if (socket && socket.readyState < WebSocket.CLOSING) socket.close();
  }

  private beginInterruption(): void {
    if (this.interruptionStartedAtMs !== null) return;
    this.interruptionStartedAtMs = this.options.nowUnixMs();
    this.metrics.unexpectedDisconnectCount += 1;
  }

  private recordRecoveryIfPending(): void {
    const startedAtMs = this.interruptionStartedAtMs;
    if (startedAtMs === null) return;
    this.reconnectRecoveryMs.add(this.options.nowUnixMs() - startedAtMs);
    this.interruptionStartedAtMs = null;
  }

  private scheduleReconnect(): void {
    if (this.manuallyClosed || this.reconnectTimer !== null) return;
    this.beginInterruption();
    this.setState('reconnecting', this.state.lastError);
    const delay = Math.min(
      this.options.reconnectMaxMs,
      this.options.reconnectBaseMs * 2 ** Math.min(this.attempts - 1, 4),
    );
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      this.connect(true);
    }, delay);
  }

  private async synchronizeClock(run: number): Promise<void> {
    const sampleCount = Math.min(16, Math.max(0, Math.floor(this.options.clockSyncSampleCount)));
    for (let index = 0; index < sampleCount; index += 1) {
      if (this.manuallyClosed || run !== this.clockSyncRun) return;
      try {
        const sample = await sampleServerClock(this.options.fetcher, this.options.nowUnixMs);
        if (this.manuallyClosed || run !== this.clockSyncRun) return;
        this.latencyTrace.addClockSample(sample);
      } catch {
        // Clock diagnostics are best effort and are retried periodically.
        break;
      }
    }
    if (!this.manuallyClosed && run === this.clockSyncRun && sampleCount > 0) {
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

  private setDimensions(width: number, height: number): void {
    this.state = { ...this.state, width, height };
  }

  private setState(
    status: WebCodecsPlayerStatus,
    lastError: string | null,
    failureKind: WebCodecsFailureKind | null = null,
  ): void {
    this.state = {
      ...this.state,
      status,
      failureKind,
      attempts: this.attempts,
      lastError,
    };
    const snapshot = this.getState();
    this.options.onStateChange?.(snapshot);
    for (const listener of this.listeners) listener(snapshot);
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
