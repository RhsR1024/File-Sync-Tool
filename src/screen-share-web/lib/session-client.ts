import type {
  SessionConnectionState,
  SessionEnvelope,
  SessionServerMessage,
} from '../types';
import { RollingNumericMetric, type NumericMetricSnapshot } from './metrics';
import { monotonicUnixNow } from './latency-trace';

export type SessionMessageListener = (message: SessionServerMessage) => void;
export type SessionStateListener = (state: SessionConnectionState) => void;
export type SessionMetricsListener = (metrics: SessionClientMetricsSnapshot) => void;
export type SessionInputTraceListener = (event: SessionInputTraceEvent) => void;

export interface SessionInputTraceEvent {
  phase: 'sent' | 'acknowledged';
  clientSequence: number;
  inputType: string;
  occurredAtClientUnixMs: number;
  observedAtClientUnixMs: number;
}

export interface SessionClientMetricsSnapshot {
  capturedAtMs: number;
  bufferedAmountBytes: NumericMetricSnapshot;
  pointerEventToSendMs: NumericMetricSnapshot;
  pointerEventToServerAckMs: NumericMetricSnapshot;
  serverReceiveToEnqueueUs: NumericMetricSnapshot;
  pointerMoveDroppedCount: number;
  pendingInputAckCount: number;
  criticalInputAbortCount: number;
  pointerEventTimingSupport: 'caller_timestamp_required' | 'measured';
  serverInputAckMs: NumericMetricSnapshot;
  serverInputAckSupport: 'caller_timestamp_required' | 'pending' | 'measured';
}

export interface SessionClientOptions {
  url?: string;
  clientId?: string;
  reconnectBaseMs?: number;
  reconnectMaxMs?: number;
  heartbeatMs?: number;
  maxPointerMoveBufferedBytes?: number;
  maxCriticalInputBufferedBytes?: number;
  maxCriticalInputAgeMs?: number;
  metricsSampleCapacity?: number;
  metricsEmitIntervalMs?: number;
  now?: () => number;
  nowUnixMs?: () => number;
  webSocketFactory?: (url: string) => WebSocket;
}

const CLIENT_ID_STORAGE_KEY = 'screen-share-client-id';

interface PendingInputAck {
  type: string;
  eventOccurredAtMs: number | null;
  occurredAtClientUnixMs: number;
  critical: boolean;
  timeoutId: number | null;
}

function isInputMessage(type: string): boolean {
  return type.startsWith('input.');
}

function isCriticalInputMessage(type: string): boolean {
  return isInputMessage(type) && type !== 'input.pointer_move';
}

function createClientId(): string {
  const randomUuid = globalThis.crypto?.randomUUID?.();
  if (randomUuid) return randomUuid;
  return `viewer-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`;
}

export function getStableClientId(): string {
  try {
    const stored = window.sessionStorage.getItem(CLIENT_ID_STORAGE_KEY);
    if (stored && /^[A-Za-z0-9_-]{1,96}$/.test(stored)) return stored;
    const generated = createClientId();
    window.sessionStorage.setItem(CLIENT_ID_STORAGE_KEY, generated);
    return generated;
  } catch {
    return createClientId();
  }
}

