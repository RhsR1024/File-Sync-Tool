import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  WebRtcH264Player,
  supportsReceiveOnlyWebRtc,
  waitForIceGatheringComplete,
} from './webrtc-player';

class FakePeerConnection extends EventTarget {
  readonly configuration: RTCConfiguration;
  readonly transceivers: Array<{ kind: string; init?: RTCRtpTransceiverInit }> = [];
  readonly remoteDescriptions: RTCSessionDescriptionInit[] = [];
  localDescription: RTCSessionDescription | null = null;
  iceGatheringState: RTCIceGatheringState = 'new';
  connectionState: RTCPeerConnectionState = 'new';
  ontrack: ((this: RTCPeerConnection, ev: RTCTrackEvent) => unknown) | null = null;
  onconnectionstatechange: ((this: RTCPeerConnection, ev: Event) => unknown) | null = null;
  closed = false;

  constructor(configuration: RTCConfiguration) {
    super();
    this.configuration = configuration;
  }

  addTransceiver(kind: string, init?: RTCRtpTransceiverInit): RTCRtpTransceiver {
    this.transceivers.push({ kind, init });
    return {} as RTCRtpTransceiver;
  }

  async createOffer(): Promise<RTCSessionDescriptionInit> {
    return { type: 'offer', sdp: 'v=0\r\n' };
  }

  async setLocalDescription(description: RTCSessionDescriptionInit): Promise<void> {
    this.localDescription = description as RTCSessionDescription;
    this.iceGatheringState = 'complete';
    this.dispatchEvent(new Event('icegatheringstatechange'));
  }

  async setRemoteDescription(description: RTCSessionDescriptionInit): Promise<void> {
    this.remoteDescriptions.push(description);
  }

  close(): void {
    this.closed = true;
    this.connectionState = 'closed';
  }

