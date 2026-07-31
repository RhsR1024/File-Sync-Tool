#!/usr/bin/env node

import { randomBytes } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import http from 'node:http';
import https from 'node:https';
import net from 'node:net';
import os from 'node:os';
import { dirname, resolve } from 'node:path';
import { performance } from 'node:perf_hooks';
import tls from 'node:tls';
import { pathToFileURL } from 'node:url';
import WebSocket from 'ws';

const TRANSPORTS = new Set(['mse-h264', 'mjpeg']);
const AUTHENTICATION_ERROR =
  'screen sharing requires authentication; this benchmark intentionally does not accept or store credentials. Restart sharing without username/password for a controlled benchmark run.';

const HELP = `Screen-share media fan-out benchmark and slow-reader fault injector

Usage:
  node scripts/screen-share-benchmark.mjs [options]

Options:
  --base-url <url>             Screen-share origin (default: http://127.0.0.1:9870/)
  --transport <name>           mse-h264 or mjpeg (default: mse-h264)
  --healthy-clients <count>    Clients that continuously consume media (default: 1)
  --slow-clients <count>       Raw TCP clients paused immediately after handshake (default: 0)
  --duration-seconds <number>  Measurement duration after all handshakes (default: 30)
  --connect-timeout-seconds <n> Per-client handshake/first-frame timeout (default: 15)
  --slow-isolation-timeout-seconds <n> Slow-viewer isolation gate (default: 2)
  --recovery-timeout-seconds <n> Wait for viewer count to return to baseline (default: 5)
  --scenario <label>           Workload label stored in the JSON report (default: manual)
  --output <path>              Exact JSON report path (default: artifacts/...timestamp.json)
  --allow-insecure-tls         Accept a self-signed HTTPS certificate
  --require-gates              Exit nonzero unless all applicable M1 gates pass
  --help                       Show this help

Examples:
  node scripts/screen-share-benchmark.mjs --transport mse-h264 --healthy-clients 30 --duration-seconds 1800
  node scripts/screen-share-benchmark.mjs --transport mjpeg --healthy-clients 5 --slow-clients 1 --duration-seconds 30 --output artifacts/screen-share-benchmarks/mjpeg-slow.json

Authentication:
  The tool works when screen sharing has no username/password. Authenticated media
  routes require a browser-issued HttpOnly cookie, so HTTP 401 is reported explicitly
  and credentials are never accepted on the command line.
`;

function optionValue(argv, index, name) {
  const value = argv[index + 1];
  if (value === undefined || value.startsWith('--')) {
    throw new Error(`--${name} requires a value`);
  }
  return value;
}

function parseNonNegativeInteger(value, name) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0) {
    throw new Error(`--${name} must be a non-negative integer`);
  }
  return parsed;
}

function parsePositiveNumber(value, name) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new Error(`--${name} must be a positive number`);
  }
  return parsed;
}

function defaultOutputPath(transport, now = new Date()) {
  const timestamp = now.toISOString().replaceAll(':', '').replaceAll('-', '').replace(/\.\d{3}Z$/, 'Z');
  return resolve('artifacts', 'screen-share-benchmarks', `${transport}-${timestamp}.json`);
}

export function parseArgs(argv, now = new Date()) {
  const values = {
    baseUrl: 'http://127.0.0.1:9870/',
    transport: 'mse-h264',
    healthyClients: 1,
    slowClients: 0,
    durationSeconds: 30,
    connectTimeoutSeconds: 15,
    slowIsolationTimeoutSeconds: 2,
    recoveryTimeoutSeconds: 5,
    scenario: 'manual',
    output: null,
    allowInsecureTls: false,
    requireGates: false,
    help: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--help') {
      values.help = true;
      continue;
    }
    if (argument === '--allow-insecure-tls') {
      values.allowInsecureTls = true;
      continue;
    }
    if (argument === '--require-gates') {
      values.requireGates = true;
      continue;
    }
    if (!argument.startsWith('--')) throw new Error(`unexpected positional argument: ${argument}`);
    const name = argument.slice(2);
    const value = optionValue(argv, index, name);
    index += 1;
    switch (name) {
      case 'base-url': values.baseUrl = value; break;
      case 'transport': values.transport = value; break;
      case 'healthy-clients':
        values.healthyClients = parseNonNegativeInteger(value, name);
        break;
      case 'slow-clients':
        values.slowClients = parseNonNegativeInteger(value, name);
        break;
      case 'duration-seconds':
        values.durationSeconds = parsePositiveNumber(value, name);
        break;
      case 'connect-timeout-seconds':
        values.connectTimeoutSeconds = parsePositiveNumber(value, name);
        break;
      case 'slow-isolation-timeout-seconds':
        values.slowIsolationTimeoutSeconds = parsePositiveNumber(value, name);
        break;
      case 'recovery-timeout-seconds':
        values.recoveryTimeoutSeconds = parsePositiveNumber(value, name);
        break;
      case 'scenario': values.scenario = value.trim(); break;
      case 'output': values.output = resolve(value); break;
      default: throw new Error(`unknown option: --${name}`);
    }
  }

  if (values.help) return values;
  if (!TRANSPORTS.has(values.transport)) {
    throw new Error('--transport must be mse-h264 or mjpeg');
  }
  if (values.healthyClients + values.slowClients === 0) {
    throw new Error('at least one healthy or slow client is required');
  }
  if (!values.scenario) throw new Error('--scenario must not be empty');
  let baseUrl;
  try {
    baseUrl = new URL(values.baseUrl);
  } catch {
    throw new Error('--base-url must be a valid http:// or https:// URL');
  }
  if (!['http:', 'https:'].includes(baseUrl.protocol)) {
    throw new Error('--base-url must use http:// or https://');
  }
  if (baseUrl.username || baseUrl.password) {
    throw new Error(`--base-url must not contain credentials; ${AUTHENTICATION_ERROR}`);
  }
  values.baseUrl = baseUrl;
  values.output ??= defaultOutputPath(values.transport, now);
  return values;
}

