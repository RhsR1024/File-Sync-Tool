import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const page = readFileSync(new URL('./VideoDeviceSimulatorPage.vue', import.meta.url), 'utf8');
const composable = readFileSync(new URL('../composables/useDeviceSimulator.ts', import.meta.url), 'utf8');
const router = readFileSync(new URL('../router/index.ts', import.meta.url), 'utf8');
const sidebar = readFileSync(new URL('../lib/sidebarNavigation.ts', import.meta.url), 'utf8');
const messages = readFileSync(new URL('../locales/messages.ts', import.meta.url), 'utf8');

assert.match(router, /path: '\/tools\/video-device-simulator'/);
assert.match(sidebar, /labelKey: 'sidebar\.videoDeviceSimulator'/);
assert.ok(messages.match(/videoDeviceSimulator: 'Video Device Simulator'/));
assert.ok(messages.match(/videoDeviceSimulator: '视频设备模拟器'/));

assert.match(page, /min-h-11/, 'primary controls should meet the 44px target');
assert.match(page, /focus-visible:ring-2/, 'keyboard focus must remain visible');
assert.match(page, /prefers-reduced-motion: reduce/, 'reduced motion must be respected');
assert.match(page, /:disabled="simulator\.topologyLocked\.value"/, 'topology fields must lock while active');
assert.match(page, /simulator\.recoverySessionId\.value/, 'residual sessions must be presented before normal work');
assert.match(page, /simulator\.runPreflight/, 'structured preflight must be available');
assert.match(page, /simulator\.alarmStats\.value/, 'alarm statistics must be visible');
assert.doesNotMatch(page, /value="vms"|>VMS</i, 'the simulator must expose UMS only');
assert.match(page, /ipc-structured/, 'structured camera must be available');
assert.match(page, /ipc-face-access/, 'face access camera must be available');
assert.match(page, /send_count: continuousAlarm\.value \? null/, 'continuous alarm mode must cross the API as null, never a magic zero');
assert.match(page, /downloadJson\('device-simulator-logs\.json'/, 'logs must be exportable');

assert.match(composable, /DEVICE_SIMULATOR_EVENTS\.status/);
assert.match(composable, /DEVICE_SIMULATOR_EVENTS\.cleanupProgress/);
assert.match(composable, /hasBlockingPreflightFailure/);
assert.doesNotMatch(composable, /password|access_token|worker_process_id/i);

console.log('VideoDeviceSimulatorPage contract tests PASSED');
