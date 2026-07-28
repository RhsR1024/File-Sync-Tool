#!/usr/bin/env node

import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import http from 'node:http';
import { isIP } from 'node:net';
import { networkInterfaces, platform, tmpdir } from 'node:os';
import { dirname, join, resolve, sep } from 'node:path';
import { promisify } from 'node:util';
import { pathToFileURL } from 'node:url';

const execFileAsync = promisify(execFile);
const BROWSERS = new Set(['all', 'chrome', 'edge']);
const HELP = `Screen-share browser capability probe

Usage:
  node scripts/screen-share-browser-probe.mjs [options]

Options:
  --browser <name>  all, chrome, or edge (default: all)
  --host-ip <ipv4>  Non-loopback IPv4 used by the browser (auto-detected by default)
  --output <path>   JSON report path (default: artifacts/screen-share-benchmarks/browser-capabilities-<timestamp>.json)
  --help            Show this help

The probe starts a one-shot HTTP server on an ephemeral port, opens the page by
its LAN IPv4 address, and records secure-context, API constructor capabilities,
and a local synthetic WebRTC media loopback. Headless results do not replace
managed-browser, LAN peer, certificate, or real screen-share media tests.
`;

function optionValue(argv, index, name) {
  const value = argv[index + 1];
  if (value === undefined || value.startsWith('--')) throw new Error(`--${name} requires a value`);
  return value;
}

function defaultOutputPath(now = new Date()) {
  const timestamp = now.toISOString().replaceAll(':', '').replaceAll('-', '').replace(/\.\d{3}Z$/u, 'Z');
  return resolve(
    'artifacts',
    'screen-share-benchmarks',
    `browser-capabilities-${timestamp}.json`,
  );
}

export function parseArgs(argv, now = new Date()) {
  const options = {
    browser: 'all',
    hostIp: null,
    output: null,
    help: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--help') {
      options.help = true;
      continue;
    }
    if (!argument.startsWith('--')) throw new Error(`unexpected positional argument: ${argument}`);
    const name = argument.slice(2);
    const value = optionValue(argv, index, name);
    index += 1;
    switch (name) {
      case 'browser': options.browser = value.toLowerCase(); break;
      case 'host-ip': options.hostIp = value; break;
      case 'output': options.output = resolve(value); break;
      default: throw new Error(`unknown option: --${name}`);
    }
  }
  if (options.help) return options;
  if (!BROWSERS.has(options.browser)) throw new Error('--browser must be all, chrome, or edge');
  if (options.hostIp !== null && !isProbeAddress(options.hostIp)) {
    throw new Error('--host-ip must be a non-loopback, non-link-local IPv4 address');
  }
  options.output ??= defaultOutputPath(now);
  return options;
}

export function isProbeAddress(address) {
  return isIP(address) === 4
    && !address.startsWith('127.')
    && !address.startsWith('169.254.')
    && address !== '0.0.0.0';
}

function isPrivateIpv4(address) {
  if (address.startsWith('10.') || address.startsWith('192.168.')) return true;
  const octets = address.split('.').map(Number);
  return octets[0] === 172 && octets[1] >= 16 && octets[1] <= 31;
}

export function discoverProbeAddresses(interfaces = networkInterfaces()) {
  const addresses = [];
  for (const [interfaceName, entries] of Object.entries(interfaces)) {
    for (const entry of entries ?? []) {
      if (entry.family !== 'IPv4' || entry.internal || !isProbeAddress(entry.address)) continue;
      addresses.push({
        interfaceName,
        address: entry.address,
        private: isPrivateIpv4(entry.address),
      });
    }
  }
  return addresses.sort((left, right) => (
    Number(right.private) - Number(left.private)
      || left.interfaceName.localeCompare(right.interfaceName)
      || left.address.localeCompare(right.address)
  ));
}