export function percentile(values, percent) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.max(0, Math.ceil(sorted.length * percent / 100) - 1)];
}

function round(value, digits = 3) {
  if (value === null || value === undefined || !Number.isFinite(value)) return null;
  const scale = 10 ** digits;
  return Math.round(value * scale) / scale;
}

export function summarizeSamples(values, digits = 3) {
  if (values.length === 0) {
    return { count: 0, p50: null, p95: null, p99: null, max: null };
  }
  return {
    count: values.length,
    p50: round(percentile(values, 50), digits),
    p95: round(percentile(values, 95), digits),
    p99: round(percentile(values, 99), digits),
    max: round(Math.max(...values), digits),
  };
}

export class ReceiveWindowCollector {
  constructor(startedAtMs, durationMs, windowMs) {
    this.startedAtMs = startedAtMs;
    this.durationMs = durationMs;
    this.windowMs = windowMs;
    this.buckets = Array.from({ length: Math.ceil(durationMs / windowMs) }, () => 0);
  }

  add(observedAtMs, bytes) {
    const offset = observedAtMs - this.startedAtMs;
    if (offset < 0 || offset >= this.durationMs || bytes <= 0) return false;
    const index = Math.floor(offset / this.windowMs);
    this.buckets[index] += bytes;
    return true;
  }

  snapshot() {
    const bitrateBps = this.buckets.map((bytes, index) => {
      const windowStart = index * this.windowMs;
      const actualWindowMs = Math.min(this.windowMs, this.durationMs - windowStart);
      return bytes * 8 * 1000 / actualWindowMs;
    });
    return {
      window_ms: this.windowMs,
      window_count: this.buckets.length,
      total_bytes: this.buckets.reduce((sum, value) => sum + value, 0),
      bitrate_bps: summarizeSamples(bitrateBps, 0),
    };
  }
}

export class MjpegParser {
  constructor(boundary, onFrame) {
    this.boundary = Buffer.from(`--${boundary}`);
    this.onFrame = onFrame;
    this.buffer = Buffer.alloc(0);
  }

  push(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    while (this.buffer.length > 0) {
      const boundaryIndex = this.buffer.indexOf(this.boundary);
      if (boundaryIndex < 0) {
        const keep = Math.min(this.buffer.length, this.boundary.length - 1);
        this.buffer = this.buffer.subarray(this.buffer.length - keep);
        return;
      }
      if (boundaryIndex > 0) this.buffer = this.buffer.subarray(boundaryIndex);
      const headerStart = this.boundary.length + (
        this.buffer.subarray(this.boundary.length, this.boundary.length + 2).equals(Buffer.from('\r\n')) ? 2 : 0
      );
      const headerEnd = this.buffer.indexOf(Buffer.from('\r\n\r\n'), headerStart);
      if (headerEnd < 0) return;
      const headers = this.buffer.subarray(headerStart, headerEnd).toString('latin1');
      const match = /^content-length:\s*(\d+)\s*$/im.exec(headers);
      if (!match) throw new Error('MJPEG part is missing Content-Length');
      const contentLength = Number(match[1]);
      const contentStart = headerEnd + 4;
      const contentEnd = contentStart + contentLength;
      if (this.buffer.length < contentEnd) return;
      this.onFrame(this.buffer.subarray(contentStart, contentEnd));
      this.buffer = this.buffer.subarray(contentEnd);
      if (this.buffer.subarray(0, 2).equals(Buffer.from('\r\n'))) {
        this.buffer = this.buffer.subarray(2);
      }
    }
  }
}

function parseBoundary(contentType) {
  const match = /boundary=(?:"([^"]+)"|([^;\s]+))/i.exec(contentType ?? '');
  return match?.[1] ?? match?.[2] ?? null;
}

export function mediaUrl(baseUrl, transport) {
  const path = transport === 'mse-h264'
    ? '/media/ws'
    : '/stream';
  const url = new URL(path, baseUrl);
  if (transport !== 'mjpeg') url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  return url;
}

function authenticationAwareError(statusCode, endpoint) {
  if (statusCode === 401) return new Error(`${endpoint}: HTTP 401; ${AUTHENTICATION_ERROR}`);
  return new Error(`${endpoint}: unexpected HTTP ${statusCode}`);
}

class HealthyClientBase {
  constructor(index, transport, collector100ms, collector1s) {
    this.index = index;
    this.transport = transport;
    this.collector100ms = collector100ms;
    this.collector1s = collector1s;
    this.connectStartedAt = performance.now();
    this.connectedAt = null;
    this.firstFrameAt = null;
    this.measurementStartedAt = null;
    this.rawBytes = 0;
    this.mediaBytes = 0;
    this.mediaFrames = 0;
    this.lastMediaAt = null;
    this.interArrivalMs = [];
    this.manuallyClosed = false;
    this.closeInfo = null;
  }

