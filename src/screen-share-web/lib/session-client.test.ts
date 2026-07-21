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
});
