import { monotonicUnixNow } from './latency-trace';
import { RollingNumericMetric, type NumericMetricSnapshot } from './metrics';

export type WebRtcPlayerState =
  | 'idle'
  | 'signaling'
  | 'connecting'
  | 'connected'
  | 'ready'
  | 'disconnected'
  | 'failed'
  | 'closed';

export interface WebRtcPlayerOptions {
  offerUrl?: string;
  peerConnectionFactory?: (configuration: RTCConfiguration) => RTCPeerConnection;
  fetcher?: typeof fetch;
  iceGatherTimeoutMs?: number;
  onStateChange?: (state: WebRtcPlayerState) => void;
  statsIntervalMs?: number;
  metricsSampleCapacity?: number;
  nowUnixMs?: () => number;
}

export interface WebRtcPlayerMetrics {
  capturedAtUnixMs: number;
  firstFramePresentedAtUnixMs: number | null;
  inboundRtp: Record<string, number> | null;
  candidatePair: Record<string, number> | null;
  presentationCallbacks: number;
  presentationTraceSource: 'expected-display-time' | 'callback-time' | 'event-fallback' | 'unavailable';
  /** Browser WebRTC timing proxy; not the authoritative server capture trace. */
  browserCaptureToDisplayMs: NumericMetricSnapshot;
  browserReceiveToDisplayMs: NumericMetricSnapshot;
  absoluteCaptureTime: {
    negotiated: boolean;
    browserCaptureTimeSamples: number;
    validation: 'not-negotiated' | 'awaiting-browser-sample' | 'pending-target-browser-correlation';
  };
  endToEndLatency: {
    available: false;
    reason: string;
  };
}

const DEFAULT_OFFER_URL = '/api/screenshare/webrtc/offer';
const DEFAULT_ICE_GATHER_TIMEOUT_MS = 5_000;
const MAX_SDP_BYTES = 256 * 1024;
const ABSOLUTE_CAPTURE_TIME_URI =
  'http://www.webrtc.org/experiments/rtp-hdrext/abs-capture-time';

export function supportsReceiveOnlyWebRtc(): boolean {
  return typeof RTCPeerConnection !== 'undefined';
}

export function waitForIceGatheringComplete(
  peerConnection: RTCPeerConnection,
  timeoutMs = DEFAULT_ICE_GATHER_TIMEOUT_MS,
): Promise<void> {
  if (peerConnection.iceGatheringState === 'complete') return Promise.resolve();
  return new Promise((resolve, reject) => {
    let timeoutId: ReturnType<typeof setTimeout> | null = setTimeout(() => {
      cleanup();
      reject(new Error('Timed out gathering LAN ICE candidates'));
    }, timeoutMs);
    const handleStateChange = () => {
      if (peerConnection.iceGatheringState === 'complete') {
        cleanup();
        resolve();
      }
    };
    const cleanup = () => {
      peerConnection.removeEventListener('icegatheringstatechange', handleStateChange);
      if (timeoutId !== null) {
        clearTimeout(timeoutId);
        timeoutId = null;
      }
    };
    peerConnection.addEventListener('icegatheringstatechange', handleStateChange);
  });
}

export class WebRtcH264Player {
  private readonly video: HTMLVideoElement;
  private readonly offerUrl: string;
  private readonly peerConnectionFactory: (configuration: RTCConfiguration) => RTCPeerConnection;
  private readonly usesInjectedPeerConnectionFactory: boolean;
  private readonly fetcher: typeof fetch;
  private readonly iceGatherTimeoutMs: number;
  private readonly onStateChange?: (state: WebRtcPlayerState) => void;
  private readonly statsIntervalMs: number;
  private readonly metricsSampleCapacity: number;
  private readonly nowUnixMs: () => number;
  private peerConnection: RTCPeerConnection | null = null;
  private abortController: AbortController | null = null;
  private generation = 0;
  private state: WebRtcPlayerState = 'idle';
  private statsTimer: number | null = null;
  private videoFrameCallbackHandle: number | null = null;
  private firstFrameListener: (() => void) | null = null;
  private hasPresentedFrame = false;
  private metrics: WebRtcPlayerMetrics = emptyMetrics();
  private browserCaptureToDisplayMs = new RollingNumericMetric();
  private browserReceiveToDisplayMs = new RollingNumericMetric();