  recordRawBytes(bytes, observedAt = performance.now()) {
    if (this.measurementStartedAt === null) return;
    this.rawBytes += bytes;
    this.collector100ms.add(observedAt, bytes);
    this.collector1s.add(observedAt, bytes);
  }

  recordMediaFrame(bytes, observedAt = performance.now()) {
    this.firstFrameAt ??= observedAt;
    if (this.measurementStartedAt === null) return;
    this.mediaBytes += bytes;
    this.mediaFrames += 1;
    if (this.lastMediaAt !== null) this.interArrivalMs.push(observedAt - this.lastMediaAt);
    this.lastMediaAt = observedAt;
  }

  startMeasurement(startedAt) {
    this.measurementStartedAt = startedAt;
    this.rawBytes = 0;
    this.mediaBytes = 0;
    this.mediaFrames = 0;
    this.lastMediaAt = null;
    this.interArrivalMs = [];
  }

  snapshot(measurementEndedAt) {
    return {
      client_index: this.index,
      connected: this.connectedAt !== null,
      connection_ms: this.connectedAt === null ? null : round(this.connectedAt - this.connectStartedAt),
      first_frame_ms: this.firstFrameAt === null ? null : round(this.firstFrameAt - this.connectStartedAt),
      measurement_bytes: this.rawBytes,
      measurement_media_bytes: this.mediaBytes,
      measurement_media_frames: this.mediaFrames,
      media_inter_arrival_ms: summarizeSamples(this.interArrivalMs),
      disconnected_before_measurement: this.closeInfo !== null
        && this.measurementStartedAt !== null
        && this.closeInfo.observedAt < this.measurementStartedAt,
      disconnected_during_measurement: this.closeInfo !== null
        && this.measurementStartedAt !== null
        && this.closeInfo.observedAt >= this.measurementStartedAt
        && this.closeInfo.observedAt <= measurementEndedAt,
      disconnect: this.closeInfo === null ? null : {
        after_connect_ms: round(this.closeInfo.observedAt - this.connectStartedAt),
        code: this.closeInfo.code,
        reason: this.closeInfo.reason,
        error: this.closeInfo.error,
        manual: this.closeInfo.manual,
      },
    };
  }
}

class HealthyH264Client extends HealthyClientBase {
  constructor(options, index, collector100ms, collector1s) {
    super(index, options.transport, collector100ms, collector1s);
    const url = mediaUrl(options.baseUrl, options.transport);
    this.socket = new WebSocket(url, {
      handshakeTimeout: options.connectTimeoutMs,
      perMessageDeflate: false,
      rejectUnauthorized: !options.allowInsecureTls,
      headers: { 'User-Agent': `screen-share-benchmark-healthy/${index}` },
    });
    this.expectingInit = false;
    this.ready = new Promise((resolveReady, rejectReady) => {
      let settled = false;
      const timer = setTimeout(() => {
        if (settled) return;
        settled = true;
        rejectReady(new Error(`healthy H.264 client ${index} timed out waiting for its first media segment`));
        this.socket.terminate();
      }, options.connectTimeoutMs);
      this.socket.once('open', () => { this.connectedAt = performance.now(); });
      this.socket.on('message', (data, isBinary) => {
        const observedAt = performance.now();
        const bytes = typeof data === 'string' ? Buffer.byteLength(data) : data.byteLength;
        this.recordRawBytes(bytes, observedAt);
        if (!isBinary) {
          try {
            const message = JSON.parse(data.toString());
            if (message?.type === 'media.hello') {
              // MSE sends a separate binary init segment after each hello.
              this.expectingInit = true;
            }
          } catch {
            // Non-protocol text is still accounted as received bytes.
          }
          return;
        }
        if (this.expectingInit) {
          this.expectingInit = false;
          return;
        }
        this.recordMediaFrame(bytes, observedAt);
        if (!settled) {
          settled = true;
          clearTimeout(timer);
          resolveReady();
        }
      });
      this.socket.once('unexpected-response', (_request, response) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        rejectReady(authenticationAwareError(response.statusCode ?? 0, url.pathname));
      });
      this.socket.once('error', (error) => {
        if (!settled) {
          settled = true;
          clearTimeout(timer);
          rejectReady(new Error(`healthy H.264 client ${index}: ${error.message}`));
        }
        this.closeInfo ??= {
          observedAt: performance.now(), code: null, reason: null, error: error.message, manual: this.manuallyClosed,
        };
      });
      this.socket.once('close', (code, reason) => {
        clearTimeout(timer);
        this.closeInfo ??= {
          observedAt: performance.now(), code, reason: reason.toString(), error: null, manual: this.manuallyClosed,
        };
        if (!settled) {
          settled = true;
          rejectReady(new Error(`healthy H.264 client ${index} closed before its first media segment`));
        }
      });
    });
  }

  close() {
    this.manuallyClosed = true;
    if (this.socket.readyState === WebSocket.OPEN) this.socket.close(1000, 'benchmark complete');
    else if (this.socket.readyState !== WebSocket.CLOSED) this.socket.terminate();
  }
}

