#!/usr/bin/env node

import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { performance } from 'node:perf_hooks';
import { isMainThread, parentPort, workerData, Worker as ThreadWorker } from 'node:worker_threads';
import WebSocket from 'ws';

function readArg(name, fallback) {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

function positiveNumber(name, fallback) {
  const value = Number(readArg(name, String(fallback)));
  if (!Number.isFinite(value) || value <= 0) throw new Error(`--${name} must be a positive number`);
  return value;
}

function positiveInteger(name, fallback, maximum = Number.MAX_SAFE_INTEGER) {
  const value = positiveNumber(name, fallback);
  if (!Number.isInteger(value) || value > maximum) {
    throw new Error(`--${name} must be an integer from 1 to ${maximum}`);
  }
  return value;
}

function nonNegativeNumber(name, fallback) {
  const value = Number(readArg(name, String(fallback)));
  if (!Number.isFinite(value) || value < 0) {
    throw new Error(`--${name} must be zero or a positive number`);
  }
  return value;
}

function viewerCounts() {
  const values = readArg('viewer-counts', '1,10,50')
    .split(',')
    .map((value) => Number(value.trim()));
  if (values.length === 0 || values.some((value) => !Number.isInteger(value) || value < 1 || value > 200)) {
    throw new Error('--viewer-counts must contain integers from 1 to 200');
  }
  return values;
}

function percentile(values, percent) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.max(0, Math.ceil(sorted.length * percent / 100) - 1);
  return sorted[index];
}

