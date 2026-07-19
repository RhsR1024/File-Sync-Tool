import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const page = readFileSync(new URL('./VideoDeviceSimulatorPage.vue', import.meta.url), 'utf8');
const composable = readFileSync(new URL('../composables/useDeviceSimulator.ts', import.meta.url), 'utf8');
const router = readFileSync(new URL('../router/index.ts', import.meta.url), 'utf8');
const sidebar = readFileSync(new URL('../lib/sidebarNavigation.ts', import.meta.url), 'utf8');
const messages = readFileSync(new URL('../locales/messages.ts', import.meta.url), 'utf8');

assert.match(router, /path: '\/tools\/video-device-simulator'/);
assert.match(sidebar, /labelKey: 'sidebar\.videoDeviceSimulator'/);
assert.ok(messages.match(/videoDeviceSimulator: 'Virtual Device Simulation'/));
assert.ok(messages.match(/videoDeviceSimulator: '虚拟设备模拟'/));
assert.ok(messages.includes("title: '虚拟设备模拟'"));
assert.ok(messages.includes("title: '发图配置'"));
for (const legacyLabel of ['服务器配置', '虚拟设备起始 IP', '虚拟设备数量', '设备类型', '发送图片规格', '发送图片间隔（毫秒）', '预设发包数', '告警类型', '数据统计']) {
  assert.ok(messages.includes(legacyLabel), `missing familiar label: ${legacyLabel}`);
}

assert.match(page, /min-h-11/, 'primary controls should meet the 44px target');
assert.match(page, /focus-visible:ring-2/, 'keyboard focus must remain visible');
assert.match(page, /prefers-reduced-motion: reduce/, 'reduced motion must be respected');
assert.match(page, /:disabled="simulator\.topologyLocked\.value"/, 'topology fields must lock while active');
assert.match(page, /simulator\.recoverySessionId\.value/, 'residual sessions must be presented before normal work');
assert.match(page, /simulator\.runPreflight/, 'structured preflight must be available');
assert.match(page, /simulator\.alarmStats\.value/, 'alarm statistics must be visible');
assert.match(page, /simulator\.alarmTypes\.value/, 'alarm names must come from the prepared device files');
assert.match(page, /requiredFileLabel/, 'internal file identifiers must use familiar display labels');
assert.doesNotMatch(messages, /告警类型 ID/, 'internal alarm IDs must not be shown to users');
assert.doesNotMatch(page, /<option value="normal">/, 'picture sizes must keep the legacy three-choice UI');
assert.doesNotMatch(page, /value="vms"|>VMS</i, 'the simulator must expose UMS only');
assert.match(page, /ipc-structured/, 'structured camera must be available');
assert.match(page, /ipc-face-access/, 'face access camera must be available');
assert.match(page, /send_count: continuousAlarm\.value \? null/, 'continuous alarm mode must cross the API as null, never a magic zero');
assert.match(page, /alarm\.mode !== 'configured'/, 'random and sequential reporting must not retain a selected alarm type');
assert.match(page, /downloadJson\('device-simulator-logs\.json'/, 'logs must be exportable');

assert.match(composable, /DEVICE_SIMULATOR_EVENTS\.status/);
assert.match(composable, /DEVICE_SIMULATOR_EVENTS\.cleanupProgress/);
assert.match(composable, /hasBlockingPreflightFailure/);
assert.match(composable, /payload\.state === 'ready'[\s\S]*await refreshAlarmTypes\(\)/, 'alarm names must refresh when required files finish preparing');
assert.match(composable, /result\.state === 'ready' \|\| result\.state === 'update_available'/, 'installed alarm names must remain available when a newer file set exists');
assert.match(composable, /servers: \[\{ id: newId\('server'\), host: '', port: 80 \}\]/, 'server IP and port must be visible without an extra setup step');
assert.doesNotMatch(composable, /password|access_token|worker_process_id/i);

console.log('VideoDeviceSimulatorPage contract tests PASSED');