  constructor(video: HTMLVideoElement, options: WebRtcPlayerOptions = {}) {
    this.video = video;
    this.offerUrl = options.offerUrl ?? DEFAULT_OFFER_URL;
    this.usesInjectedPeerConnectionFactory = options.peerConnectionFactory !== undefined;
    this.peerConnectionFactory = options.peerConnectionFactory
      ?? ((configuration) => new RTCPeerConnection(configuration));
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.iceGatherTimeoutMs = options.iceGatherTimeoutMs ?? DEFAULT_ICE_GATHER_TIMEOUT_MS;
    this.onStateChange = options.onStateChange;
    this.statsIntervalMs = options.statsIntervalMs ?? 1_000;
    this.metricsSampleCapacity = options.metricsSampleCapacity ?? 512;
    this.nowUnixMs = options.nowUnixMs ?? monotonicUnixNow;
  }

  getState(): WebRtcPlayerState {
    return this.state;
  }

  getMetrics(): WebRtcPlayerMetrics {
    return {
      ...this.metrics,
      inboundRtp: this.metrics.inboundRtp ? { ...this.metrics.inboundRtp } : null,
      candidatePair: this.metrics.candidatePair ? { ...this.metrics.candidatePair } : null,
    };
  }

  async start(): Promise<void> {
    this.stop();
    this.browserCaptureToDisplayMs = new RollingNumericMetric(this.metricsSampleCapacity);
    this.browserReceiveToDisplayMs = new RollingNumericMetric(this.metricsSampleCapacity);
    this.metrics = emptyMetrics(this.nowUnixMs());
    if (!supportsReceiveOnlyWebRtc() && !this.usesInjectedPeerConnectionFactory) {
      throw new Error('WebRTC is not supported by this browser');
    }

    const generation = ++this.generation;
    const abortController = new AbortController();
    this.abortController = abortController;
    const peerConnection = this.peerConnectionFactory({
      // The LAN mode intentionally does not contact STUN or TURN services.
      iceServers: [],
      iceTransportPolicy: 'all',
      bundlePolicy: 'max-bundle',
    });
    this.peerConnection = peerConnection;
    this.setState('signaling');

    peerConnection.addTransceiver('video', { direction: 'recvonly' });
    peerConnection.ontrack = (event) => {
      if (generation !== this.generation) return;
      const stream = event.streams[0] ?? new MediaStream([event.track]);
      this.video.autoplay = true;
      this.video.playsInline = true;
      this.video.muted = true;
      this.video.srcObject = stream;
      void this.video.play().catch(() => undefined);
      this.armFirstFramePresentation(peerConnection, generation);
    };
    peerConnection.onconnectionstatechange = () => {
      if (generation !== this.generation) return;
      switch (peerConnection.connectionState) {
        case 'connected':
          this.setState('connected');
          if (this.hasPresentedFrame) this.setState('ready');
          else this.armFirstFramePresentation(peerConnection, generation);
          break;
        case 'disconnected':
          this.hasPresentedFrame = false;
          this.setState('disconnected');
          break;
        case 'failed':
          this.disposePeer(peerConnection, true);
          this.setState('failed');
          break;
        case 'closed':
          this.disposePeer(peerConnection, true);
          this.setState('closed');
          break;
        default:
          if (this.state === 'signaling') this.setState('connecting');
      }
    };

    try {
      const offer = await peerConnection.createOffer();
      if (generation !== this.generation) return;
      await peerConnection.setLocalDescription(offer);
      await waitForIceGatheringComplete(peerConnection, this.iceGatherTimeoutMs);
      const localDescription = peerConnection.localDescription;
      if (!localDescription || localDescription.type !== 'offer') {
        throw new Error('Browser did not produce an SDP offer');
      }
      if (localDescription.sdp.length > MAX_SDP_BYTES) {
        throw new Error('Browser SDP offer is too large');
      }
      const response = await this.fetcher(this.offerUrl, {
        method: 'POST',
        credentials: 'same-origin',
        headers: {
          Accept: 'application/json',
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(localDescription),
        signal: abortController.signal,
      });
      if (!response.ok) {
        const message = await response.text().catch(() => response.statusText);
        throw new Error(message || `WebRTC offer failed (${response.status})`);
      }
      const answer = await response.json() as RTCSessionDescriptionInit;
      if (answer.type !== 'answer' || typeof answer.sdp !== 'string' || answer.sdp.length === 0) {
        throw new Error('Server returned an invalid SDP answer');
      }
      if (generation !== this.generation) return;
      const absoluteCaptureTimeNegotiated = sdpNegotiatesExtension(
        answer.sdp,
        ABSOLUTE_CAPTURE_TIME_URI,
      );
      this.metrics = {
        ...this.metrics,
        absoluteCaptureTime: {
          negotiated: absoluteCaptureTimeNegotiated,
          browserCaptureTimeSamples: 0,
          validation: absoluteCaptureTimeNegotiated
            ? 'awaiting-browser-sample'
            : 'not-negotiated',
        },
        endToEndLatency: {
          available: false,
          reason: absoluteCaptureTimeNegotiated
            ? 'Absolute Capture Time was negotiated; waiting for a target-browser captureTime sample and capture-sequence correlation'
            : 'Absolute Capture Time was not negotiated',
        },
      };
      await peerConnection.setRemoteDescription(answer);
      if (this.abortController === abortController) this.abortController = null;
      this.startStatsSampling(peerConnection, generation);
      if (this.state === 'signaling') this.setState('connecting');
    } catch (error) {
      if (generation !== this.generation || abortController.signal.aborted) return;
      this.disposePeer(peerConnection, true);
      this.setState('failed');
      throw error;
    }
  }

  stop(): void {
    const wasStarted = this.peerConnection !== null
      || this.abortController !== null
      || this.state !== 'idle';
    ++this.generation;
    const peerConnection = this.peerConnection;
    if (peerConnection) this.disposePeer(peerConnection, true);
    else {
      this.abortController?.abort();
      this.abortController = null;
      this.clearStatsSampling();
      this.clearFirstFramePresentation();
      this.video.srcObject = null;
    }
    if (wasStarted) this.setState('closed');
  }

  private setState(state: WebRtcPlayerState): void {
    if (this.state === state) return;
    this.state = state;
    this.onStateChange?.(state);
  }

  private startStatsSampling(peerConnection: RTCPeerConnection, generation: number): void {
    if (typeof peerConnection.getStats !== 'function') return;
    const sample = async () => {
      if (generation !== this.generation) return;
      try {
        const report = await peerConnection.getStats();
        if (generation !== this.generation) return;
        const inbound = Array.from(report.values()).find((entry) => (
          entry.type === 'inbound-rtp' && (entry.kind === 'video' || entry.mediaType === 'video')
        ));
        const candidate = Array.from(report.values()).find((entry) => (
          entry.type === 'candidate-pair' && entry.state === 'succeeded' && entry.nominated === true
        ));
        this.metrics = {
          ...this.metrics,
          capturedAtUnixMs: this.nowUnixMs(),
          inboundRtp: numericStats(inbound, [
            'packetsReceived', 'packetsLost', 'jitter', 'bytesReceived',
            'framesDecoded', 'framesDropped', 'framesPerSecond', 'keyFramesDecoded',
            'jitterBufferDelay', 'jitterBufferEmittedCount', 'totalDecodeTime',
            'totalProcessingDelay', 'freezeCount', 'totalFreezesDuration',
            'nackCount', 'pliCount', 'firCount', 'qpSum',
          ]),
          candidatePair: numericStats(candidate, [
            'currentRoundTripTime', 'availableIncomingBitrate', 'availableOutgoingBitrate',
          ]),
        };
      } catch {
        // Stats are diagnostic-only; inability to sample must not stop media.
      }
    };
    void sample();
    this.statsTimer = window.setInterval(sample, this.statsIntervalMs);
  }

  private armFirstFramePresentation(
    peerConnection: RTCPeerConnection,
    generation: number,
  ): void {
    if (generation !== this.generation || peerConnection !== this.peerConnection) return;
    if (!this.hasPresentedFrame && this.firstFrameListener === null) {
      this.firstFrameListener = () => this.markFirstFramePresented(peerConnection, generation);
      this.video.addEventListener('loadeddata', this.firstFrameListener);
      this.video.addEventListener('playing', this.firstFrameListener);
    }
    if (
      this.videoFrameCallbackHandle === null
      && typeof this.video.requestVideoFrameCallback === 'function'
    ) {
      this.videoFrameCallbackHandle = this.video.requestVideoFrameCallback((now, metadata) => {
        this.videoFrameCallbackHandle = null;
        this.recordPresentedFrame(peerConnection, generation, now, metadata);
        this.armFirstFramePresentation(peerConnection, generation);
      });
    }
  }

  private markFirstFramePresented(
    peerConnection: RTCPeerConnection,
    generation: number,
  ): void {
    if (generation !== this.generation || peerConnection !== this.peerConnection) return;
    this.hasPresentedFrame = true;
    if (this.metrics.firstFramePresentedAtUnixMs === null) {
      this.metrics = {
        ...this.metrics,
        firstFramePresentedAtUnixMs: this.nowUnixMs(),
        presentationTraceSource: 'event-fallback',
      };
    }
    this.clearFirstFrameEventListeners();
    if (peerConnection.connectionState === 'connected') this.setState('ready');
  }

  private recordPresentedFrame(
    peerConnection: RTCPeerConnection,
    generation: number,
    now: DOMHighResTimeStamp,
    metadata: VideoFrameCallbackMetadata,
  ): void {
    if (generation !== this.generation || peerConnection !== this.peerConnection) return;
    const expectedDisplayDelta = Number.isFinite(metadata.expectedDisplayTime)
      ? metadata.expectedDisplayTime - now
      : Number.NaN;
    const usesExpectedDisplayTime = Number.isFinite(expectedDisplayDelta)
      && Math.abs(expectedDisplayDelta) <= 1_000;
    const displayPerformanceMs = usesExpectedDisplayTime ? metadata.expectedDisplayTime : now;
    const timing = metadata as VideoFrameCallbackMetadata & {
      captureTime?: number;
      receiveTime?: number;
    };
    if (Number.isFinite(timing.captureTime)) {
      const elapsed = displayPerformanceMs - (timing.captureTime as number);
      if (elapsed >= 0 && elapsed <= 60_000) this.browserCaptureToDisplayMs.add(elapsed);
    }
    if (Number.isFinite(timing.receiveTime)) {
      const elapsed = displayPerformanceMs - (timing.receiveTime as number);
      if (elapsed >= 0 && elapsed <= 60_000) this.browserReceiveToDisplayMs.add(elapsed);
    }
    const presentedAtUnixMs = this.nowUnixMs() + (usesExpectedDisplayTime ? expectedDisplayDelta : 0);
    const browserCaptureTimeSamples = this.browserCaptureToDisplayMs.snapshot().sampleCount;
    const hasNegotiatedCaptureSample = this.metrics.absoluteCaptureTime.negotiated
      && browserCaptureTimeSamples > 0;
    this.hasPresentedFrame = true;
    this.metrics = {
      ...this.metrics,
      firstFramePresentedAtUnixMs: this.metrics.firstFramePresentedAtUnixMs ?? presentedAtUnixMs,
      presentationCallbacks: this.metrics.presentationCallbacks + 1,
      presentationTraceSource: usesExpectedDisplayTime ? 'expected-display-time' : 'callback-time',
      browserCaptureToDisplayMs: this.browserCaptureToDisplayMs.snapshot(),
      browserReceiveToDisplayMs: this.browserReceiveToDisplayMs.snapshot(),
      absoluteCaptureTime: {
        ...this.metrics.absoluteCaptureTime,
        browserCaptureTimeSamples,
        validation: hasNegotiatedCaptureSample
          ? 'pending-target-browser-correlation'
          : this.metrics.absoluteCaptureTime.negotiated
            ? 'awaiting-browser-sample'
            : 'not-negotiated',
      },
      endToEndLatency: {
        available: false,
        reason: hasNegotiatedCaptureSample
          ? 'Absolute Capture Time and browser captureTime are present, but the target-browser sample is not yet correlated to the server capture sequence'
          : this.metrics.absoluteCaptureTime.negotiated
            ? 'Absolute Capture Time was negotiated, but the browser has not exposed a valid captureTime sample'
            : 'Absolute Capture Time was not negotiated',
      },
    };
    this.clearFirstFrameEventListeners();
    if (peerConnection.connectionState === 'connected') this.setState('ready');
  }

  private disposePeer(peerConnection: RTCPeerConnection, clearVideo: boolean): void {
    if (this.peerConnection === peerConnection) this.peerConnection = null;
    peerConnection.ontrack = null;
    peerConnection.onconnectionstatechange = null;
    this.abortController?.abort();
    this.abortController = null;
    this.clearStatsSampling();
    this.clearFirstFramePresentation();
    this.hasPresentedFrame = false;
    peerConnection.close();
    if (clearVideo) this.video.srcObject = null;
  }

  private clearStatsSampling(): void {
    if (this.statsTimer !== null) {
      window.clearInterval(this.statsTimer);
      this.statsTimer = null;
    }
  }

  private clearFirstFramePresentation(): void {
    if (
      this.videoFrameCallbackHandle !== null
      && typeof this.video.cancelVideoFrameCallback === 'function'
    ) {
      this.video.cancelVideoFrameCallback(this.videoFrameCallbackHandle);
    }
    this.videoFrameCallbackHandle = null;
    this.clearFirstFrameEventListeners();
  }

  private clearFirstFrameEventListeners(): void {
    if (this.firstFrameListener !== null) {
      this.video.removeEventListener('loadeddata', this.firstFrameListener);
      this.video.removeEventListener('playing', this.firstFrameListener);
      this.firstFrameListener = null;
    }
  }
}

function emptyMetrics(capturedAtUnixMs = monotonicUnixNow()): WebRtcPlayerMetrics {
  return {
    capturedAtUnixMs,
    firstFramePresentedAtUnixMs: null,
    inboundRtp: null,
    candidatePair: null,
    presentationCallbacks: 0,
    presentationTraceSource: 'unavailable',
    browserCaptureToDisplayMs: new RollingNumericMetric(0).snapshot(),
    browserReceiveToDisplayMs: new RollingNumericMetric(0).snapshot(),
    absoluteCaptureTime: {
      negotiated: false,
      browserCaptureTimeSamples: 0,
      validation: 'not-negotiated',
    },
    endToEndLatency: {
      available: false,
      reason: 'Absolute Capture Time was not negotiated',
    },
  };
}

function sdpNegotiatesExtension(sdp: string, expectedUri: string): boolean {
  return sdp.split(/\r?\n/u).some((line) => {
    if (!line.startsWith('a=extmap:')) return false;
    return line.slice('a=extmap:'.length).trim().split(/\s+/u)[1] === expectedUri;
  });
}

function numericStats(
  report: RTCStats | undefined,
  names: string[],
): Record<string, number> | null {
  if (!report) return null;
  const values: Record<string, number> = {};
  const record = report as unknown as Record<string, unknown>;
  for (const name of names) {
    const value = record[name];
    if (typeof value === 'number' && Number.isFinite(value)) {
      values[name] = value;
    }
  }
  return Object.keys(values).length > 0 ? values : null;
}