class HealthyMjpegClient extends HealthyClientBase {
  constructor(options, index, collector100ms, collector1s) {
    super(index, 'mjpeg', collector100ms, collector1s);
    const url = mediaUrl(options.baseUrl, 'mjpeg');
    const transport = url.protocol === 'https:' ? https : http;
    this.request = transport.get(url, {
      rejectUnauthorized: !options.allowInsecureTls,
      headers: {
        Accept: 'multipart/x-mixed-replace',
        'User-Agent': `screen-share-benchmark-healthy/${index}`,
      },
      timeout: options.connectTimeoutMs,
    });
    this.response = null;
    this.ready = new Promise((resolveReady, rejectReady) => {
      let settled = false;
      const rejectOnce = (error) => {
        if (settled) return;
        settled = true;
        rejectReady(error);
      };
      this.request.once('response', (response) => {
        this.response = response;
        this.connectedAt = performance.now();
        if (response.statusCode !== 200) {
          response.resume();
          rejectOnce(authenticationAwareError(response.statusCode ?? 0, url.pathname));
          return;
        }
        const boundary = parseBoundary(response.headers['content-type']);
        if (!boundary) {
          response.resume();
          rejectOnce(new Error(`healthy MJPEG client ${index}: response has no multipart boundary`));
          return;
        }
        const parser = new MjpegParser(boundary, (frame) => {
          const observedAt = performance.now();
          this.recordMediaFrame(frame.length, observedAt);
          if (!settled) {
            settled = true;
            this.request.setTimeout(0);
            this.request.socket?.setTimeout(0);
            resolveReady();
          }
        });
        response.on('data', (chunk) => {
          this.recordRawBytes(chunk.length);
          try {
            parser.push(chunk);
          } catch (error) {
            rejectOnce(error instanceof Error ? error : new Error(String(error)));
            this.request.destroy();
          }
        });
        response.once('error', (error) => {
          rejectOnce(new Error(`healthy MJPEG client ${index}: ${error.message}`));
          this.closeInfo ??= {
            observedAt: performance.now(), code: null, reason: null, error: error.message, manual: this.manuallyClosed,
          };
        });
        response.once('close', () => {
          this.closeInfo ??= {
            observedAt: performance.now(), code: null, reason: 'HTTP response closed', error: null, manual: this.manuallyClosed,
          };
          rejectOnce(new Error(`healthy MJPEG client ${index} closed before its first frame`));
        });
      });
      this.request.once('timeout', () => {
        rejectOnce(new Error(`healthy MJPEG client ${index} timed out waiting for its first frame`));
        this.request.destroy();
      });
      this.request.once('error', (error) => {
        rejectOnce(new Error(`healthy MJPEG client ${index}: ${error.message}`));
        this.closeInfo ??= {
          observedAt: performance.now(), code: null, reason: null, error: error.message, manual: this.manuallyClosed,
        };
      });
    });
  }

  close() {
    this.manuallyClosed = true;
    this.response?.destroy();
    this.request.destroy();
  }
}

function buildRawRequest(url, transport, clientIndex) {
  const host = url.port ? `${url.hostname}:${url.port}` : url.hostname;
  if (transport !== 'mjpeg') {
    const key = randomBytes(16).toString('base64');
    return [
      `GET ${url.pathname}${url.search} HTTP/1.1`,
      `Host: ${host}`,
      'Upgrade: websocket',
      'Connection: Upgrade',
      `Sec-WebSocket-Key: ${key}`,
      'Sec-WebSocket-Version: 13',
      `User-Agent: screen-share-benchmark-slow/${clientIndex}`,
      '', '',
    ].join('\r\n');
  }
  return [
    `GET ${url.pathname}${url.search} HTTP/1.1`,
    `Host: ${host}`,
    'Accept: multipart/x-mixed-replace',
    'Connection: keep-alive',
    `User-Agent: screen-share-benchmark-slow/${clientIndex}`,
    '', '',
  ].join('\r\n');
}