function browserCandidates(kind) {
  if (platform() === 'win32') {
    const programFiles = process.env.ProgramFiles ?? 'C:\\Program Files';
    const programFilesX86 = process.env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)';
    const localAppData = process.env.LOCALAPPDATA ?? '';
    return kind === 'edge'
      ? [
          join(programFilesX86, 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
          join(programFiles, 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
          join(localAppData, 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
        ]
      : [
          join(programFiles, 'Google', 'Chrome', 'Application', 'chrome.exe'),
          join(programFilesX86, 'Google', 'Chrome', 'Application', 'chrome.exe'),
          join(localAppData, 'Google', 'Chrome', 'Application', 'chrome.exe'),
        ];
  }
  if (platform() === 'darwin') {
    return kind === 'edge'
      ? ['/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge']
      : ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'];
  }
  return kind === 'edge'
    ? ['/usr/bin/microsoft-edge', '/usr/bin/microsoft-edge-stable']
    : ['/usr/bin/google-chrome', '/usr/bin/google-chrome-stable', '/usr/bin/chromium'];
}

export function findBrowserExecutable(kind, fileExists = existsSync) {
  return browserCandidates(kind).find((candidate) => candidate && fileExists(candidate)) ?? null;
}

function probePage() {
  return `<!doctype html><meta charset="utf-8"><title>screen-share-browser-probe</title>
<body>probe pending<script>
(() => {
  const now = () => Math.round(performance.now() * 1000) / 1000;
  const waitFor = (predicate, timeoutMs) => new Promise((resolve, reject) => {
    const deadline = performance.now() + timeoutMs;
    const poll = () => {
      if (predicate()) return resolve();
      if (performance.now() >= deadline) return reject(new Error('timed out'));
      setTimeout(poll, 20);
    };
    poll();
  });
  const loopbackMedia = async () => {
    const evidence = {
      supported: typeof RTCPeerConnection === 'function'
        && typeof HTMLCanvasElement.prototype.captureStream === 'function',
      attempted: false,
      success: false,
      ice_connection_state: null,
      connection_state: null,
      signaling_state: null,
      remote_track_received: false,
      video_ready_state: null,
      video_frames_observed: 0,
      frames_decoded: null,
      frames_received: null,
      bytes_received: null,
      negotiation_ms: null,
      media_observation_ms: null,
      duration_ms: null,
      video_frame_callback_error: null,
      error: null,
      managed_browser_external_acceptance: false,
    };
    if (!evidence.supported) return evidence;
    evidence.attempted = true;
    const started = performance.now();
    const sender = new RTCPeerConnection({ iceServers: [] });
    const receiver = new RTCPeerConnection({ iceServers: [] });
    const canvas = document.createElement('canvas');
    canvas.width = 96; canvas.height = 54;
    const context = canvas.getContext('2d');
    const video = document.createElement('video');
    video.muted = true; video.autoplay = true; video.playsInline = true;
    document.body.append(video);
    let timer = null;
    let captureTrack = null;
    try {
      const draw = () => {
        const tick = Math.floor(performance.now());
        context.fillStyle = 'rgb(' + (tick % 255) + ',80,160)';
        context.fillRect(0, 0, canvas.width, canvas.height);
        context.fillStyle = '#fff';
        context.fillRect((tick / 8) % canvas.width, 8, 12, 12);
        captureTrack?.requestFrame?.();
      };
      draw(); timer = setInterval(draw, 50);
      const stream = canvas.captureStream(0);
      const track = stream.getVideoTracks()[0];
      if (!track) throw new Error('canvas captureStream produced no video track');
      captureTrack = track;
      draw();
      sender.addTrack(track, stream);
      sender.onicecandidate = (event) => {
        if (event.candidate) receiver.addIceCandidate(event.candidate).catch(() => {});
      };
      receiver.onicecandidate = (event) => {
        if (event.candidate) sender.addIceCandidate(event.candidate).catch(() => {});
      };
      receiver.ontrack = (event) => {
        evidence.remote_track_received = true;
        video.srcObject = event.streams[0] || new MediaStream([event.track]);
        video.play().catch(() => {});
      };
      const offer = await sender.createOffer();
      await sender.setLocalDescription(offer);
      await receiver.setRemoteDescription(offer);
      const answer = await receiver.createAnswer();
      await receiver.setLocalDescription(answer);
      await sender.setRemoteDescription(answer);
      evidence.negotiation_ms = now() - Math.round(started * 1000) / 1000;
      await waitFor(() => receiver.connectionState === 'connected' || receiver.iceConnectionState === 'connected' || receiver.iceConnectionState === 'completed', 8000);
      const mediaStarted = performance.now();
      await waitFor(() => video.srcObject !== null, 5000);
      await video.play();
      draw();
      const collectInboundStats = async () => {
        const stats = await receiver.getStats();
        for (const stat of stats.values()) {
          if (stat.type === 'inbound-rtp' && (stat.kind === 'video' || stat.mediaType === 'video')) {
            evidence.frames_decoded = stat.framesDecoded ?? null;
            evidence.frames_received = stat.framesReceived ?? null;
            evidence.bytes_received = stat.bytesReceived ?? null;
            return;
          }
        }
      };
      if (typeof video.requestVideoFrameCallback === 'function') {
        try { await new Promise((resolve, reject) => {
          const timeout = setTimeout(() => reject(new Error('timed out waiting for remote video frame')), 5000);
          video.requestVideoFrameCallback(() => { evidence.video_frames_observed += 1; clearTimeout(timeout); resolve(); });
        }); } catch (error) { evidence.video_frame_callback_error = error instanceof Error ? error.message : String(error); }
      } else {
        try { await waitFor(() => video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA, 5000); evidence.video_frames_observed = 1; }
        catch (error) { evidence.video_frame_callback_error = error instanceof Error ? error.message : String(error); }
      }
      await collectInboundStats();
      evidence.video_ready_state = video.readyState;
      evidence.media_observation_ms = now() - Math.round(mediaStarted * 1000) / 1000;
      evidence.success = evidence.remote_track_received && video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA
        && (evidence.video_frames_observed > 0
          || (evidence.frames_decoded ?? evidence.frames_received ?? 0) > 0
          || (evidence.bytes_received ?? 0) > 0)
        && ((evidence.frames_decoded ?? evidence.frames_received ?? 0) > 0 || (evidence.bytes_received ?? 0) > 0);
      if (!evidence.success) evidence.error = 'loopback connected without decoded inbound video evidence';
    } catch (error) {
      evidence.error = error instanceof Error ? error.message : String(error);
    } finally {
      evidence.ice_connection_state = receiver.iceConnectionState;
      evidence.connection_state = receiver.connectionState;
      evidence.signaling_state = receiver.signalingState;
      evidence.duration_ms = now() - Math.round(started * 1000) / 1000;
      if (timer !== null) clearInterval(timer);
      sender.close(); receiver.close();
      video.remove();
    }
    return evidence;
  };
  const finish = async () => {
  let peerConstructed = false;
  let peerError = null;
  try {
    const peer = new RTCPeerConnection();
    peer.close();
    peerConstructed = true;
  } catch (error) {
    peerError = error instanceof Error ? error.message : String(error);
  }
  const result = {
    href: location.href,
    isSecureContext: window.isSecureContext,
    crossOriginIsolated: window.crossOriginIsolated,
    rtcPeerConnectionType: typeof RTCPeerConnection,
    rtcPeerConnectionConstructed: peerConstructed,
    rtcPeerConnectionError: peerError,
    videoDecoderType: typeof VideoDecoder,
    mediaSourceType: typeof MediaSource,
    webrtc_loopback_media: await loopbackMedia(),
    userAgent: navigator.userAgent,
    webdriver: navigator.webdriver,
    managed_browser_external_acceptance: false,
  };
  const bytes = new TextEncoder().encode(JSON.stringify(result));
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  document.body.textContent = 'PROBE_JSON:' + btoa(binary);
  document.title = 'PROBE_DONE';
  };
  finish().catch((error) => { document.body.textContent = 'PROBE_ERROR:' + String(error); document.title = 'PROBE_FAILED'; });
})();
</script></body>`;
}

async function listenProbeServer(hostIp) {
  const page = Buffer.from(probePage(), 'utf8');
  const server = http.createServer((request, response) => {
    if (request.url !== '/') {
      response.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
      response.end('not found');
      return;
    }
    response.writeHead(200, {
      'Cache-Control': 'no-store',
      'Content-Length': page.length,
      'Content-Type': 'text/html; charset=utf-8',
    });
    response.end(page);
  });
  await new Promise((resolvePromise, rejectPromise) => {
    server.once('error', rejectPromise);
    server.listen(0, hostIp, resolvePromise);
  });
  const address = server.address();
  if (address === null || typeof address === 'string') throw new Error('probe server has no TCP port');
  return { server, port: address.port };
}

function parseProbeDom(dom) {
  const encoded = /PROBE_JSON:([A-Za-z0-9+/=]+)/u.exec(dom)?.[1];
  if (!encoded) throw new Error('browser DOM did not contain a completed probe result');
  return JSON.parse(Buffer.from(encoded, 'base64').toString('utf8'));
}

export function browserVersionFromUserAgent(kind, userAgent) {
  const pattern = kind === 'edge'
    ? /\bEdg\/([0-9.]+)/u
    : /\b(?:HeadlessChrome|Chrome)\/([0-9.]+)/u;
  return pattern.exec(userAgent)?.[1] ?? null;
}

export function normalizeLoopbackEvidence(evidence) {
  const value = evidence && typeof evidence === 'object' ? evidence : {};
  return {
    supported: value.supported === true,
    attempted: value.attempted === true,
    success: value.success === true,
    ice_connection_state: value.ice_connection_state ?? null,
    connection_state: value.connection_state ?? null,
    signaling_state: value.signaling_state ?? null,
    remote_track_received: value.remote_track_received === true,
    video_frames_observed: Number.isFinite(value.video_frames_observed) ? value.video_frames_observed : 0,
    frames_decoded: Number.isFinite(value.frames_decoded) ? value.frames_decoded : null,
    frames_received: Number.isFinite(value.frames_received) ? value.frames_received : null,
    bytes_received: Number.isFinite(value.bytes_received) ? value.bytes_received : null,
    error: typeof value.error === 'string' ? value.error : null,
    video_frame_callback_error: typeof value.video_frame_callback_error === 'string' ? value.video_frame_callback_error : null,
    managed_browser_external_acceptance: false,
  };
}

async function runBrowserProbe(kind, executable, url) {
  const temporaryRoot = resolve(tmpdir());
  const profile = await mkdtemp(join(temporaryRoot, `fst-screen-share-${kind}-`));
  const resolvedProfile = resolve(profile);
  if (!resolvedProfile.startsWith(`${temporaryRoot}${sep}`)) {
    throw new Error('temporary browser profile resolved outside the OS temp directory');
  }
  try {
    const { stdout } = await execFileAsync(executable, [
      '--headless=new',
      '--no-first-run',
      '--no-default-browser-check',
      `--user-data-dir=${resolvedProfile}`,
      '--autoplay-policy=no-user-gesture-required',
      '--virtual-time-budget=12000',
      '--dump-dom',
      url,
    ], {
      encoding: 'utf8',
      maxBuffer: 4 * 1024 * 1024,
      timeout: 20_000,
      windowsHide: true,
    });
    const result = parseProbeDom(stdout);
    result.webrtc_loopback_media = normalizeLoopbackEvidence(result.webrtc_loopback_media);
    result.managed_browser_external_acceptance = false;
    return {
      browser: kind,
      executable,
      version: browserVersionFromUserAgent(kind, result.userAgent),
      version_source: 'user-agent-reduced',
      result,
      error: null,
    };
  } catch (error) {
    return {
      browser: kind,
      executable,
      version: null,
      version_source: null,
      result: null,
      error: error instanceof Error ? error.message : String(error),
    };
  } finally {
    await rm(resolvedProfile, { recursive: true, force: true });
  }
}

export async function runProbe(options) {
  const discovered = discoverProbeAddresses();
  const hostIp = options.hostIp ?? discovered[0]?.address;
  if (!hostIp) throw new Error('no non-loopback IPv4 address found; pass --host-ip explicitly');
  const requestedKinds = options.browser === 'all' ? ['chrome', 'edge'] : [options.browser];
  const browsers = requestedKinds.map((kind) => ({
    kind,
    executable: findBrowserExecutable(kind),
  }));
  if (browsers.every(({ executable }) => executable === null)) {
    throw new Error(`no requested browser executable found (${requestedKinds.join(', ')})`);
  }

  const { server, port } = await listenProbeServer(hostIp);
  const url = `http://${hostIp}:${port}/`;
  try {
    const results = [];
    for (const browser of browsers) {
      if (browser.executable === null) {
        results.push({
          browser: browser.kind,
          executable: null,
          version: null,
          version_source: null,
          result: null,
          error: 'browser executable not found',
        });
      } else {
        results.push(await runBrowserProbe(browser.kind, browser.executable, url));
      }
    }
    return {
      schema_version: 1,
      generated_at_utc: new Date().toISOString(),
      host_ip: hostIp,
      probe_url: url,
      discovered_addresses: discovered,
      browsers: results,
      limitations: [
        'The page is served by this process over plain HTTP using a non-loopback address.',
        'Headless capability results do not prove media decode, rendering, managed-policy, certificate, or multi-device behavior.',
        'A successful local WebRTC loopback proves only synthetic same-browser ICE/DTLS/RTP and decoded-frame evidence.',
        'managed_browser_external_acceptance is always false: policy, profile, certificate, LAN peer, and independent-device acceptance remain external tests.',
      ],
    };
  } finally {
    await new Promise((resolvePromise) => server.close(resolvePromise));
  }
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
  let report;
  try {
    report = await runProbe(options);
  } catch (error) {
    report = {
      schema_version: 1,
      generated_at_utc: new Date().toISOString(),
      result: 'failed',
      error: error instanceof Error ? error.message : String(error),
    };
    process.exitCode = 1;
  }
  await mkdir(dirname(options.output), { recursive: true });
  await writeFile(options.output, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  process.stdout.write(`${options.output}\n`);
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : null;
if (invokedPath === import.meta.url) await main();