function average(values) {
  if (values.length === 0) return null;
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function round(value, digits = 3) {
  if (value === null || value === undefined || !Number.isFinite(value)) return null;
  const scale = 10 ** digits;
  return Math.round(value * scale) / scale;
}

function mediaWebSocketUrl(baseUrl, viewerIndex) {
  const url = new URL('/media/ws', baseUrl);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  url.searchParams.set('benchmark', `${Date.now()}-${viewerIndex}`);
  return url.toString();
}

async function fetchStatus(baseUrl) {
  const response = await fetch(new URL('/status', baseUrl), { cache: 'no-store' });
  if (!response.ok) throw new Error(`status request failed with HTTP ${response.status}`);
  const status = await response.json();
  if (!status.active) throw new Error('screen sharing is not active');
  if (status.transport !== 'mse_h264' || !status.h264_media?.ready) {
    throw new Error(`H.264 media is not ready (transport=${status.transport ?? 'unknown'})`);
  }
  return status;
}

function connectViewer(baseUrl, viewerIndex, timeoutMs) {
  const connectedAt = performance.now();
  const socket = new WebSocket(mediaWebSocketUrl(baseUrl, viewerIndex));
  socket.binaryType = 'arraybuffer';
  let binaryBytes = 0;
  let binaryMessages = 0;
  let firstMediaMs = null;
  let manuallyClosed = false;
  let closedUnexpectedly = false;

  const ready = new Promise((resolveReady, rejectReady) => {
    const timer = setTimeout(() => {
      rejectReady(new Error(`viewer ${viewerIndex + 1} timed out waiting for the first H.264 segment`));
      socket.close();
    }, timeoutMs);
    socket.on('message', (data, isBinary) => {
      if (!isBinary) return;
      binaryBytes += data.byteLength;
      binaryMessages += 1;
      // The first binary message is the fMP4 initialization segment. The second
      // is the first decodable media fragment.
      if (firstMediaMs === null && binaryMessages >= 2) {
        firstMediaMs = performance.now() - connectedAt;
        clearTimeout(timer);
        resolveReady();
      }
    });
    socket.once('error', () => {
      if (firstMediaMs === null) {
        clearTimeout(timer);
        rejectReady(new Error(`viewer ${viewerIndex + 1} media connection failed`));
      }
    });
    socket.once('close', () => {
      if (!manuallyClosed) closedUnexpectedly = true;
      if (firstMediaMs === null) {
        clearTimeout(timer);
        rejectReady(new Error(`viewer ${viewerIndex + 1} closed before the first H.264 segment`));
      }
    });
  });

  return {
    ready,
    reset() {
      binaryBytes = 0;
      binaryMessages = 0;
    },
    snapshot() {
      return { binaryBytes, binaryMessages, firstMediaMs, closedUnexpectedly };
    },
    close() {
      manuallyClosed = true;
      if (socket.readyState < WebSocket.CLOSING) socket.close();
    },
  };
}

async function runWorker() {
  const { baseUrl, count, offset, connectTimeoutMs } = workerData;
  const viewers = Array.from(
    { length: count },
    (_, index) => connectViewer(baseUrl, offset + index, connectTimeoutMs),
  );
  try {
    await Promise.all(viewers.map((viewer) => viewer.ready));
    parentPort.postMessage({ type: 'ready' });
    await new Promise((resolveMeasure, rejectMeasure) => {
      parentPort.once('message', async (message) => {
        if (message?.type !== 'measure') {
          rejectMeasure(new Error('invalid benchmark worker command'));
          return;
        }
        const initialSnapshots = viewers.map((viewer) => viewer.snapshot());
        viewers.forEach((viewer) => viewer.reset());
        const startedAt = performance.now();
        await new Promise((resolveDelay) => setTimeout(resolveDelay, message.durationMs));
        const elapsedSeconds = (performance.now() - startedAt) / 1000;
        const snapshots = viewers.map((viewer) => viewer.snapshot());
        parentPort.postMessage({ type: 'result', elapsedSeconds, initialSnapshots, snapshots });
        await new Promise((resolveClose, rejectClose) => {
          parentPort.once('message', (closeMessage) => {
            if (closeMessage?.type === 'close') resolveClose();
            else rejectClose(new Error('invalid benchmark worker close command'));
          });
        });
        resolveMeasure();
      });
    });
  } catch (error) {
    parentPort.postMessage({ type: 'error', error: error instanceof Error ? error.message : String(error) });
  } finally {
    viewers.forEach((viewer) => viewer.close());
    parentPort.close();
  }
}

function startWorkerGroup(data) {
  const worker = new ThreadWorker(new URL(import.meta.url), { workerData: data });
  let resolveReady;
  let rejectReady;
  let resolveResult;
  let rejectResult;
  const ready = new Promise((resolvePromise, rejectPromise) => {
    resolveReady = resolvePromise;
    rejectReady = rejectPromise;
  });
  const result = new Promise((resolvePromise, rejectPromise) => {
    resolveResult = resolvePromise;
    rejectResult = rejectPromise;
  });
  result.catch(() => undefined);
  worker.on('message', (message) => {
    if (message?.type === 'ready') resolveReady();
    if (message?.type === 'result') resolveResult(message);
    if (message?.type === 'error') {
      const error = new Error(message.error);
      rejectReady(error);
      rejectResult(error);
    }
  });
  worker.on('error', (error) => {
    rejectReady(error);
    rejectResult(error);
  });
  return {
    worker,
    ready,
    result,
    measure(durationMs) {
      worker.postMessage({ type: 'measure', durationMs });
    },
    close() {
      worker.postMessage({ type: 'close' });
    },
  };
}

async function runCase(
  baseUrl,
  count,
  durationSeconds,
  warmupSeconds,
  workerViewerLimit,
  connectBatchDelayMs,
  connectTimeoutMs,
) {
  await fetchStatus(baseUrl);
  const groups = [];
  try {
    for (let offset = 0; offset < count; offset += workerViewerLimit) {
      const group = startWorkerGroup({
        baseUrl: baseUrl.toString(),
        count: Math.min(workerViewerLimit, count - offset),
        offset,
        connectTimeoutMs,
      });
      groups.push(group);
      await group.ready;
      if (offset + workerViewerLimit < count && connectBatchDelayMs > 0) {
        await new Promise((resolveDelay) => setTimeout(resolveDelay, connectBatchDelayMs));
      }
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, warmupSeconds * 1000));
    const before = await fetchStatus(baseUrl);
    groups.forEach((group) => group.measure(durationSeconds * 1000));
    const workerResults = await Promise.all(groups.map((group) => group.result));
    const after = await fetchStatus(baseUrl);
    groups.forEach((group) => group.close());
    const snapshots = workerResults.flatMap((result) => result.snapshots);
    const initialSnapshots = workerResults.flatMap((result) => result.initialSnapshots);
    if (snapshots.some((snapshot) => snapshot.closedUnexpectedly)) {
      throw new Error('one or more media connections closed during measurement');
    }
    const elapsedSeconds = Math.max(...workerResults.map((result) => result.elapsedSeconds));
    const bytesPerViewer = snapshots.map((snapshot) => snapshot.binaryBytes);
    const mbpsPerViewer = snapshots.map((snapshot, index) => (
      snapshot.binaryBytes * 8 / workerResults[Math.floor(index / workerViewerLimit)].elapsedSeconds / 1_000_000
    ));
    const totalBinaryBytes = bytesPerViewer.reduce((sum, value) => sum + value, 0);
    const encodedBytesDelta = Math.max(
      0,
      Number(after.h264_media.encoded_bytes ?? 0) - Number(before.h264_media.encoded_bytes ?? 0),
    );
    const encodedFramesDelta = Math.max(
      0,
      Number(after.h264_media.encoded_frame_count ?? 0)
        - Number(before.h264_media.encoded_frame_count ?? 0),
    );
    const mediaMessages = snapshots.map((snapshot) => snapshot.binaryMessages);
    return {
      viewer_count: count,
      duration_seconds: round(elapsedSeconds),
      total_binary_bytes: totalBinaryBytes,
      aggregate_mbps: round(totalBinaryBytes * 8 / elapsedSeconds / 1_000_000),
      per_viewer_mbps_avg: round(average(mbpsPerViewer)),
      per_viewer_mbps_p50: round(percentile(mbpsPerViewer, 50)),
      per_viewer_mbps_p95: round(percentile(mbpsPerViewer, 95)),
      media_messages_avg: round(average(mediaMessages), 1),
      media_messages_min: Math.min(...mediaMessages),
      media_messages_p95: round(percentile(mediaMessages, 95), 1),
      media_messages_max: Math.max(...mediaMessages),
      first_media_avg_ms: round(average(initialSnapshots.map((snapshot) => snapshot.firstMediaMs))),
      first_media_p95_ms: round(percentile(initialSnapshots.map((snapshot) => snapshot.firstMediaMs), 95)),
      initial_binary_bytes_avg: round(average(initialSnapshots.map((snapshot) => snapshot.binaryBytes)), 0),
      initial_binary_bytes_p95: round(percentile(initialSnapshots.map((snapshot) => snapshot.binaryBytes), 95), 0),
      initial_binary_messages_avg: round(average(initialSnapshots.map((snapshot) => snapshot.binaryMessages)), 1),
      initial_binary_messages_p95: round(percentile(initialSnapshots.map((snapshot) => snapshot.binaryMessages), 95), 1),
      source_encoded_mbps: round(encodedBytesDelta * 8 / elapsedSeconds / 1_000_000),
      encoded_frame_count_delta: encodedFramesDelta,
      viewer_count_start: Number(before.viewers ?? 0),
      viewer_count_end: Number(after.viewers ?? 0),
      keyframe_count_delta: Math.max(
        0,
        Number(after.h264_media.keyframe_count ?? 0) - Number(before.h264_media.keyframe_count ?? 0),
      ),
      cached_segment_count_end: Number(after.h264_media.cached_segment_count ?? 0),
      cached_bytes_end: Number(after.h264_media.cached_bytes ?? 0),
      dropped_input_frames_delta: Math.max(
        0,
        Number(after.h264_media.dropped_input_frames ?? 0)
          - Number(before.h264_media.dropped_input_frames ?? 0),
      ),
      slow_client_dropped_frames_delta: Math.max(
        0,
        Number(after.media_metrics?.slow_client_dropped_frames ?? 0)
          - Number(before.media_metrics?.slow_client_dropped_frames ?? 0),
      ),
    };
  } finally {
    await Promise.allSettled(groups.map((group) => group.worker.terminate()));
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 500));
  }
}

