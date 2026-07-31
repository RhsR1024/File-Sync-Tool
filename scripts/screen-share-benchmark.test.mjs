import assert from 'node:assert/strict';
import test from 'node:test';

import {
  MjpegParser,
  ReceiveWindowCollector,
  counterDelta,
  evaluateBenchmarkAcceptance,
  mediaUrl,
  parseArgs,
  percentile,
  statusViewerCount,
  summarizeSamples,
  waitForViewerRecovery,
} from './screen-share-benchmark.mjs';

test('parseArgs accepts the required scenario controls and resolves output', () => {
  const parsed = parseArgs([
    '--base-url', 'http://192.0.2.10:9870/share',
    '--transport', 'mjpeg',
    '--healthy-clients', '30',
    '--slow-clients', '1',
    '--duration-seconds', '12.5',
    '--connect-timeout-seconds', '9',
    '--slow-isolation-timeout-seconds', '2.5',
    '--recovery-timeout-seconds', '4',
    '--scenario', 'fast-scroll',
    '--output', 'artifacts/result.json',
    '--require-gates',
  ]);

  assert.equal(parsed.baseUrl.toString(), 'http://192.0.2.10:9870/share');
  assert.equal(parsed.transport, 'mjpeg');
  assert.equal(parsed.healthyClients, 30);
  assert.equal(parsed.slowClients, 1);
  assert.equal(parsed.durationSeconds, 12.5);
  assert.equal(parsed.connectTimeoutSeconds, 9);
  assert.equal(parsed.slowIsolationTimeoutSeconds, 2.5);
  assert.equal(parsed.recoveryTimeoutSeconds, 4);
  assert.equal(parsed.scenario, 'fast-scroll');
  assert.match(parsed.output, /artifacts[\\/]result\.json$/);
  assert.equal(parsed.requireGates, true);
});

test('parseArgs rejects invalid transport and an empty client population', () => {
  assert.throws(
    () => parseArgs(['--transport', 'webrtc']),
    /mse-h264 or mjpeg/,
  );
  assert.throws(
    () => parseArgs(['--healthy-clients', '0', '--slow-clients', '0']),
    /at least one healthy or slow client/,
  );
  assert.throws(
    () => parseArgs(['--base-url', 'http://user:secret@127.0.0.1:9870/']),
    /must not contain credentials/,
  );
});

test('parseArgs returns help without requiring a runnable scenario', () => {
  assert.equal(parseArgs(['--help']).help, true);
});

test('media URLs obey the server deny-unknown-query contract', () => {
  const base = new URL('https://192.0.2.10:9870/view?ignored=base');
  const h264 = mediaUrl(base, 'mse-h264');
  const mjpeg = mediaUrl(base, 'mjpeg');
  assert.equal(h264.toString(), 'wss://192.0.2.10:9870/media/ws');
  assert.equal(mjpeg.toString(), 'https://192.0.2.10:9870/stream');
  assert.equal(h264.search, '');
  assert.equal(mjpeg.search, '');
});

test('percentiles use nearest-rank semantics and empty summaries stay explicit', () => {
  assert.equal(percentile([40, 10, 30, 20], 50), 20);
  assert.equal(percentile([40, 10, 30, 20], 99), 40);
  assert.deepEqual(summarizeSamples([]), {
    count: 0, p50: null, p95: null, p99: null, max: null,
  });
});

test('receive windows include idle buckets and normalize a partial final bucket', () => {
  const collector = new ReceiveWindowCollector(1000, 250, 100);
  assert.equal(collector.add(999, 99), false);
  assert.equal(collector.add(1000, 100), true);
  assert.equal(collector.add(1210, 50), true);
  assert.equal(collector.add(1250, 99), false);

  const snapshot = collector.snapshot();
  assert.equal(snapshot.window_count, 3);
  assert.equal(snapshot.total_bytes, 150);
  // 100 bytes / 100 ms = 8,000 bps; idle middle window remains in the distribution;
  // 50 bytes / 50 ms in the partial final bucket is also 8,000 bps.
  assert.deepEqual(snapshot.bitrate_bps, {
    count: 3, p50: 8000, p95: 8000, p99: 8000, max: 8000,
  });
});

test('MJPEG parser handles boundaries, headers, and JPEG payloads split across chunks', () => {
  const frames = [];
  const parser = new MjpegParser('frame', (frame) => frames.push(frame.toString('hex')));
  const stream = Buffer.from(
    '--frame\r\nContent-Type: image/jpeg\r\nContent-Length: 4\r\n\r\n' +
    '\xff\xd8\xff\xd9\r\n' +
    '--frame\r\nContent-Type: image/jpeg\r\nContent-Length: 3\r\n\r\nabc\r\n',
    'latin1',
  );
  parser.push(stream.subarray(0, 17));
  parser.push(stream.subarray(17, 61));
  parser.push(stream.subarray(61));

  assert.deepEqual(frames, ['ffd8ffd9', '616263']);
});