  setConnectionState(state: RTCPeerConnectionState): void {
    this.connectionState = state;
    this.onconnectionstatechange?.call(this as unknown as RTCPeerConnection, new Event('connectionstatechange'));
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

describe('receive-only WebRTC player', () => {
  it('detects RTCPeerConnection without requiring a secure context', () => {
    vi.stubGlobal('RTCPeerConnection', class {});
    expect(supportsReceiveOnlyWebRtc()).toBe(true);
    vi.stubGlobal('RTCPeerConnection', undefined);
    expect(supportsReceiveOnlyWebRtc()).toBe(false);
  });

  it('signals one recvonly video transceiver with host candidates only', async () => {
    let peer: FakePeerConnection | null = null;
    const fetcher = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      expect(init?.method).toBe('POST');
      expect(JSON.parse(String(init?.body))).toEqual({ type: 'offer', sdp: 'v=0\r\n' });
      return new Response(JSON.stringify({ type: 'answer', sdp: 'v=0\r\nanswer' }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    const states: string[] = [];
    const player = new WebRtcH264Player(createVideo(), {
      peerConnectionFactory: (configuration) => {
        peer = new FakePeerConnection(configuration);
        return peer as unknown as RTCPeerConnection;
      },
      fetcher: fetcher as typeof fetch,
      onStateChange: (state) => states.push(state),
    });

    await player.start();

    expect(peer).not.toBeNull();
    const configuredPeer = peer as FakePeerConnection | null;
    expect(configuredPeer?.configuration.iceServers).toEqual([]);
    expect(configuredPeer?.transceivers).toEqual([
      { kind: 'video', init: { direction: 'recvonly' } },
    ]);
    expect(configuredPeer?.remoteDescriptions).toEqual([
      { type: 'answer', sdp: 'v=0\r\nanswer' },
    ]);
    expect(states).toEqual(['signaling', 'connecting']);
  });

  it('tracks connection recovery and closes a failed peer', async () => {
    let peer!: FakePeerConnection;
    const states: string[] = [];
    const player = new WebRtcH264Player(createVideo(), {
      peerConnectionFactory: (configuration) => {
        peer = new FakePeerConnection(configuration);
        return peer as unknown as RTCPeerConnection;
      },
      fetcher: vi.fn(async () => new Response(
        JSON.stringify({
          type: 'answer',
          sdp: 'v=0\r\na=extmap:9/recvonly http://www.webrtc.org/experiments/rtp-hdrext/abs-capture-time\r\n',
        }),
        { status: 200 },
      )) as typeof fetch,
      onStateChange: (state) => states.push(state),
    });
    await player.start();
    peer.setConnectionState('connected');
    peer.setConnectionState('disconnected');
    peer.setConnectionState('connected');
    peer.setConnectionState('failed');

    expect(states).toEqual([
      'signaling',
      'connecting',
      'connected',
      'disconnected',
      'connected',
      'failed',
    ]);
    expect(peer.closed).toBe(true);
  });

  it('does not report ready until the first video frame is presentable', async () => {
    let peer!: FakePeerConnection;
    const video = createVideo();
    const states: string[] = [];
    const player = new WebRtcH264Player(video, {
      peerConnectionFactory: (configuration) => {
        peer = new FakePeerConnection(configuration);
        return peer as unknown as RTCPeerConnection;
      },
      fetcher: vi.fn(async () => new Response(
        JSON.stringify({ type: 'answer', sdp: 'answer' }),
        { status: 200 },
      )) as typeof fetch,
      onStateChange: (state) => states.push(state),
    });
    await player.start();
    peer.ontrack?.call(peer as unknown as RTCPeerConnection, {
      streams: [{} as MediaStream],
      track: {} as MediaStreamTrack,
    } as unknown as RTCTrackEvent);
    peer.setConnectionState('connected');
    expect(player.getState()).toBe('connected');

    video.dispatchEvent(new Event('loadeddata'));
    expect(player.getState()).toBe('ready');
    expect(player.getMetrics().firstFramePresentedAtUnixMs).not.toBeNull();
    expect(states.slice(-2)).toEqual(['connected', 'ready']);
  });

  it('continuously records browser WebRTC presentation timing proxies', async () => {
    let peer!: FakePeerConnection;
    let nowUnixMs = 1_000;
    const callbacks: VideoFrameRequestCallback[] = [];
    const video = createVideo();
    Object.defineProperty(video, 'requestVideoFrameCallback', {
      configurable: true,
      value: vi.fn((callback: VideoFrameRequestCallback) => {
        callbacks.push(callback);
        return callbacks.length;
      }),
    });
    Object.defineProperty(video, 'cancelVideoFrameCallback', {
      configurable: true,
      value: vi.fn(),
    });
    const player = new WebRtcH264Player(video, {
      peerConnectionFactory: (configuration) => {
        peer = new FakePeerConnection(configuration);
        return peer as unknown as RTCPeerConnection;
      },
      fetcher: vi.fn(async () => new Response(
        JSON.stringify({
          type: 'answer',
          sdp: 'v=0\r\na=extmap:9/recvonly http://www.webrtc.org/experiments/rtp-hdrext/abs-capture-time\r\n',
        }),
        { status: 200 },
      )) as typeof fetch,
      nowUnixMs: () => nowUnixMs,
    });
    await player.start();
    peer.ontrack?.call(peer as unknown as RTCPeerConnection, {
      streams: [{} as MediaStream],
      track: {} as MediaStreamTrack,
    } as unknown as RTCTrackEvent);
    peer.setConnectionState('connected');
    expect(callbacks).toHaveLength(1);

    callbacks[0]?.(100, {
      mediaTime: 0.5,
      expectedDisplayTime: 105,
      presentationTime: 100,
      presentedFrames: 1,
      width: 1280,
      height: 720,
      processingDuration: 0,
      captureTime: 50,
      receiveTime: 80,
    } as VideoFrameCallbackMetadata);
    expect(callbacks).toHaveLength(2);
    expect(player.getMetrics()).toMatchObject({
      firstFramePresentedAtUnixMs: 1_005,
      presentationCallbacks: 1,
      presentationTraceSource: 'expected-display-time',
      browserCaptureToDisplayMs: { sampleCount: 1, last: 55 },
      browserReceiveToDisplayMs: { sampleCount: 1, last: 25 },
      absoluteCaptureTime: {
        negotiated: true,
        browserCaptureTimeSamples: 1,
        validation: 'pending-target-browser-correlation',
      },
      endToEndLatency: { available: false },
    });

    nowUnixMs = 1_020;
    callbacks[1]?.(120, {
      mediaTime: 0.516,
      expectedDisplayTime: Number.NaN,
      presentationTime: 120,
      presentedFrames: 2,
      width: 1280,
      height: 720,
      processingDuration: 0,
    } as VideoFrameCallbackMetadata);
    expect(player.getMetrics()).toMatchObject({
      firstFramePresentedAtUnixMs: 1_005,
      presentationCallbacks: 2,
      presentationTraceSource: 'callback-time',
    });
    player.stop();
  });

  it('cleans stats and media resources when a remote peer closes', async () => {
    vi.useFakeTimers();
    let peer!: FakePeerConnection;
    const video = createVideo();
    const player = new WebRtcH264Player(video, {
      peerConnectionFactory: (configuration) => {
        peer = new FakePeerConnection(configuration);
        (peer as unknown as { getStats: () => Promise<RTCStatsReport> }).getStats = vi.fn(
          async () => new Map() as unknown as RTCStatsReport,
        );
        return peer as unknown as RTCPeerConnection;
      },
      fetcher: vi.fn(async () => new Response(
        JSON.stringify({ type: 'answer', sdp: 'answer' }),
        { status: 200 },
      )) as typeof fetch,
      statsIntervalMs: 100,
    });
    await player.start();
    expect(vi.getTimerCount()).toBe(1);

    peer.setConnectionState('closed');
    expect(player.getState()).toBe('closed');
    expect(video.srcObject).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
    vi.useRealTimers();
  });

  it('fails closed when signaling returns a non-answer', async () => {
    let peer!: FakePeerConnection;
    const player = new WebRtcH264Player(createVideo(), {
      peerConnectionFactory: (configuration) => {
        peer = new FakePeerConnection(configuration);
        return peer as unknown as RTCPeerConnection;
      },
      fetcher: vi.fn(async () => new Response(
        JSON.stringify({ type: 'offer', sdp: 'wrong' }),
        { status: 200 },
      )) as typeof fetch,
    });

    await expect(player.start()).rejects.toThrow('invalid SDP answer');
    expect(player.getState()).toBe('failed');
    expect(peer.closed).toBe(true);
  });

  it('bounds ICE gathering waits', async () => {
    vi.useFakeTimers();
    const peer = new FakePeerConnection({});
    const result = waitForIceGatheringComplete(
      peer as unknown as RTCPeerConnection,
      100,
    );
    const expectation = expect(result).rejects.toThrow('Timed out');
    await vi.advanceTimersByTimeAsync(100);
    await expectation;
    vi.useRealTimers();
  });
});
