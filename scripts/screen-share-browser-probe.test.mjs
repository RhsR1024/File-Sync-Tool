import assert from 'node:assert/strict';
import test from 'node:test';

import {
  browserVersionFromUserAgent,
  discoverProbeAddresses,
  findBrowserExecutable,
  isProbeAddress,
  normalizeLoopbackEvidence,
  parseArgs,
} from './screen-share-browser-probe.mjs';

test('parseArgs validates browser and LAN address while resolving output', () => {
  const parsed = parseArgs([
    '--browser', 'edge',
    '--host-ip', '192.168.50.10',
    '--output', 'artifacts/browser.json',
  ]);
  assert.equal(parsed.browser, 'edge');
  assert.equal(parsed.hostIp, '192.168.50.10');
  assert.match(parsed.output, /artifacts[\\/]browser\.json$/u);
  assert.throws(() => parseArgs(['--browser', 'firefox']), /all, chrome, or edge/u);
  assert.throws(() => parseArgs(['--host-ip', '127.0.0.1']), /non-loopback/u);
});

test('probe address rejects localhost, wildcard, link-local, and IPv6', () => {
  assert.equal(isProbeAddress('192.168.1.20'), true);
  assert.equal(isProbeAddress('198.18.0.1'), true);
  assert.equal(isProbeAddress('127.0.0.1'), false);
  assert.equal(isProbeAddress('169.254.1.2'), false);
  assert.equal(isProbeAddress('0.0.0.0'), false);
  assert.equal(isProbeAddress('::1'), false);
});

test('address discovery prefers private IPv4 interfaces deterministically', () => {
  const addresses = discoverProbeAddresses({
    Virtual: [{ address: '198.18.0.1', family: 'IPv4', internal: false }],
    Ethernet: [{ address: '192.168.1.22', family: 'IPv4', internal: false }],
    Loopback: [{ address: '127.0.0.1', family: 'IPv4', internal: true }],
  });
  assert.deepEqual(addresses.map(({ address }) => address), ['192.168.1.22', '198.18.0.1']);
});

test('browser executable discovery uses the first existing candidate', () => {
  const executable = findBrowserExecutable('chrome', (candidate) => (
    candidate.toLowerCase().includes('google') && candidate.endsWith('chrome.exe')
  ));
  if (process.platform === 'win32') assert.match(executable, /Google[\\/]Chrome/u);
  else assert.equal(typeof executable === 'string' || executable === null, true);
});

test('browser version extraction handles reduced Chrome and Edge user agents', () => {
  assert.equal(
    browserVersionFromUserAgent(
      'chrome',
      'Mozilla/5.0 HeadlessChrome/150.0.0.0 Safari/537.36',
    ),
    '150.0.0.0',
  );
  assert.equal(
    browserVersionFromUserAgent(
      'edge',
      'Mozilla/5.0 Chrome/150.0.0.0 Safari/537.36 Edg/150.0.0.0',
    ),
    '150.0.0.0',
  );
});

test('loopback evidence preserves decoded media facts without becoming managed-browser acceptance', () => {
  const evidence = normalizeLoopbackEvidence({
    supported: true,
    attempted: true,
    success: true,
    ice_connection_state: 'completed',
    connection_state: 'connected',
    signaling_state: 'stable',
    remote_track_received: true,
    video_frames_observed: 2,
    frames_decoded: 2,
    frames_received: 2,
    bytes_received: 4096,
    managed_browser_external_acceptance: true,
  });
  assert.equal(evidence.success, true);
  assert.equal(evidence.frames_decoded, 2);
  assert.equal(evidence.managed_browser_external_acceptance, false);
});

test('unsupported or failed loopback evidence remains explicit', () => {
  const unsupported = normalizeLoopbackEvidence(null);
  assert.deepEqual(unsupported, {
    supported: false,
    attempted: false,
    success: false,
    ice_connection_state: null,
    connection_state: null,
    signaling_state: null,
    remote_track_received: false,
    video_frames_observed: 0,
    frames_decoded: null,
    frames_received: null,
    bytes_received: null,
    error: null,
    video_frame_callback_error: null,
    managed_browser_external_acceptance: false,
  });
  const failed = normalizeLoopbackEvidence({ supported: true, attempted: true, error: 'ICE timed out' });
  assert.equal(failed.success, false);
  assert.equal(failed.error, 'ICE timed out');
  assert.equal(failed.managed_browser_external_acceptance, false);
});
