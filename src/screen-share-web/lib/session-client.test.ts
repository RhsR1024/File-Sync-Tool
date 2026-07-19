import { afterEach, describe, expect, it, vi } from 'vitest';

import { ScreenShareSessionClient } from './session-client';

class FakeSocket extends EventTarget {
  readyState: number = WebSocket.CONNECTING;
  readonly sent: string[] = [];

  open() {
    this.readyState = WebSocket.OPEN;
    this.dispatchEvent(new Event('open'));
  }

  send(value: string) {
    this.sent.push(value);
  }

  close() {
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
});

describe('screen share session client', () => {
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
});