function defaultUrl(clientId: string): string {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}/session/ws?client_id=${encodeURIComponent(clientId)}`;
}

function parseMessage(data: unknown): SessionServerMessage | null {
  if (typeof data !== 'string') return null;
  try {
    const value = JSON.parse(data) as Record<string, unknown>;
    if (value.v !== 1 || typeof value.type !== 'string') return null;
    const sessionId = typeof value.session_id === 'number' ? value.session_id : 0;
    const sourceEpoch = typeof value.source_epoch === 'number' ? value.source_epoch : 0;
    return {
      ...(value as unknown as SessionEnvelope),
      v: 1,
      type: value.type,
      session_id: sessionId,
      source_epoch: sourceEpoch,
    } as SessionServerMessage;
  } catch {
    return null;
  }
}

export class ScreenShareSessionClient {
  private readonly options: Required<Pick<
    SessionClientOptions,
    'reconnectBaseMs' | 'reconnectMaxMs' | 'heartbeatMs' | 'maxPointerMoveBufferedBytes'
    | 'maxCriticalInputBufferedBytes' | 'maxCriticalInputAgeMs'
    | 'metricsSampleCapacity' | 'metricsEmitIntervalMs' | 'now' | 'nowUnixMs'
  >> & SessionClientOptions;
  private socket: WebSocket | null = null;
  private reconnectTimer: number | null = null;
  private heartbeatTimer: number | null = null;
  private manuallyClosed = false;
  private sessionId = 0;
  private sourceEpoch = 0;
  private clientSeq = 0;
  private attempts = 0;
  private readonly messageListeners = new Set<SessionMessageListener>();
  private readonly stateListeners = new Set<SessionStateListener>();
  private readonly metricsListeners = new Set<SessionMetricsListener>();
  private readonly inputTraceListeners = new Set<SessionInputTraceListener>();
  private readonly clientId: string;
  private state: SessionConnectionState = { status: 'idle', attempts: 0, lastError: null };
  private readonly bufferedAmountBytes: RollingNumericMetric;
  private readonly pointerEventToSendMs: RollingNumericMetric;
  private readonly pointerEventToServerAckMs: RollingNumericMetric;
  private readonly serverReceiveToEnqueueUs: RollingNumericMetric;
  private pointerMoveDroppedCount = 0;
  private criticalInputAbortCount = 0;
  private abandoningControl = false;
  private readonly pendingInputAcks = new Map<number, PendingInputAck>();
  private lastMetricsEmittedAtMs = Number.NEGATIVE_INFINITY;

  constructor(options: SessionClientOptions = {}) {
    this.options = {
      reconnectBaseMs: 700,
      reconnectMaxMs: 8000,
      heartbeatMs: 15000,
      ...options,
      maxPointerMoveBufferedBytes: options.maxPointerMoveBufferedBytes ?? 64 * 1024,
      maxCriticalInputBufferedBytes: options.maxCriticalInputBufferedBytes ?? 256 * 1024,
      maxCriticalInputAgeMs: options.maxCriticalInputAgeMs ?? 500,
      metricsSampleCapacity: options.metricsSampleCapacity ?? 512,
      metricsEmitIntervalMs: options.metricsEmitIntervalMs ?? 1000,
      now: options.now ?? (() => performance.now()),
      nowUnixMs: options.nowUnixMs ?? monotonicUnixNow,
    };
    this.clientId = options.clientId ?? getStableClientId();
    this.bufferedAmountBytes = new RollingNumericMetric(this.options.metricsSampleCapacity);
    this.pointerEventToSendMs = new RollingNumericMetric(this.options.metricsSampleCapacity);
    this.pointerEventToServerAckMs = new RollingNumericMetric(this.options.metricsSampleCapacity);
    this.serverReceiveToEnqueueUs = new RollingNumericMetric(this.options.metricsSampleCapacity);
  }

  onMessage(listener: SessionMessageListener): () => void {
    this.messageListeners.add(listener);
    return () => this.messageListeners.delete(listener);
  }

  onState(listener: SessionStateListener): () => void {
    this.stateListeners.add(listener);
    listener(this.state);
    return () => this.stateListeners.delete(listener);
  }

  onMetrics(listener: SessionMetricsListener): () => void {
    this.metricsListeners.add(listener);
    const snapshot = this.getMetrics();
    this.lastMetricsEmittedAtMs = snapshot.capturedAtMs;
    listener(snapshot);
    return () => this.metricsListeners.delete(listener);
  }

  onInputTrace(listener: SessionInputTraceListener): () => void {
    this.inputTraceListeners.add(listener);
    return () => this.inputTraceListeners.delete(listener);
  }

  getMetrics(): SessionClientMetricsSnapshot {
    const pointerEventToSendMs = this.pointerEventToSendMs.snapshot();
    const pointerEventToServerAckMs = this.pointerEventToServerAckMs.snapshot();
    return {
      capturedAtMs: this.options.now(),
      bufferedAmountBytes: this.bufferedAmountBytes.snapshot(),
      pointerEventToSendMs,
      pointerEventToServerAckMs,
      serverReceiveToEnqueueUs: this.serverReceiveToEnqueueUs.snapshot(),
      pointerMoveDroppedCount: this.pointerMoveDroppedCount,
      pendingInputAckCount: this.pendingInputAcks.size,
      criticalInputAbortCount: this.criticalInputAbortCount,
      pointerEventTimingSupport: pointerEventToSendMs.sampleCount > 0
        ? 'measured'
        : 'caller_timestamp_required',
      serverInputAckMs: pointerEventToServerAckMs,
      serverInputAckSupport: pointerEventToServerAckMs.sampleCount > 0
        ? 'measured'
        : this.pendingInputAcks.size > 0 ? 'pending' : 'caller_timestamp_required',
    };
  }

  connect(sessionId = this.sessionId, sourceEpoch = this.sourceEpoch): void {
    this.sessionId = sessionId;
    this.sourceEpoch = sourceEpoch;
    this.manuallyClosed = false;
    this.clearReconnectTimer();
    if (this.socket && (this.socket.readyState === WebSocket.OPEN || this.socket.readyState === WebSocket.CONNECTING)) {
      return;
    }
    this.setState({ status: this.attempts ? 'reconnecting' : 'connecting', lastError: null });
    const factory = this.options.webSocketFactory ?? ((url: string) => new WebSocket(url));
    try {
      const socket = factory(this.options.url ?? defaultUrl(this.clientId));
      this.socket = socket;
      socket.addEventListener('open', this.handleOpen);
      socket.addEventListener('message', this.handleMessage);
      socket.addEventListener('error', this.handleError);
      socket.addEventListener('close', this.handleClose);
    } catch (error) {
      this.handleFailure(error instanceof Error ? error.message : 'WebSocket unavailable');
    }
  }

  close(): void {
    this.manuallyClosed = true;
    this.clearReconnectTimer();
    this.clearHeartbeat();
    this.clearPendingInputAcks();
    const socket = this.socket;
    this.socket = null;
    if (socket) socket.close();
    this.setState({ status: 'closed', attempts: this.attempts });
  }

  updateContext(sessionId: number, sourceEpoch: number): void {
    this.sessionId = sessionId;
    this.sourceEpoch = sourceEpoch;
  }

  /**
   * `eventOccurredAtMs`, when supplied for pointer input, must use the same
   * performance-timeline clock as `performance.now()` / `Event.timeStamp`.
   */
  send(type: string, payload?: unknown, eventOccurredAtMs?: number): boolean {
    const criticalInput = isCriticalInputMessage(type);
    const sendStartedAtMs = this.options.now();
    if (
      criticalInput
      && eventOccurredAtMs !== undefined
      && Number.isFinite(eventOccurredAtMs)
      && sendStartedAtMs - eventOccurredAtMs > this.options.maxCriticalInputAgeMs
    ) {
      this.abandonControl('Critical remote input was stale before send');
      return false;
    }
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
      if (criticalInput) this.abandonControl('Critical remote input could not be sent');
      return false;
    }
    const bufferedAmountBeforeSend = this.socket.bufferedAmount;
    if (
      type === 'input.pointer_move'
      && bufferedAmountBeforeSend > this.options.maxPointerMoveBufferedBytes
    ) {
      this.bufferedAmountBytes.add(bufferedAmountBeforeSend);
      this.pointerMoveDroppedCount += 1;
      this.emitMetrics();
      return false;
    }
    if (
      criticalInput
      && bufferedAmountBeforeSend > this.options.maxCriticalInputBufferedBytes
    ) {
      this.bufferedAmountBytes.add(bufferedAmountBeforeSend);
      this.abandonControl('Critical remote input exceeded the socket backlog limit');
      this.emitMetrics();
      return false;
    }
    const clientSeq = this.clientSeq + 1;
    const message: SessionEnvelope = {
      v: 1,
      type,
      session_id: this.sessionId,
      source_epoch: this.sourceEpoch,
      client_seq: clientSeq,
      ...(payload === undefined ? {} : { payload }),
    };
    try {
      this.socket.send(JSON.stringify(message));
      this.clientSeq = clientSeq;
      const sentAtMs = this.options.now();
      const sentAtUnixMs = this.options.nowUnixMs();
      this.bufferedAmountBytes.add(this.socket.bufferedAmount);
      if (
        type.startsWith('input.pointer_')
        && eventOccurredAtMs !== undefined
        && Number.isFinite(eventOccurredAtMs)
        && sentAtMs >= eventOccurredAtMs
      ) {
        this.pointerEventToSendMs.add(sentAtMs - eventOccurredAtMs);
      }
      if (isInputMessage(type)) {
        const compatibleEventTime = eventOccurredAtMs !== undefined
          && Number.isFinite(eventOccurredAtMs)
          && sentAtMs >= eventOccurredAtMs
          ? eventOccurredAtMs
          : null;
        const occurredAtClientUnixMs = compatibleEventTime === null
          ? sentAtUnixMs
          : sentAtUnixMs - (sentAtMs - compatibleEventTime);
        this.trackPendingInputAck(
          clientSeq,
          type,
          compatibleEventTime,
          occurredAtClientUnixMs,
          criticalInput,
        );
        this.emitInputTrace({
          phase: 'sent',
          clientSequence: clientSeq,
          inputType: type,
          occurredAtClientUnixMs,
          observedAtClientUnixMs: sentAtUnixMs,
        });
      }
      this.emitMetrics();
      return true;
    } catch {
      this.bufferedAmountBytes.add(this.socket.bufferedAmount);
      if (criticalInput) this.abandonControl('Critical remote input send failed');
      this.emitMetrics();
      return false;
    }
  }

  isConnected(): boolean {
    return this.state.status === 'connected';
  }

  private readonly handleOpen = (event: Event): void => {
    if (event.currentTarget !== this.socket) return;
    this.attempts = 0;
    this.setState({ status: 'connected', attempts: 0, lastError: null });
    this.startHeartbeat();
    this.send('session.heartbeat');
  };

  private readonly handleMessage = (event: MessageEvent): void => {
    if (event.currentTarget !== this.socket) return;
    const message = parseMessage(event.data);
    if (!message) {
      this.setState({ lastError: 'Invalid session message' });
      return;
    }
    if (message.type === 'session.hello') {
      this.sessionId = message.session_id || this.sessionId;
      this.sourceEpoch = message.source_epoch || this.sourceEpoch;
    }
    if (message.type === 'input.ack') {
      this.handleInputAck(message);
    } else if (message.type === 'session.error' && typeof message.client_seq === 'number') {
      const pending = this.takePendingInputAck(message.client_seq);
      if (pending?.critical) this.abandonControl('Critical remote input was rejected');
    }
    for (const listener of this.messageListeners) listener(message);
  };

  private readonly handleError = (event: Event): void => {
    if (event.currentTarget !== this.socket) return;
    this.handleFailure('Interaction connection error');
  };

  private readonly handleClose = (event: CloseEvent): void => {
    if (event.currentTarget !== this.socket) return;
    this.clearHeartbeat();
    this.clearPendingInputAcks();
    this.socket = null;
    if (this.manuallyClosed) {
      this.setState({ status: 'closed' });
      return;
    }
    this.scheduleReconnect();
  };

  private handleFailure(message: string): void {
    this.setState({ lastError: message });
    if (this.socket) {
      try { this.socket.close(); } catch { /* noop */ }
    } else {
      this.scheduleReconnect();
    }
  }

  private scheduleReconnect(): void {
    if (this.manuallyClosed || this.reconnectTimer !== null) return;
    this.attempts += 1;
    const delay = Math.min(this.options.reconnectMaxMs, this.options.reconnectBaseMs * 2 ** Math.min(this.attempts - 1, 5));
    this.setState({ status: 'reconnecting', attempts: this.attempts });
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }

  private startHeartbeat(): void {
    this.clearHeartbeat();
    this.heartbeatTimer = window.setInterval(() => this.send('session.heartbeat'), this.options.heartbeatMs);
  }

  private clearHeartbeat(): void {
    if (this.heartbeatTimer !== null) window.clearInterval(this.heartbeatTimer);
    this.heartbeatTimer = null;
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer !== null) window.clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
  }

  private trackPendingInputAck(
    clientSeq: number,
    type: string,
    eventOccurredAtMs: number | null,
    occurredAtClientUnixMs: number,
    critical: boolean,
  ): void {
    while (this.pendingInputAcks.size >= this.options.metricsSampleCapacity) {
      const oldestSeq = this.pendingInputAcks.keys().next().value as number | undefined;
      if (oldestSeq === undefined) break;
      const oldest = this.takePendingInputAck(oldestSeq);
      if (oldest?.critical) {
        this.abandonControl('Critical remote input acknowledgement backlog overflowed');
        return;
      }
    }
    const pending: PendingInputAck = {
      type,
      eventOccurredAtMs,
      occurredAtClientUnixMs,
      critical,
      timeoutId: null,
    };
    if (critical) {
      const now = this.options.now();
      const elapsedBeforeTracking = eventOccurredAtMs === null
        ? 0
        : Math.max(0, now - eventOccurredAtMs);
      const remainingAgeMs = Math.max(0, this.options.maxCriticalInputAgeMs - elapsedBeforeTracking);
      pending.timeoutId = window.setTimeout(() => {
        if (!this.pendingInputAcks.has(clientSeq)) return;
        this.takePendingInputAck(clientSeq);
        this.abandonControl('Critical remote input acknowledgement timed out');
      }, remainingAgeMs);
    }
    this.pendingInputAcks.set(clientSeq, pending);
  }

  private handleInputAck(message: SessionServerMessage): void {
    if (typeof message.client_seq !== 'number') return;
    const pending = this.takePendingInputAck(message.client_seq);
    if (!pending) return;
    const payload = message.payload && typeof message.payload === 'object'
      ? message.payload as Record<string, unknown>
      : {};
    if (typeof payload.receive_to_enqueue_us === 'number') {
      this.serverReceiveToEnqueueUs.add(payload.receive_to_enqueue_us);
    }
    if (
      pending.type.startsWith('input.pointer_')
      && pending.eventOccurredAtMs !== null
    ) {
      const ackReceivedAtMs = this.options.now();
      if (ackReceivedAtMs >= pending.eventOccurredAtMs) {
        this.pointerEventToServerAckMs.add(ackReceivedAtMs - pending.eventOccurredAtMs);
      }
    }
    this.emitInputTrace({
      phase: 'acknowledged',
      clientSequence: message.client_seq,
      inputType: pending.type,
      occurredAtClientUnixMs: pending.occurredAtClientUnixMs,
      observedAtClientUnixMs: this.options.nowUnixMs(),
    });
    this.emitMetrics();
  }

  private takePendingInputAck(clientSeq: number): PendingInputAck | undefined {
    const pending = this.pendingInputAcks.get(clientSeq);
    if (!pending) return undefined;
    this.pendingInputAcks.delete(clientSeq);
    if (pending.timeoutId !== null) window.clearTimeout(pending.timeoutId);
    return pending;
  }

  private clearPendingInputAcks(): void {
    for (const pending of this.pendingInputAcks.values()) {
      if (pending.timeoutId !== null) window.clearTimeout(pending.timeoutId);
    }
    this.pendingInputAcks.clear();
  }

  private abandonControl(reason: string): void {
    if (this.abandoningControl) return;
    this.abandoningControl = true;
    this.criticalInputAbortCount += 1;
    this.clearPendingInputAcks();
    const socket = this.socket;
    if (socket?.readyState === WebSocket.OPEN) {
      this.sendBestEffortUntracked(socket, 'input.release_all');
      this.sendBestEffortUntracked(socket, 'control.release');
      try { socket.close(1011, reason.slice(0, 120)); } catch { /* noop */ }
    }
    this.setState({ lastError: reason });
    this.abandoningControl = false;
  }

  private sendBestEffortUntracked(socket: WebSocket, type: string): void {
    try {
      socket.send(JSON.stringify({
        v: 1,
        type,
        session_id: this.sessionId,
        source_epoch: this.sourceEpoch,
        client_seq: ++this.clientSeq,
      } satisfies SessionEnvelope));
    } catch {
      // Closing the socket still revokes the server-side controller grant.
    }
  }

  private setState(next: Partial<SessionConnectionState>): void {
    this.state = { ...this.state, ...next };
    for (const listener of this.stateListeners) listener(this.state);
  }

  private emitInputTrace(event: SessionInputTraceEvent): void {
    for (const listener of this.inputTraceListeners) listener(event);
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
