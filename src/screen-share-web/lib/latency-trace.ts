import { RollingNumericMetric, type NumericMetricSnapshot } from './metrics';

export interface ClockSyncSample {
  requestStartedAtUnixMs: number;
  responseFinishedAtUnixMs: number;
  serverUnixMs: number;
}

export interface ClockSyncSnapshot {
  sampleCount: number;
  offsetMs: number | null;
  roundTripMs: number | null;
  offsetRangeMs: number | null;
  lastOffsetMs: number | null;
  clockDiscontinuityCount: number;
}

export interface MediaTracePoint {
  generation: number;
  sequence: number;
  captureSequence: number;
  capturedAtServerUnixMs: number;
  mediaTimeUs: number;
  durationUs: number;
  visibleInputSequence?: number | null;
  inputAppliedAtServerUnixMs?: number | null;
}

export interface EndToEndLatencySnapshot {
  clock: ClockSyncSnapshot;
  captureToDisplayMs: NumericMetricSnapshot;
  inputToQueueAcknowledgementMs: NumericMetricSnapshot;
  inputToSendInputMs: NumericMetricSnapshot;
  inputToVisibleResponseMs: NumericMetricSnapshot;
  pendingMediaTracePoints: number;
  pendingInputEvents: number;
}

interface NormalizedClockSample extends ClockSyncSample {
  roundTripMs: number;
  offsetMs: number;
}

function emptyNumericMetric(): NumericMetricSnapshot {
  return {
    sampleCount: 0,
    retainedSampleCount: 0,
    last: null,
    min: null,
    max: null,
    average: null,
    p50: null,
    p95: null,
    p99: null,
  };
}

/**
 * Estimates server-minus-client clock offset using the lowest-RTT sample.
 * This is deliberately an estimate: reports retain RTT so clock uncertainty is
 * never hidden behind a falsely precise capture-to-display number.
 */
export class ServerClockEstimator {
  private readonly samples: NormalizedClockSample[] = [];
  private clockDiscontinuityCount = 0;

  constructor(private readonly capacity = 16) {}

  add(sample: ClockSyncSample): boolean {
    const roundTripMs = sample.responseFinishedAtUnixMs - sample.requestStartedAtUnixMs;
    if (
      !Number.isFinite(roundTripMs)
      || roundTripMs < 0
      || !Number.isFinite(sample.serverUnixMs)
    ) return false;
    const midpoint = sample.requestStartedAtUnixMs + roundTripMs / 2;
    const normalized = { ...sample, roundTripMs, offsetMs: sample.serverUnixMs - midpoint };
    const current = this.bestSample();
    // An NTP/manual clock step must start a new calibration epoch. Keeping
    // samples from both sides of a wall-clock jump would manufacture negative
    // or extremely large capture-to-display values until the rolling window
    // eventually evicted the old epoch.
    const discontinuityThresholdMs = Math.max(
      250,
      (current?.roundTripMs ?? 0) + normalized.roundTripMs,
    );
    if (current && Math.abs(normalized.offsetMs - current.offsetMs) > discontinuityThresholdMs) {
      this.samples.length = 0;
      this.clockDiscontinuityCount += 1;
    }
    this.samples.push(normalized);
    if (this.samples.length > this.capacity) this.samples.shift();
    return true;
  }

  snapshot(): ClockSyncSnapshot {
    const best = this.bestSample();
    const offsets = this.samples.map(sample => sample.offsetMs);
    const offsetRangeMs = offsets.length === 0
      ? null
      : Math.max(...offsets) - Math.min(...offsets);
    return {
      sampleCount: this.samples.length,
      offsetMs: best?.offsetMs ?? null,
      roundTripMs: best?.roundTripMs ?? null,
      offsetRangeMs,
      lastOffsetMs: this.samples.at(-1)?.offsetMs ?? null,
      clockDiscontinuityCount: this.clockDiscontinuityCount,
    };
  }

  serverToClientUnixMs(serverUnixMs: number): number | null {
    const offset = this.snapshot().offsetMs;
    return offset === null ? null : serverUnixMs - offset;
  }

  private bestSample(): NormalizedClockSample | null {
    return this.samples.reduce<NormalizedClockSample | null>(
      (current, sample) => current === null || sample.roundTripMs < current.roundTripMs ? sample : current,
      null,
    );
  }
}