class SlowRawClient {
  constructor(options, index) {
    this.index = index;
    this.transport = options.transport;
    this.connectStartedAt = performance.now();
    this.handshakeAt = null;
    this.measurementStartedAt = null;
    this.manuallyClosed = false;
    this.closeInfo = null;
    const media = mediaUrl(options.baseUrl, options.transport);
    const rawUrl = new URL(media);
    rawUrl.protocol = media.protocol === 'wss:' ? 'https:' : media.protocol === 'ws:' ? 'http:' : media.protocol;
    const port = Number(rawUrl.port || (rawUrl.protocol === 'https:' ? 443 : 80));
    const connectOptions = { host: rawUrl.hostname, port };
    this.socket = rawUrl.protocol === 'https:'
      ? tls.connect({ ...connectOptions, servername: rawUrl.hostname, rejectUnauthorized: !options.allowInsecureTls })
      : net.createConnection(connectOptions);
    this.ready = new Promise((resolveReady, rejectReady) => {
      let settled = false;
      let response = Buffer.alloc(0);
      const timer = setTimeout(() => {
        if (settled) return;
        settled = true;
        rejectReady(new Error(`slow ${options.transport} client ${index} handshake timed out`));
        this.socket.destroy();
      }, options.connectTimeoutMs);
      const connectedEvent = rawUrl.protocol === 'https:' ? 'secureConnect' : 'connect';
      this.socket.once(connectedEvent, () => {
        this.socket.write(buildRawRequest(rawUrl, options.transport, index));
      });
      const onData = (chunk) => {
        response = Buffer.concat([response, chunk]);
        const headerEnd = response.indexOf(Buffer.from('\r\n\r\n'));
        if (headerEnd < 0) {
          if (response.length > 64 * 1024) {
            rejectReady(new Error(`slow ${options.transport} client ${index} received an oversized HTTP header`));
            this.socket.destroy();
          }
          return;
        }
        const statusLine = response.subarray(0, response.indexOf(Buffer.from('\r\n'))).toString('latin1');
        const statusCode = Number(/^HTTP\/1\.[01]\s+(\d{3})/.exec(statusLine)?.[1] ?? 0);
        const expected = options.transport !== 'mjpeg' ? 101 : 200;
        if (statusCode !== expected) {
          settled = true;
          clearTimeout(timer);
          rejectReady(authenticationAwareError(statusCode, rawUrl.pathname));
          this.socket.destroy();
          return;
        }
        // Deliberately leave the TCP connection open but stop consuming bytes. This
        // eventually closes the receive window and applies real server-side backpressure.
        this.socket.removeListener('data', onData);
        this.socket.pause();
        this.handshakeAt = performance.now();
        settled = true;
        clearTimeout(timer);
        resolveReady();
      };
      this.socket.on('data', onData);
      this.socket.once('error', (error) => {
        this.closeInfo ??= {
          observedAt: performance.now(), error: error.message, manual: this.manuallyClosed,
        };
        if (!settled) {
          settled = true;
          clearTimeout(timer);
          rejectReady(new Error(`slow ${options.transport} client ${index}: ${error.message}`));
        }
      });
      this.socket.once('close', () => {
        this.closeInfo ??= { observedAt: performance.now(), error: null, manual: this.manuallyClosed };
        if (!settled) {
          settled = true;
          clearTimeout(timer);
          rejectReady(new Error(`slow ${options.transport} client ${index} closed during handshake`));
        }
      });
    });
  }

  startMeasurement(startedAt) {
    this.measurementStartedAt = startedAt;
  }

  snapshot(measurementEndedAt) {
    return {
      client_index: this.index,
      raw_tcp_read_paused: this.handshakeAt !== null,
      handshake_ms: this.handshakeAt === null ? null : round(this.handshakeAt - this.connectStartedAt),
      disconnected_before_measurement: this.closeInfo !== null
        && this.measurementStartedAt !== null
        && this.closeInfo.observedAt < this.measurementStartedAt,
      disconnected_during_measurement: this.closeInfo !== null
        && this.measurementStartedAt !== null
        && this.closeInfo.observedAt >= this.measurementStartedAt
        && this.closeInfo.observedAt <= measurementEndedAt,
      disconnect_after_measurement_start_ms: this.closeInfo === null || this.measurementStartedAt === null
        ? null
        : round(this.closeInfo.observedAt - this.measurementStartedAt),
      disconnect_error: this.closeInfo?.error ?? null,
      disconnect_manual: this.closeInfo?.manual ?? false,
    };
  }

  close() {
    this.manuallyClosed = true;
    this.socket.destroy();
  }
}

function aggregateHealthyClients(clients, endedAt) {
  const snapshots = clients.map((client) => client.snapshot(endedAt));
  return {
    clients: snapshots,
    connection_ms: summarizeSamples(snapshots.map((entry) => entry.connection_ms).filter(Number.isFinite)),
    first_frame_ms: summarizeSamples(snapshots.map((entry) => entry.first_frame_ms).filter(Number.isFinite)),
    total_measurement_bytes: snapshots.reduce((sum, entry) => sum + entry.measurement_bytes, 0),
    total_media_frames: snapshots.reduce((sum, entry) => sum + entry.measurement_media_frames, 0),
    unexpected_disconnect_count: snapshots.filter((entry) => (
      (entry.disconnected_before_measurement || entry.disconnected_during_measurement)
      && !entry.disconnect?.manual
    )).length,
    clients_without_measurement_frames: snapshots
      .filter((entry) => entry.measurement_media_frames <= 0)
      .map((entry) => entry.client_index),
  };
}

async function fetchStatus(baseUrl, allowInsecureTls, timeoutMs = 10_000) {
  const url = new URL('/status', baseUrl);
  const transport = url.protocol === 'https:' ? https : http;
  const body = await new Promise((resolveBody, rejectBody) => {
    const request = transport.get(url, {
      rejectUnauthorized: !allowInsecureTls,
      headers: { Accept: 'application/json', 'Cache-Control': 'no-store' },
    }, (response) => {
      const chunks = [];
      response.on('data', (chunk) => chunks.push(chunk));
      response.once('end', () => {
        if (response.statusCode !== 200) {
          rejectBody(authenticationAwareError(response.statusCode ?? 0, '/status'));
          return;
        }
        try {
          resolveBody(JSON.parse(Buffer.concat(chunks).toString('utf8')));
        } catch (error) {
          rejectBody(new Error(`/status returned invalid JSON: ${error.message}`));
        }
      });
    });
    request.setTimeout(timeoutMs, () => request.destroy(new Error('/status request timed out')));
    request.once('error', rejectBody);
  });
  if (!body.active) throw new Error('screen sharing is not active');
  return body;
}

function machineMetadata() {
  const cpus = os.cpus();
  return {
    hostname: os.hostname(),
    platform: os.platform(),
    release: os.release(),
    architecture: os.arch(),
    cpu_model: cpus[0]?.model ?? null,
    logical_cpu_count: cpus.length,
    total_memory_bytes: os.totalmem(),
    node_version: process.version,
  };
}

