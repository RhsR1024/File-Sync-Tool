import { afterEach, describe, expect, it, vi } from 'vitest';

import { ScreenShareSessionClient } from './session-client';

class FakeSocket extends EventTarget {
  readyState: number = WebSocket.CONNECTING;
  bufferedAmount = 0;
  readonly sent: string[] = [];
  closed = false;
  throwOnSend = false;

  open() {
    this.readyState = WebSocket.OPEN;
    this.dispatchEvent(new Event('open'));
  }

  send(value: string) {
    if (this.throwOnSend) throw new Error('send failed');
    this.sent.push(value);
  }

  close() {
    this.closed = true;
    this.readyState = WebSocket.CLOSED;
  }

  finishClose() {
    this.dispatchEvent(new CloseEvent('close'));
  }

  receive(value: unknown) {
    this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(value) }));
  }
}

afterEach(() => {
  vi.useRealTimers();
  window.sessionStorage.clear();
});

describe('screen share session client', () => {
  it('adds a stable browser identity to the interaction socket URL', () => {
    let socketUrl = '';
    const socket = new FakeSocket();
    const client = new ScreenShareSessionClient({
      clientId: 'viewer-stable-1',
      webSocketFactory: (url) => {
        socketUrl = url;
        return socket as unknown as WebSocket;
      },
    });

    client.connect(12, 4);

    expect(new URL(socketUrl).searchParams.get('client_id')).toBe('viewer-stable-1');
  });

  it('ignores a stale close event after an explicit reconnect', () => {
    vi.useFakeTimers();
    const sockets: FakeSocket[] = [];
    const client = new ScreenShareSessionClient({
      webSocketFactory: () => {
        const socket = new FakeSocket();
        sockets.push(socket);
        return socket as unknown as WebSocket;
      },
    });

    client.connect(12, 4);
    sockets[0].open();
    client.close();
    client.connect(12, 4);
    sockets[0].finishClose();
    sockets[1].open();
    vi.advanceTimersByTime(10_000);

    expect(sockets).toHaveLength(2);
    expect(client.isConnected()).toBe(true);
  });

  it('updates session context from hello before sending commands', () => {
    const socket = new FakeSocket();
    const client = new ScreenShareSessionClient({
      webSocketFactory: () => socket as unknown as WebSocket,
    });
    client.connect();
    socket.open();
    socket.receive({
      v: 1,
      type: 'session.hello',
      session_id: 88,
      source_epoch: 9,
      payload: { client_id: 'viewer-a' },
    });

    expect(client.send('annotation.undo')).toBe(true);
    expect(JSON.parse(socket.sent.at(-1) ?? '{}')).toMatchObject({
      type: 'annotation.undo',
      session_id: 88,
      source_epoch: 9,
    });
  });

  it('sequences control and pointer messages in the approved session context', () => {
    const socket = new FakeSocket();
    const client = new ScreenShareSessionClient({
      webSocketFactory: () => socket as unknown as WebSocket,
    });
    client.connect(55, 8);
    socket.open();

    expect(client.send('control.request')).toBe(true);
    expect(client.send('input.pointer_move', { x: 0.25, y: 0.75 })).toBe(true);
    expect(client.send('input.pointer_button', { button: 'left', pressed: true })).toBe(true);

    const messages = socket.sent.slice(-3).map((value) => JSON.parse(value));
    expect(messages.map((message) => message.client_seq)).toEqual([2, 3, 4]);
    expect(messages[1]).toMatchObject({
      type: 'input.pointer_move',
      session_id: 55,
      source_epoch: 8,
      payload: { x: 0.25, y: 0.75 },
    });
  });

  it('drops only pointer moves when the interaction socket is congested', () => {
    const socket = new FakeSocket();
    const client = new ScreenShareSessionClient({
      maxPointerMoveBufferedBytes: 64,
      webSocketFactory: () => socket as unknown as WebSocket,
    });
    client.connect(55, 8);
    socket.open();
    socket.bufferedAmount = 65;

    expect(client.send('input.pointer_move', { x: 0.25, y: 0.75 })).toBe(false);
    expect(client.send('input.pointer_button', { button: 'left', pressed: false })).toBe(true);
    expect(client.send('input.key', { code: 'Escape', pressed: false })).toBe(true);
    expect(client.send('input.release_all')).toBe(true);

    const messages = socket.sent.slice(-3).map((value) => JSON.parse(value));
    expect(messages.map((message) => message.type)).toEqual([
      'input.pointer_button',
      'input.key',
      'input.release_all',
    ]);
    expect(messages.map((message) => message.client_seq)).toEqual([2, 3, 4]);
  });

  it('reports bufferedAmount samples and the peak while preserving critical input', () => {
    const socket = new FakeSocket();
    const client = new ScreenShareSessionClient({
      maxPointerMoveBufferedBytes: 64,
      metricsEmitIntervalMs: 0,
      webSocketFactory: () => socket as unknown as WebSocket,
    });
    const listener = vi.fn();
    client.onMetrics(listener);
    client.connect(55, 8);
    socket.open();
    socket.bufferedAmount = 12;
    expect(client.send('input.pointer_move', { x: 0.1, y: 0.2 })).toBe(true);
    socket.bufferedAmount = 80;
    expect(client.send('input.pointer_move', { x: 0.2, y: 0.3 })).toBe(false);
    expect(client.send('input.release_all')).toBe(true);

    expect(client.getMetrics()).toMatchObject({
      bufferedAmountBytes: {
        sampleCount: 4,
        last: 80,
        max: 80,
      },
      pointerMoveDroppedCount: 1,
      pointerEventTimingSupport: 'caller_timestamp_required',
      serverInputAckMs: { sampleCount: 0 },
      serverInputAckSupport: 'pending',
    });
    expect(listener).toHaveBeenCalled();
    expect(listener.mock.lastCall?.[0]).toMatchObject({ pointerMoveDroppedCount: 1 });
  });

  it('measures pointer event-to-send only when the caller supplies a compatible event timestamp', () => {
    let now = 125;
    const socket = new FakeSocket();
    const client = new ScreenShareSessionClient({
      now: () => now,
      webSocketFactory: () => socket as unknown as WebSocket,
    });
    client.connect(55, 8);
    socket.open();

    expect(client.send('input.pointer_move', { x: 0.1, y: 0.2 }, 100)).toBe(true);
    now = 140;
    expect(client.send('input.pointer_button', { button: 'left', pressed: true }, 132)).toBe(true);
    // Future/mismatched clock values are rejected instead of producing fake latency.
    expect(client.send('input.pointer_move', { x: 0.2, y: 0.3 }, 500)).toBe(true);

    expect(client.getMetrics()).toMatchObject({
      pointerEventToSendMs: {
        sampleCount: 2,
        last: 8,
        min: 8,
        max: 25,
        p50: 8,
        p95: 25,
      },
      pointerEventTimingSupport: 'measured',
    });
  });

  it('correlates input ACK sequence numbers into bounded event-to-ACK metrics', () => {
    let now = 125;
    let unixNow = 10_125;
    const socket = new FakeSocket();
    const client = new ScreenShareSessionClient({
      now: () => now,
      nowUnixMs: () => unixNow,
      metricsSampleCapacity: 2,
      webSocketFactory: () => socket as unknown as WebSocket,
    });
    const traceListener = vi.fn();
    client.onInputTrace(traceListener);
    client.connect(55, 8);
    socket.open();

    expect(client.send('input.pointer_move', { x: 0.1, y: 0.2 }, 100)).toBe(true);
    const pointer = JSON.parse(socket.sent.at(-1) ?? '{}');
    now = 160;
    unixNow = 10_160;
    socket.receive({
      v: 1,
      type: 'input.ack',
      session_id: 55,
      source_epoch: 8,
      client_seq: pointer.client_seq,
      payload: { receive_to_enqueue_us: 240, queue_outcome: 'coalesced' },
    });

    expect(client.getMetrics()).toMatchObject({
      pendingInputAckCount: 0,
      pointerEventToServerAckMs: { sampleCount: 1, last: 60 },
      serverInputAckMs: { sampleCount: 1, last: 60 },
      serverReceiveToEnqueueUs: { sampleCount: 1, last: 240 },
      serverInputAckSupport: 'measured',
    });
    expect(traceListener).toHaveBeenNthCalledWith(1, {
      phase: 'sent',
      clientSequence: pointer.client_seq,
      inputType: 'input.pointer_move',
      occurredAtClientUnixMs: 10_100,
      observedAtClientUnixMs: 10_125,
    });
    expect(traceListener).toHaveBeenNthCalledWith(2, {
      phase: 'acknowledged',
      clientSequence: pointer.client_seq,
      inputType: 'input.pointer_move',
      occurredAtClientUnixMs: 10_100,
      observedAtClientUnixMs: 10_160,
    });
  });

  it('ends control and best-effort releases inputs when a critical ACK becomes stale', () => {
    vi.useFakeTimers();
    let now = 10;
    const socket = new FakeSocket();
    const client = new ScreenShareSessionClient({
      now: () => now,
      maxCriticalInputAgeMs: 100,
      webSocketFactory: () => socket as unknown as WebSocket,
    });
    client.connect(55, 8);
    socket.open();

    expect(client.send('input.pointer_button', { button: 'left', pressed: true }, 10)).toBe(true);
    now = 111;
    vi.advanceTimersByTime(101);

    const types = socket.sent.map((value) => JSON.parse(value).type);
    expect(types.slice(-2)).toEqual(['input.release_all', 'control.release']);
    expect(socket.closed).toBe(true);
    expect(client.getMetrics()).toMatchObject({
      criticalInputAbortCount: 1,
      pendingInputAckCount: 0,
    });
  });

  it('does not send an already stale critical input', () => {
    const socket = new FakeSocket();
    const client = new ScreenShareSessionClient({
      now: () => 800,
      maxCriticalInputAgeMs: 100,
      webSocketFactory: () => socket as unknown as WebSocket,
    });
    client.connect(55, 8);
    socket.open();

    expect(client.send('input.key', { code: 'KeyA', pressed: true }, 600)).toBe(false);

    const types = socket.sent.map((value) => JSON.parse(value).type);
    expect(types).not.toContain('input.key');
    expect(types.slice(-2)).toEqual(['input.release_all', 'control.release']);
    expect(socket.closed).toBe(true);
  });

  it('ends control when the browser rejects a critical input send', () => {
    const socket = new FakeSocket();
    const client = new ScreenShareSessionClient({
      webSocketFactory: () => socket as unknown as WebSocket,
    });
    client.connect(55, 8);
    socket.open();
    socket.throwOnSend = true;

    expect(client.send('input.pointer_button', { button: 'left', pressed: false })).toBe(false);

    expect(socket.closed).toBe(true);
    expect(client.getMetrics().criticalInputAbortCount).toBe(1);
  });

  it('keeps control active when the critical input is acknowledged before its deadline', () => {
    vi.useFakeTimers();
    const socket = new FakeSocket();
    const client = new ScreenShareSessionClient({
      now: () => 10,
      maxCriticalInputAgeMs: 100,
      webSocketFactory: () => socket as unknown as WebSocket,
    });
    client.connect(55, 8);
    socket.open();

    expect(client.send('input.release_all')).toBe(true);
    const release = JSON.parse(socket.sent.at(-1) ?? '{}');
    socket.receive({
      v: 1,
      type: 'input.ack',
      session_id: 55,
      source_epoch: 8,
      client_seq: release.client_seq,
      payload: { receive_to_enqueue_us: 10 },
    });
    vi.advanceTimersByTime(101);

    expect(socket.closed).toBe(false);
    expect(client.getMetrics().criticalInputAbortCount).toBe(0);
  });
});