/** A Unix-compatible clock that does not move backwards after wall-clock steps. */
export function monotonicUnixNow(): number {
  const performanceClock = globalThis.performance;
  if (
    performanceClock
    && Number.isFinite(performanceClock.timeOrigin)
    && typeof performanceClock.now === 'function'
  ) {
    return performanceClock.timeOrigin + performanceClock.now();
  }
  return Date.now();
}

/** Bounded correlator for server capture/input sequences and displayed frames. */
export class EndToEndLatencyTrace {
  private readonly clock: ServerClockEstimator;
  private readonly mediaPoints: MediaTracePoint[] = [];
  private readonly inputOccurredAtUnixMs = new Map<number, number>();
  private readonly measuredSendInputSequences = new Set<number>();
  private readonly captureToDisplayMs: RollingNumericMetric;
  private readonly inputToQueueAcknowledgementMs: RollingNumericMetric;
  private readonly inputToSendInputMs: RollingNumericMetric;
  private readonly inputToVisibleResponseMs: RollingNumericMetric;

  constructor(private readonly capacity = 512, clockCapacity = 16) {
    this.clock = new ServerClockEstimator(clockCapacity);
    this.captureToDisplayMs = new RollingNumericMetric(capacity);
    this.inputToQueueAcknowledgementMs = new RollingNumericMetric(capacity);
    this.inputToSendInputMs = new RollingNumericMetric(capacity);
    this.inputToVisibleResponseMs = new RollingNumericMetric(capacity);
  }

  addClockSample(sample: ClockSyncSample): boolean {
    return this.clock.add(sample);
  }

  recordInput(sequence: number, occurredAtClientUnixMs: number): void {
    if (!Number.isSafeInteger(sequence) || sequence < 0 || !Number.isFinite(occurredAtClientUnixMs)) return;
    this.inputOccurredAtUnixMs.set(sequence, occurredAtClientUnixMs);
    while (this.inputOccurredAtUnixMs.size > this.capacity) {
      const oldest = this.inputOccurredAtUnixMs.keys().next().value as number | undefined;
      if (oldest === undefined) break;
      this.inputOccurredAtUnixMs.delete(oldest);
    }
  }

  recordInputQueueAcknowledged(sequence: number, acknowledgedAtClientUnixMs: number): boolean {
    if (!Number.isSafeInteger(sequence) || !Number.isFinite(acknowledgedAtClientUnixMs)) return false;
    const inputAt = this.inputOccurredAtUnixMs.get(sequence);
    if (inputAt === undefined || acknowledgedAtClientUnixMs < inputAt) return false;
    this.inputToQueueAcknowledgementMs.add(acknowledgedAtClientUnixMs - inputAt);
    return true;
  }

  addMediaTrace(point: MediaTracePoint): boolean {
    const hasVisibleInput = point.visibleInputSequence !== null
      && point.visibleInputSequence !== undefined;
    const hasAppliedTime = point.inputAppliedAtServerUnixMs !== null
      && point.inputAppliedAtServerUnixMs !== undefined;
    if (
      !Number.isSafeInteger(point.generation)
      || !Number.isSafeInteger(point.sequence)
      || !Number.isSafeInteger(point.captureSequence)
      || !Number.isFinite(point.capturedAtServerUnixMs)
      || !Number.isFinite(point.mediaTimeUs)
      || !Number.isFinite(point.durationUs)
      || point.durationUs <= 0
      || hasVisibleInput !== hasAppliedTime
      || (hasVisibleInput && (
        !Number.isSafeInteger(point.visibleInputSequence)
        || (point.visibleInputSequence as number) <= 0
      ))
      || (hasAppliedTime && !Number.isFinite(point.inputAppliedAtServerUnixMs))
    ) return false;
    const duplicate = this.mediaPoints.some(candidate => (
      candidate.generation === point.generation && candidate.sequence === point.sequence
    ));
    if (duplicate) return false;
    this.mediaPoints.push({ ...point });
    this.mediaPoints.sort((left, right) => left.mediaTimeUs - right.mediaTimeUs);
    if (this.mediaPoints.length > this.capacity) {
      this.mediaPoints.splice(0, this.mediaPoints.length - this.capacity);
    }
    return true;
  }