function delay(milliseconds) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

export function statusViewerCount(status) {
  const value = Number(status?.viewers);
  return Number.isSafeInteger(value) && value >= 0 ? value : null;
}

export async function waitForViewerRecovery(
  fetchStatusSnapshot,
  baselineViewerCount,
  timeoutMs,
  pollIntervalMs = 100,
  now = () => performance.now(),
  sleep = delay,
  elapsedOffsetMs = 0,
) {
  if (!Number.isSafeInteger(baselineViewerCount) || baselineViewerCount < 0) {
    return {
      available: false,
      recovered: false,
      elapsed_ms: null,
      baseline_viewer_count: baselineViewerCount ?? null,
      final_viewer_count: null,
      samples: [],
      error: 'baseline /status response has no valid viewers count',
    };
  }
  const startedAt = now() - Math.max(0, elapsedOffsetMs);
  const samples = [];
  let lastError = null;
  let finalStatus = null;
  for (;;) {
    try {
      finalStatus = await fetchStatusSnapshot();
      const elapsedMs = Math.max(0, now() - startedAt);
      const viewerCount = statusViewerCount(finalStatus);
      samples.push({ elapsed_ms: round(elapsedMs), viewer_count: viewerCount, error: null });
      if (viewerCount !== null && viewerCount <= baselineViewerCount && elapsedMs <= timeoutMs) {
        return {
          available: true,
          recovered: true,
          elapsed_ms: round(elapsedMs),
          baseline_viewer_count: baselineViewerCount,
          final_viewer_count: viewerCount,
          samples,
          error: null,
          status: finalStatus,
        };
      }
    } catch (error) {
      const elapsedMs = Math.max(0, now() - startedAt);
      lastError = error instanceof Error ? error.message : String(error);
      samples.push({ elapsed_ms: round(elapsedMs), viewer_count: null, error: lastError });
    }
    const elapsedMs = Math.max(0, now() - startedAt);
    if (elapsedMs >= timeoutMs) break;
    await sleep(Math.min(pollIntervalMs, Math.max(0, timeoutMs - elapsedMs)));
  }
  return {
    available: finalStatus !== null,
    recovered: false,
    elapsed_ms: round(Math.max(0, now() - startedAt)),
    baseline_viewer_count: baselineViewerCount,
    final_viewer_count: statusViewerCount(finalStatus),
    samples,
    error: lastError,
    status: finalStatus,
  };
}

function nestedNumber(value, path) {
  let current = value;
  for (const key of path) current = current?.[key];
  return typeof current === 'number' && Number.isFinite(current) ? current : null;
}

export function counterDelta(before, after, path) {
  const start = nestedNumber(before, path);
  const end = nestedNumber(after, path);
  if (start === null || end === null || end < start) return null;
  return end - start;
}