test('viewer recovery polls until the server returns to its pre-benchmark baseline', async () => {
  let nowMs = 0;
  const snapshots = [{ viewers: 3 }, { viewers: 1 }];
  const result = await waitForViewerRecovery(
    async () => snapshots.shift(),
    1,
    500,
    100,
    () => nowMs,
    async (milliseconds) => { nowMs += milliseconds; },
  );

  assert.equal(statusViewerCount({ viewers: 2 }), 2);
  assert.equal(statusViewerCount({ viewers: -1 }), null);
  assert.equal(result.available, true);
  assert.equal(result.recovered, true);
  assert.equal(result.elapsed_ms, 100);
  assert.equal(result.final_viewer_count, 1);
  assert.deepEqual(result.samples.map((sample) => sample.viewer_count), [3, 1]);
});

test('viewer recovery stays explicit when the status schema has no baseline count', async () => {
  const result = await waitForViewerRecovery(async () => ({ viewers: 0 }), null, 500);
  assert.equal(result.available, false);
  assert.equal(result.recovered, false);
  assert.match(result.error, /no valid viewers count/);
});

test('viewer recovery measures status latency after the request completes', async () => {
  let nowMs = 0;
  const result = await waitForViewerRecovery(
    async () => {
      nowMs += 3_100;
      return { viewers: 0 };
    },
    0,
    3_000,
    100,
    () => nowMs,
    async (milliseconds) => { nowMs += milliseconds; },
  );

  assert.equal(result.recovered, false);
  assert.equal(result.elapsed_ms, 3_100);
  assert.equal(result.samples[0].elapsed_ms, 3_100);
});

test('viewer recovery includes time already spent since slow-client fault activation', async () => {
  let nowMs = 900;
  const result = await waitForViewerRecovery(
    async () => ({ viewers: 0 }),
    0,
    2_000,
    100,
    () => nowMs,
    async (milliseconds) => { nowMs += milliseconds; },
    900,
  );

  assert.equal(result.recovered, true);
  assert.equal(result.elapsed_ms, 900);
});

test('acceptance evaluator distinguishes passing gates from missing matrix scope', () => {
  const report = {
    scenario: {
      healthy_client_count: 5,
      stopped_reading_client_count: 1,
      duration_seconds: 30,
      slow_isolation_timeout_seconds: 2,
    },
    status_before: {
      viewers: 0,
      viewer_ip_reference_count: 0,
      active_media_task_count: 0,
      media_metrics: { slow_client_dropped_frames: 4 },
    },
    status_measurement_start: { viewers: 5 },
    status_after: { viewers: 5, media_metrics: { slow_client_dropped_frames: 5 } },
    healthy_clients: {
      unexpected_disconnect_count: 0,
      clients_without_measurement_frames: [],
    },
    slow_clients: [{}],
    slow_client_isolation: { available: true, recovered: true, elapsed_ms: 900 },
    status_recovery: {
      available: true,
      recovered: true,
      elapsed_ms: 200,
      status: { viewer_ip_reference_count: 0, active_media_task_count: 0 },
    },
  };

  assert.equal(counterDelta(report.status_before, report.status_after, [
    'media_metrics', 'slow_client_dropped_frames',
  ]), 1);
  const acceptance = evaluateBenchmarkAcceptance(report);
  assert.equal(acceptance.scope, 'fanout_subset');
  assert.equal(acceptance.fanout_subset_overall, 'inconclusive');
  assert.equal(
    acceptance.checks.find((check) => check.id === 'slow_client_isolated_within_threshold').status,
    'pass',
  );
  assert.equal(
    acceptance.checks.find((check) => check.id === 'm1_thirty_client_thirty_minute_scope').status,
    'inconclusive',
  );
});

test('acceptance evaluator fails a missed viewer recovery deadline', () => {
  const acceptance = evaluateBenchmarkAcceptance({
    scenario: {
      healthy_client_count: 30,
      stopped_reading_client_count: 0,
      duration_seconds: 1_800,
    },
    status_before: {
      viewers: 0,
      viewer_ip_reference_count: 0,
      active_media_task_count: 0,
      media_metrics: { slow_client_dropped_frames: 0 },
    },
    status_measurement_start: { viewers: 30 },
    status_after: { viewers: 30, media_metrics: { slow_client_dropped_frames: 0 } },
    healthy_clients: {
      unexpected_disconnect_count: 0,
      clients_without_measurement_frames: [],
    },
    slow_clients: [],
    status_recovery: {
      available: true,
      recovered: true,
      elapsed_ms: 3_001,
      status: { viewer_ip_reference_count: 0, active_media_task_count: 0 },
    },
  });
  assert.equal(acceptance.fanout_subset_overall, 'fail');
});
