import type { H264MediaHello } from '../types';

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
  maxQueuedSegments?: number;
}

export type MsePlayerStateListener = (state: MsePlayerState) => void;

const LIVE_EDGE_TARGET_LATENCY_SECONDS = 0.12;
const LIVE_EDGE_RATE_TOLERANCE_SECONDS = 0.04;
const LIVE_EDGE_MAX_RATE_LATENCY_SECONDS = 0.5;
const LIVE_EDGE_SEEK_LATENCY_SECONDS = 1;
const MAX_LIVE_EDGE_PLAYBACK_RATE = 1.1;
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
  private queuedSegments: ArrayBuffer[] = [];
  private hasSyncedLiveEdge = false;
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
      maxQueuedSegments: options.maxQueuedSegments ?? 60,
    };
  }

  onState(listener: MsePlayerStateListener): () => void {
    this.listeners.add(listener);
    listener({ ...this.state });
    return () => this.listeners.delete(listener);
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
    this.manuallyClosed = false;
    this.attempts = 0;
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
    this.closeSocket();
    this.cleanupMediaSource();
    if (this.video) {
      this.video.removeEventListener('loadeddata', this.handleVideoReady);
      this.video.removeEventListener('playing', this.handleVideoReady);
      this.video.removeEventListener('error', this.handleVideoError);
    }
    this.video = null;
    this.generation = 0;
    this.queuedSegments = [];
    this.hasSyncedLiveEdge = false;
    if (markClosed) this.setState({ status: 'closed', lastError: null });
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
    if (typeof data === 'string') {
      const hello = parseMediaHello(data);
      if (hello) {
        this.handleHello(hello);
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
      void data.arrayBuffer().then((buffer) => this.enqueueSegment(buffer)).catch(() => this.fail('Invalid H.264 media segment'));
    }
  }

  private handleHello(hello: H264MediaHello): void {
    if (!supportsMseH264(hello.mime_type)) {
      this.fail(`Unsupported H.264 media type: ${hello.mime_type}`, true);
      return;
    }
    if (hello.generation === this.generation && this.mediaSource) return;
    this.generation = hello.generation;
    this.queuedSegments = [];
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

  private enqueueSegment(segment: ArrayBuffer): void {
    if (!this.mediaSource || this.generation === 0) return;
    if (this.queuedSegments.length >= this.options.maxQueuedSegments) {
      this.fail('H.264 append queue fell behind');
      return;
    }
    this.queuedSegments.push(segment);
    this.pumpQueue();
  }

  private pumpQueue(): void {
    const sourceBuffer = this.sourceBuffer;
    if (!sourceBuffer || sourceBuffer.updating) return;
    const next = this.queuedSegments.shift();
    if (next) {
      try {
        sourceBuffer.appendBuffer(next);
      } catch {
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
    const action = lowLatencyAction(video.currentTime, start, end, this.hasSyncedLiveEdge);
    if (action.seekTo !== null) {
      video.currentTime = action.seekTo;
      this.hasSyncedLiveEdge = true;
    }
    if (video.playbackRate !== action.playbackRate) video.playbackRate = action.playbackRate;
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

  private scheduleReconnect(error: string): void {
    if (this.manuallyClosed || this.reconnectTimer !== null) return;
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
    this.setState({
      status: unsupported ? 'unsupported' : 'error',
      lastError: error,
    });
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
    if (this.objectUrl) {
      this.options.revokeObjectUrl(this.objectUrl);
      this.objectUrl = null;
    }
    this.queuedSegments = [];
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
}