export function evaluateBenchmarkAcceptance(report) {
  const checks = [];
  const add = (id, status, observed, expected, required = true) => {
    checks.push({ id, status, required, observed, expected });
  };
  const healthy = report.healthy_clients;
  add(
    'healthy_clients_no_unexpected_disconnects',
    healthy && Number.isSafeInteger(healthy.unexpected_disconnect_count)
      ? (healthy.unexpected_disconnect_count === 0 ? 'pass' : 'fail')
      : 'inconclusive',
    healthy?.unexpected_disconnect_count ?? null,
    0,
  );
  add(
    'every_healthy_client_received_media_during_measurement',
    !healthy || !Array.isArray(healthy.clients_without_measurement_frames)
      ? 'inconclusive'
      : healthy.clients_without_measurement_frames.length === 0 ? 'pass' : 'fail',
    healthy?.clients_without_measurement_frames ?? null,
    [],
  );

  const baselineViewers = statusViewerCount(report.status_before);
  const expectedHealthyViewers = baselineViewers === null
    ? null
    : baselineViewers + Number(report.scenario?.healthy_client_count ?? 0);
  for (const [id, status] of [
    ['healthy_viewers_present_at_measurement_start', report.status_measurement_start],
    ['healthy_viewers_present_at_measurement_end', report.status_after],
  ]) {
    const observed = statusViewerCount(status);
    add(
      id,
      expectedHealthyViewers === null || observed === null
        ? 'inconclusive'
        : observed >= expectedHealthyViewers ? 'pass' : 'fail',
      observed,
      expectedHealthyViewers === null ? null : `>= ${expectedHealthyViewers}`,
    );
  }

  const slowCount = report.scenario?.stopped_reading_client_count ?? report.slow_clients?.length ?? 0;
  if (slowCount === 0) {
    add('slow_client_isolated_within_threshold', 'not_applicable', null, null, false);
  } else {
    const isolation = report.slow_client_isolation;
    const thresholdMs = Number(report.scenario?.slow_isolation_timeout_seconds) * 1000;
    const conclusive = isolation?.available === true && Number.isFinite(isolation?.elapsed_ms);
    add(
      'slow_client_isolated_within_threshold',
      !conclusive ? 'inconclusive' : isolation.recovered && isolation.elapsed_ms <= thresholdMs ? 'pass' : 'fail',
      isolation?.elapsed_ms ?? null,
      thresholdMs,
    );
  }

  const recovery = report.status_recovery;
  add(
    'viewer_count_returns_to_baseline_within_3s',
    recovery?.available !== true || !Number.isFinite(recovery?.elapsed_ms)
      ? 'inconclusive'
      : recovery.recovered && recovery.elapsed_ms <= 3_000 ? 'pass' : 'fail',
    recovery?.elapsed_ms ?? null,
    3_000,
  );

  for (const [id, field] of [
    ['viewer_ip_references_return_to_baseline_within_3s', 'viewer_ip_reference_count'],
    ['active_media_tasks_return_to_baseline_within_3s', 'active_media_task_count'],
  ]) {
    const baseline = nestedNumber(report.status_before, [field]);
    const final = nestedNumber(recovery?.status, [field]);
    add(
      id,
      baseline === null || final === null || !Number.isFinite(recovery?.elapsed_ms)
        ? 'inconclusive'
        : recovery.elapsed_ms <= 3_000 && final <= baseline ? 'pass' : 'fail',
      { elapsed_ms: recovery?.elapsed_ms ?? null, baseline, final },
      { elapsed_ms: '<= 3000', final: baseline },
    );
  }

  const laggedDelta = counterDelta(
    report.status_before,
    report.status_after,
    ['media_metrics', 'slow_client_dropped_frames'],
  );
  if (slowCount > 0) {
    add('healthy_steady_state_lagged_frames', 'not_applicable', laggedDelta, 0, false);
  } else {
    add(
      'healthy_steady_state_lagged_frames',
      laggedDelta === null ? 'inconclusive' : laggedDelta === 0 ? 'pass' : 'fail',
      laggedDelta,
      0,
    );
  }

  const healthyCount = report.scenario?.healthy_client_count ?? 0;
  const durationSeconds = report.scenario?.duration_seconds ?? 0;
  add(
    'm1_thirty_client_thirty_minute_scope',
    healthyCount >= 30 && durationSeconds >= 1_800 ? 'pass' : 'inconclusive',
    { healthy_client_count: healthyCount, duration_seconds: durationSeconds },
    { healthy_client_count: 30, duration_seconds: 1_800 },
  );

  const required = checks.filter((check) => check.required);
  const fanoutSubsetOverall = required.some((check) => check.status === 'fail')
    ? 'fail'
    : required.some((check) => check.status === 'inconclusive') ? 'inconclusive' : 'pass';
  return {
    scope: 'fanout_subset',
    fanout_subset_overall: fanoutSubsetOverall,
    checks,
    missing_m1_gates: [
      'healthy client live-edge P99 and hard-seek count require browser presentation diagnostics',
      'IDR storm validation requires keyframe-request and emitted-IDR counters',
      'input receive-to-SendInput and input-to-visible distributions require generated control traffic',
      'CPU, GPU, memory, packet-loss, jitter, and independent-device behavior require target-host tooling',
    ],
  };
}

export async function runBenchmark(options) {
  const durationMs = options.durationSeconds * 1000;
  const initialClock = performance.now();
  const collector100ms = new ReceiveWindowCollector(initialClock, durationMs, 100);
  const collector1s = new ReceiveWindowCollector(initialClock, durationMs, 1000);
  const healthy = [];
  const slow = [];
  let statusBefore = null;
  let statusMeasurementStart = null;
  let statusAfter = null;
  let slowClientIsolation = null;
  let clientsClosed = false;
  let measurementStartedAt = null;
  let measurementEndedAt = null;
  try {
    statusBefore = await fetchStatus(options.baseUrl, options.allowInsecureTls);
    if (options.transport !== 'mjpeg' && !statusBefore.h264_media?.ready) {
      throw new Error('H.264 media is not ready; start sharing with an H.264-capable transport first');
    }
    for (let index = 0; index < options.healthyClients; index += 1) {
      const client = options.transport !== 'mjpeg'
        ? new HealthyH264Client(options, index, collector100ms, collector1s)
        : new HealthyMjpegClient(options, index, collector100ms, collector1s);
      healthy.push(client);
    }
    await Promise.all(healthy.map((client) => client.ready));

    for (let index = 0; index < options.slowClients; index += 1) {
      slow.push(new SlowRawClient(options, index));
    }
    await Promise.all(slow.map((client) => client.ready));

    measurementStartedAt = performance.now();
    collector100ms.startedAtMs = measurementStartedAt;
    collector1s.startedAtMs = measurementStartedAt;
    healthy.forEach((client) => client.startMeasurement(measurementStartedAt));
    slow.forEach((client) => client.startMeasurement(measurementStartedAt));
    const baselineViewerCount = statusViewerCount(statusBefore);
    const latestSlowFaultAt = slow.reduce(
      (latest, client) => Math.max(latest, client.handshakeAt ?? measurementStartedAt),
      Number.NEGATIVE_INFINITY,
    );
    const slowIsolationPromise = slow.length === 0
      ? Promise.resolve({
        available: true,
        recovered: true,
        elapsed_ms: 0,
        baseline_viewer_count: baselineViewerCount,
        final_viewer_count: baselineViewerCount === null ? null : baselineViewerCount + healthy.length,
        samples: [],
        error: null,
        not_applicable: true,
      })
      : waitForViewerRecovery(
        () => fetchStatus(
          options.baseUrl,
          options.allowInsecureTls,
          Math.min(1_000, options.slowIsolationTimeoutMs),
        ),
        baselineViewerCount === null ? null : baselineViewerCount + healthy.length,
        options.slowIsolationTimeoutMs,
        100,
        () => performance.now(),
        delay,
        Math.max(0, measurementStartedAt - latestSlowFaultAt),
      );
    statusMeasurementStart = await fetchStatus(
      options.baseUrl,
      options.allowInsecureTls,
      Math.min(1_000, options.slowIsolationTimeoutMs),
    );
    await delay(durationMs);
    measurementEndedAt = performance.now();
    slowClientIsolation = await slowIsolationPromise;
    statusAfter = await fetchStatus(options.baseUrl, options.allowInsecureTls);

    const healthySnapshot = aggregateHealthyClients(healthy, measurementEndedAt);
    const slowSnapshot = slow.map((client) => client.snapshot(measurementEndedAt));
    healthy.forEach((client) => client.close());
    slow.forEach((client) => client.close());
    clientsClosed = true;
    const statusRecovery = await waitForViewerRecovery(
      () => fetchStatus(
        options.baseUrl,
        options.allowInsecureTls,
        Math.min(1_000, options.recoveryTimeoutMs),
      ),
      statusViewerCount(statusBefore),
      options.recoveryTimeoutMs,
    );

    const result = {
      result: 'completed',
      measurement: {
        requested_duration_ms: durationMs,
        actual_duration_ms: round(measurementEndedAt - measurementStartedAt),
      },
      status_before: statusBefore,
      status_measurement_start: statusMeasurementStart,
      status_after: statusAfter,
      healthy_clients: healthySnapshot,
      slow_clients: slowSnapshot,
      slow_client_isolation: slowClientIsolation,
      status_recovery: statusRecovery,
      outbound_receive_windows: {
        '100ms': collector100ms.snapshot(),
        '1s': collector1s.snapshot(),
      },
    };
    result.acceptance = evaluateBenchmarkAcceptance({
      ...result,
      scenario: {
        healthy_client_count: options.healthyClients,
        stopped_reading_client_count: options.slowClients,
        duration_seconds: options.durationSeconds,
        slow_isolation_timeout_seconds: options.slowIsolationTimeoutMs / 1000,
      },
    });
    return result;
  } finally {
    const endedAt = measurementEndedAt ?? performance.now();
    if (!clientsClosed) {
      healthy.forEach((client) => client.close());
      slow.forEach((client) => client.close());
    }
    await delay(50);
    if (measurementStartedAt !== null && measurementEndedAt === null) measurementEndedAt = endedAt;
  }
}