  recordPresented(mediaTimeSeconds: number, presentedAtClientUnixMs: number): boolean {
    if (!Number.isFinite(mediaTimeSeconds) || !Number.isFinite(presentedAtClientUnixMs)) return false;
    const mediaTimeUs = mediaTimeSeconds * 1_000_000;
    let index = -1;
    for (let candidate = this.mediaPoints.length - 1; candidate >= 0; candidate -= 1) {
      const point = this.mediaPoints[candidate];
      if (mediaTimeUs + 1 >= point.mediaTimeUs) {
        index = candidate;
        break;
      }
    }
    if (index < 0) return false;
    const point = this.mediaPoints[index];
    const capturedAtClientUnixMs = this.clock.serverToClientUnixMs(point.capturedAtServerUnixMs);
    if (capturedAtClientUnixMs === null) return false;
    const captureLatency = presentedAtClientUnixMs - capturedAtClientUnixMs;
    if (captureLatency >= 0) this.captureToDisplayMs.add(captureLatency);

    const visibleInputSequence = point.visibleInputSequence;
    if (visibleInputSequence !== null && visibleInputSequence !== undefined) {
      const inputAt = this.inputOccurredAtUnixMs.get(visibleInputSequence);
      if (inputAt !== undefined) {
        const appliedAtServerUnixMs = point.inputAppliedAtServerUnixMs;
        if (
          appliedAtServerUnixMs !== null
          && appliedAtServerUnixMs !== undefined
          && !this.measuredSendInputSequences.has(visibleInputSequence)
        ) {
          const appliedAtClientUnixMs = this.clock.serverToClientUnixMs(appliedAtServerUnixMs);
          if (appliedAtClientUnixMs !== null && appliedAtClientUnixMs >= inputAt) {
            this.inputToSendInputMs.add(appliedAtClientUnixMs - inputAt);
            this.measuredSendInputSequences.add(visibleInputSequence);
            while (this.measuredSendInputSequences.size > this.capacity) {
              const oldest = this.measuredSendInputSequences.values().next().value as number | undefined;
              if (oldest === undefined) break;
              this.measuredSendInputSequences.delete(oldest);
            }
          }
        }
        const inputLatency = presentedAtClientUnixMs - inputAt;
        if (inputLatency >= 0) this.inputToVisibleResponseMs.add(inputLatency);
        for (const sequence of this.inputOccurredAtUnixMs.keys()) {
          if (sequence <= visibleInputSequence) this.inputOccurredAtUnixMs.delete(sequence);
        }
      }
    }
    // Presentation time is monotonic; older metadata can never match a future
    // callback more accurately and is discarded to keep correlation bounded.
    this.mediaPoints.splice(0, index + 1);
    return captureLatency >= 0;
  }

  resetGeneration(generation: number): void {
    for (let index = this.mediaPoints.length - 1; index >= 0; index -= 1) {
      if (this.mediaPoints[index].generation !== generation) this.mediaPoints.splice(index, 1);
    }
  }

  snapshot(): EndToEndLatencySnapshot {
    return {
      clock: this.clock.snapshot(),
      captureToDisplayMs: this.captureToDisplayMs.snapshot() ?? emptyNumericMetric(),
      inputToQueueAcknowledgementMs: this.inputToQueueAcknowledgementMs.snapshot() ?? emptyNumericMetric(),
      inputToSendInputMs: this.inputToSendInputMs.snapshot() ?? emptyNumericMetric(),
      inputToVisibleResponseMs: this.inputToVisibleResponseMs.snapshot() ?? emptyNumericMetric(),
      pendingMediaTracePoints: this.mediaPoints.length,
      pendingInputEvents: this.inputOccurredAtUnixMs.size,
    };
  }
}

export async function sampleServerClock(
  fetcher: typeof fetch,
  nowUnixMs: () => number,
  endpoint = '/time',
): Promise<ClockSyncSample> {
  const requestStartedAtUnixMs = nowUnixMs();
  const response = await fetcher(endpoint, { cache: 'no-store', credentials: 'same-origin' });
  if (!response.ok) throw new Error(`screen-share clock sync failed: HTTP ${response.status}`);
  const body = await response.json() as { server_unix_ms?: unknown };
  const responseFinishedAtUnixMs = nowUnixMs();
  if (typeof body.server_unix_ms !== 'number' || !Number.isFinite(body.server_unix_ms)) {
    throw new Error('screen-share clock sync returned an invalid server_unix_ms');
  }
  return { requestStartedAtUnixMs, responseFinishedAtUnixMs, serverUnixMs: body.server_unix_ms };
}