async function main() {
  const baseUrl = new URL(readArg('base-url', 'http://127.0.0.1:9870/'));
  const counts = viewerCounts();
  const durationSeconds = positiveNumber('duration-seconds', 10);
  const warmupSeconds = positiveNumber('warmup-seconds', 2);
  const workerViewerLimit = positiveInteger('connect-batch-size', 8, 10);
  const connectBatchDelayMs = nonNegativeNumber('connect-batch-delay-ms', 250);
  const connectTimeoutMs = positiveNumber('connect-timeout-seconds', 30) * 1000;
  const outputDirectory = resolve(readArg('output-directory', 'artifacts/screen-share-benchmarks'));
  const scenario = readArg('scenario', 'current-screen');
  const cases = [];
  const failures = [];

  for (const count of counts) {
    process.stdout.write(`Running H.264 benchmark with ${count} viewer(s)...\n`);
    try {
      cases.push(await runCase(
        baseUrl,
        count,
        durationSeconds,
        warmupSeconds,
        workerViewerLimit,
        connectBatchDelayMs,
        connectTimeoutMs,
      ));
    } catch (error) {
      failures.push({ viewer_count: count, error: error instanceof Error ? error.message : String(error) });
      break;
    }
  }

  const report = {
    schema_version: 1,
    generated_at_utc: new Date().toISOString(),
    base_url: baseUrl.toString(),
    scenario,
    duration_seconds: durationSeconds,
    warmup_seconds: warmupSeconds,
    worker_viewer_limit: workerViewerLimit,
    connect_batch_delay_ms: connectBatchDelayMs,
    connect_timeout_ms: connectTimeoutMs,
    cases,
    failures,
  };
  const timestamp = report.generated_at_utc.replaceAll(':', '').replaceAll('-', '').replace(/\.\d{3}Z$/, 'Z');
  await mkdir(outputDirectory, { recursive: true });
  const outputPath = resolve(outputDirectory, `h264-${timestamp}.json`);
  await writeFile(outputPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');

  console.table(cases.map((entry) => ({
    viewers: entry.viewer_count,
    aggregate_mbps: entry.aggregate_mbps,
    per_viewer_mbps: entry.per_viewer_mbps_avg,
    first_media_p95_ms: entry.first_media_p95_ms,
    dropped_input_frames: entry.dropped_input_frames_delta,
  })));
  process.stdout.write(`${outputPath}\n`);
  if (failures.length > 0) process.exitCode = 1;
}

if (isMainThread) await main();
else await runWorker();
