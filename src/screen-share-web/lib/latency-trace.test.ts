import { describe, expect, it, vi } from 'vitest';

import {
  EndToEndLatencyTrace,
  ServerClockEstimator,
  monotonicUnixNow,
  sampleServerClock,
} from './latency-trace';

describe('screen-share end-to-end latency trace', () => {
  it('uses the minimum-RTT clock sample and exposes its uncertainty', () => {
    const clock = new ServerClockEstimator();
    clock.add({ requestStartedAtUnixMs: 1000, responseFinishedAtUnixMs: 1040, serverUnixMs: 1030 });
    clock.add({ requestStartedAtUnixMs: 2000, responseFinishedAtUnixMs: 2010, serverUnixMs: 2020 });
    expect(clock.snapshot()).toEqual({
      sampleCount: 2,
      offsetMs: 15,
      roundTripMs: 10,
      offsetRangeMs: 5,
      lastOffsetMs: 15,
      clockDiscontinuityCount: 0,
    });
    expect(clock.serverToClientUnixMs(2030)).toBe(2015);
  });

  it('starts a new calibration epoch after a wall-clock discontinuity', () => {
    const clock = new ServerClockEstimator();
    clock.add({ requestStartedAtUnixMs: 1000, responseFinishedAtUnixMs: 1010, serverUnixMs: 1015 });
    clock.add({ requestStartedAtUnixMs: 2000, responseFinishedAtUnixMs: 2010, serverUnixMs: 2415 });
    expect(clock.snapshot()).toMatchObject({
      sampleCount: 1,
      offsetMs: 410,
      offsetRangeMs: 0,
      clockDiscontinuityCount: 1,
    });
  });

  it('maps performance time to a monotonic Unix-compatible value', () => {
    const now = monotonicUnixNow();
    expect(Number.isFinite(now)).toBe(true);
    expect(now).toBeGreaterThan(0);
  });

  it('correlates capture and visible input sequences with a presented media time', () => {
    const trace = new EndToEndLatencyTrace(8);
    trace.addClockSample({
      requestStartedAtUnixMs: 1000,
      responseFinishedAtUnixMs: 1010,
      serverUnixMs: 1015,
    }); // server is 10ms ahead of client
    trace.recordInput(7, 1100);
    expect(trace.recordInputQueueAcknowledged(7, 1135)).toBe(true);
    expect(trace.addMediaTrace({
      generation: 2,
      sequence: 11,
      captureSequence: 99,
      capturedAtServerUnixMs: 1120,
      mediaTimeUs: 500_000,
      durationUs: 33_333,
      visibleInputSequence: 7,
      inputAppliedAtServerUnixMs: 1130,
    })).toBe(true);

    expect(trace.recordPresented(0.5, 1200)).toBe(true);
    expect(trace.snapshot()).toMatchObject({
      captureToDisplayMs: { sampleCount: 1, last: 90 },
      inputToQueueAcknowledgementMs: { sampleCount: 1, last: 35 },
      inputToSendInputMs: { sampleCount: 1, last: 20 },
      inputToVisibleResponseMs: { sampleCount: 1, last: 100 },
      pendingMediaTracePoints: 0,
      pendingInputEvents: 0,
    });
  });

  it('rejects acknowledgement samples without a matching sent input', () => {
    const trace = new EndToEndLatencyTrace(2);
    expect(trace.recordInputQueueAcknowledged(9, 100)).toBe(false);
    trace.recordInput(9, 110);
    expect(trace.recordInputQueueAcknowledged(9, 100)).toBe(false);
    expect(trace.snapshot().inputToQueueAcknowledgementMs.sampleCount).toBe(0);
  });

  it('samples successful SendInput once even when later frames repeat its sequence', () => {
    const trace = new EndToEndLatencyTrace(8);
    trace.addClockSample({
      requestStartedAtUnixMs: 1000,
      responseFinishedAtUnixMs: 1010,
      serverUnixMs: 1015,
    });
    trace.recordInput(5, 1100);
    for (const sequence of [20, 21]) {
      trace.addMediaTrace({
        generation: 1,
        sequence,
        captureSequence: sequence,
        capturedAtServerUnixMs: 1160,
        mediaTimeUs: sequence * 1000,
        durationUs: 1000,
        visibleInputSequence: 5,
        inputAppliedAtServerUnixMs: 1135,
      });
      trace.recordPresented(sequence / 1000, 1200 + sequence);
    }
    expect(trace.snapshot().inputToSendInputMs).toMatchObject({ sampleCount: 1, last: 25 });
  });

  it('bounds pending inputs and rejects duplicate media identity', () => {
    const trace = new EndToEndLatencyTrace(2);
    trace.recordInput(1, 1);
    trace.recordInput(2, 2);
    trace.recordInput(3, 3);
    const point = {
      generation: 1,
      sequence: 1,
      captureSequence: 1,
      capturedAtServerUnixMs: 1,
      mediaTimeUs: 1,
      durationUs: 1,
    };
    expect(trace.addMediaTrace(point)).toBe(true);
    expect(trace.addMediaTrace(point)).toBe(false);
    expect(trace.snapshot()).toMatchObject({ pendingInputEvents: 2, pendingMediaTracePoints: 1 });
  });

  it('samples the server clock without accepting malformed payloads', async () => {
    const now = vi.fn()
      .mockReturnValueOnce(1000)
      .mockReturnValueOnce(1012);
    const fetcher = vi.fn().mockResolvedValue(new Response(
      JSON.stringify({ server_unix_ms: 1010 }),
      { status: 200, headers: { 'content-type': 'application/json' } },
    ));
    await expect(sampleServerClock(fetcher, now)).resolves.toEqual({
      requestStartedAtUnixMs: 1000,
      responseFinishedAtUnixMs: 1012,
      serverUnixMs: 1010,
    });
    expect(fetcher).toHaveBeenCalledWith('/time', { cache: 'no-store', credentials: 'same-origin' });
  });
});
