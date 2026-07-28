import type {
  SessionConnectionState,
  SessionEnvelope,
  SessionServerMessage,
} from '../types';

export type SessionMessageListener = (message: SessionServerMessage) => void;
export type SessionStateListener = (state: SessionConnectionState) => void;

export interface SessionClientOptions {
  url?: string;
  clientId?: string;
  reconnectBaseMs?: number;
  reconnectMaxMs?: number;
  heartbeatMs?: number;
  maxPointerMoveBufferedBytes?: number;
  webSocketFactory?: (url: string) => WebSocket;
}

const CLIENT_ID_STORAGE_KEY = 'screen-share-client-id';

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
  private readonly clientId: string;
  private state: SessionConnectionState = { status: 'idle', attempts: 0, lastError: null };

  constructor(options: SessionClientOptions = {}) {
    this.options = {
      reconnectBaseMs: 700,
      reconnectMaxMs: 8000,
      heartbeatMs: 15000,
      ...options,
      maxPointerMoveBufferedBytes: options.maxPointerMoveBufferedBytes ?? 64 * 1024,
    };
    this.clientId = options.clientId ?? getStableClientId();
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
    const socket = this.socket;
    this.socket = null;
    if (socket) socket.close();
    this.setState({ status: 'closed', attempts: this.attempts });
  }

  updateContext(sessionId: number, sourceEpoch: number): void {
    this.sessionId = sessionId;
    this.sourceEpoch = sourceEpoch;
  }

  send(type: string, payload?: unknown): boolean {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) return false;
    if (
      type === 'input.pointer_move'
      && this.socket.bufferedAmount > this.options.maxPointerMoveBufferedBytes
    ) return false;
    const message: SessionEnvelope = {
      v: 1,
      type,
      session_id: this.sessionId,
      source_epoch: this.sourceEpoch,
      client_seq: ++this.clientSeq,
      ...(payload === undefined ? {} : { payload }),
    };
    try {
      this.socket.send(JSON.stringify(message));
      return true;
    } catch {
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
    for (const listener of this.messageListeners) listener(message);
  };

  private readonly handleError = (event: Event): void => {
    if (event.currentTarget !== this.socket) return;
    this.handleFailure('Interaction connection error');
  };

  private readonly handleClose = (event: CloseEvent): void => {
    if (event.currentTarget !== this.socket) return;
    this.clearHeartbeat();
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

  private setState(next: Partial<SessionConnectionState>): void {
    this.state = { ...this.state, ...next };
    for (const listener of this.stateListeners) listener(this.state);
  }
}