async function writeReport(outputPath, report) {
  await mkdir(dirname(outputPath), { recursive: true });
  await writeFile(outputPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
}

async function main() {
  let options;
  try {
    options = parseArgs(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`Error: ${error instanceof Error ? error.message : String(error)}\n\n${HELP}`);
    process.exitCode = 2;
    return;
  }
  if (options.help) {
    process.stdout.write(HELP);
    return;
  }

  const generatedAt = new Date();
  const endpoint = mediaUrl(options.baseUrl, options.transport);
  endpoint.search = '';
  const report = {
    schema_version: 1,
    generated_at_utc: generatedAt.toISOString(),
    scenario: {
      label: options.scenario,
      base_url: options.baseUrl.toString(),
      transport: options.transport,
      media_endpoint: endpoint.toString(),
      media_observation_scope: options.transport === 'mjpeg'
        ? 'http-wire-only'
        : 'websocket-wire-only',
      healthy_client_count: options.healthyClients,
      stopped_reading_client_count: options.slowClients,
      duration_seconds: options.durationSeconds,
      connect_timeout_seconds: options.connectTimeoutSeconds,
      slow_isolation_timeout_seconds: options.slowIsolationTimeoutSeconds,
      recovery_timeout_seconds: options.recoveryTimeoutSeconds,
      slow_client_fault: 'raw TCP/TLS connection; valid HTTP/WebSocket handshake; socket.pause() immediately after response headers',
      require_gates: options.requireGates,
    },
    machine: machineMetadata(),
    authentication: {
      credentials_supported: false,
      expectation: 'screen sharing started without username/password',
    },
    limitations: [
      'Receive timestamps are observed by this Node.js process; they are not capture-to-display timestamps.',
      'Receive byte windows count MJPEG HTTP body bytes or H.264 WebSocket message payload bytes, not TCP/IP, TLS, or WebSocket framing overhead.',
      'The tool does not decode or present video and therefore cannot report live-edge, dropped/presented frames, or visual latency.',
      'The tool does not generate remote-control input and cannot report input-to-SendInput or input-to-visible-response.',
      'CPU/GPU/process-memory measurements and packet-loss/jitter injection require separate tooling.',
      'One host process can validate fan-out behavior but does not replace the final multi-device hardware/browser matrix.',
    ],
  };

  try {
    Object.assign(report, await runBenchmark({
      ...options,
      connectTimeoutMs: options.connectTimeoutSeconds * 1000,
      slowIsolationTimeoutMs: options.slowIsolationTimeoutSeconds * 1000,
      recoveryTimeoutMs: options.recoveryTimeoutSeconds * 1000,
    }));
    if (options.requireGates && report.acceptance?.fanout_subset_overall !== 'pass') {
      process.exitCode = 1;
    }
  } catch (error) {
    report.result = 'failed';
    report.error = error instanceof Error ? error.message : String(error);
    process.exitCode = 1;
  }
  await writeReport(options.output, report);
  process.stdout.write(`${options.output}\n`);
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : null;
if (invokedPath === import.meta.url) await main();
