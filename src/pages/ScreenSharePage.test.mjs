import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const pageSource = readFileSync(join(__dirname, 'ScreenSharePage.vue'), 'utf8');
const cargoSource = readFileSync(join(__dirname, '..', '..', 'src-tauri', 'Cargo.toml'), 'utf8');

test('screen share shows the connected IP list directly without a details toggle gate', () => {
  assert.match(pageSource, /tools\.screenShare\.connectedIpList/);
  assert.doesNotMatch(pageSource, /v-if="showConnectionDetails"/);
  assert.doesNotMatch(pageSource, /@click="showConnectionDetails = !showConnectionDetails"/);
  assert.doesNotMatch(pageSource, /tools\.screenShare\.connectionDetails/);
});

test('screen share still collapses connected IPs after the first 10 entries', () => {
  assert.match(pageSource, /connectedIps\.value\.slice\(0, 10\)/);
  assert.match(pageSource, /Math\.max\(0, connectedIps\.value\.length - 10\)/);
});

test('screen share places connection count and uptime side by side like file share', () => {
  assert.match(
    pageSource,
    /<div class="ss-stat-card">[\s\S]*?tools\.screenShare\.connectionCount[\s\S]*?{{ connectionCount }}[\s\S]*?<\/div>\s*<div class="ss-stat-card">[\s\S]*?tools\.screenShare\.uptime[\s\S]*?{{ formattedUptime }}/,
  );
});

test('screen share stop action executes directly without a confirmation prompt', () => {
  assert.doesNotMatch(pageSource, /window\.confirm\(t\('tools\.screenShare\.stopConfirm'\)\)/);
  assert.match(pageSource, /@click="stopShare"/);
  assert.doesNotMatch(pageSource, /@click="confirmStopShare"/);
});

test('screen share exposes 60 FPS as the final discrete frame-rate tier', () => {
  assert.match(pageSource, /const FRAME_RATE_OPTIONS = \[5, 10, 15, 20, 25, 30, 60\] as const;/);
  assert.match(pageSource, /const DEFAULT_FRAME_RATE = 15;/);
  assert.match(pageSource, /const frameRateIndex = ref\(FRAME_RATE_OPTIONS\.indexOf\(DEFAULT_FRAME_RATE\)\);/);
  assert.match(
    pageSource,
    /v-model\.number="frameRateIndex"[\s\S]*?min="0"[\s\S]*?:max="FRAME_RATE_OPTIONS\.length - 1"[\s\S]*?step="1"/,
  );
  assert.match(pageSource, /tools\.screenShare\.highFpsNotice/);
  assert.doesNotMatch(pageSource, /tools\.screenShare\.highFpsExperiment/);
  // 原生滑块按档位索引移动，30 后直接跳到 60，不暴露 35-55 FPS。
  assert.doesNotMatch(pageSource, /v-model\.number="fps"[\s\S]*?min="5"[\s\S]*?max="30"/);
});

test('screen share frame-rate ticks align with the native range thumb positions', () => {
  assert.match(
    pageSource,
    /function frameRateTickPosition\(index: number\): string \{[\s\S]*?index \/ \(FRAME_RATE_OPTIONS\.length - 1\)/,
  );
  assert.match(pageSource, /:style="\{ left: frameRateTickPosition\(index\) \}"/);
  assert.match(pageSource, /\.ss-range-ticks \{[\s\S]*?margin-inline: calc\(var\(--ss-range-thumb-size\) \/ 2\);/);
  assert.doesNotMatch(pageSource, /grid grid-cols-7 font-mono/);
});

test('screen share persists the selected frame rate and migrates the legacy experiment toggle', () => {
  assert.match(pageSource, /fps: fps\.value,/);
  assert.match(pageSource, /saved\.highFpsExperiment \? HIGH_FRAME_RATE : saved\.fps/);
  assert.doesNotMatch(pageSource, /highFpsExperiment: highFpsExperiment\.value,/);
});

test('screen share removes WebCodecs and migrates its saved transport to MSE H.264', () => {
  assert.doesNotMatch(pageSource, /value: 'web_codecs' as const/);
  assert.doesNotMatch(pageSource, /mediaTransportWebCodecs/);
  assert.match(
    pageSource,
    /saved\.mediaTransport === 'web_codecs'[\s\S]*?\? 'mse_h264'/,
  );
});

test('standard builds compile the WebRTC transport by default', () => {
  assert.match(cargoSource, /default = \["screen-share-webrtc-prototype"\]/);
});

test('screen share exposes explicit capture backend modes with explanatory helper copy', () => {
  assert.match(pageSource, /tools\.screenShare\.backendMode/);
  assert.match(pageSource, /tools\.screenShare\.backendModeHint/);
  assert.match(pageSource, /tools\.screenShare\.backendModeAuto/);
  assert.match(pageSource, /tools\.screenShare\.backendModeWgc/);
  assert.match(pageSource, /tools\.screenShare\.backendModeDxgi/);
  assert.match(pageSource, /tools\.screenShare\.backendModeAutoDesc/);
  assert.match(pageSource, /tools\.screenShare\.backendModeWgcDesc/);
  assert.match(pageSource, /tools\.screenShare\.backendModeDxgiDesc/);
});

test('screen share no longer offers the host local preview', () => {
  // 预览窗口显示的就是被捕获的那块屏幕，必然无限嵌套；排除捕获会让 WGC/DXGI
  // 返回黑帧（见 screen_share_desktop_overlay_ready 的注释），所以该功能已删除。
  assert.doesNotMatch(pageSource, /screenShareOpenLocalPreview/);
  assert.doesNotMatch(pageSource, /openLocalPreview/);
  assert.doesNotMatch(pageSource, /isOpeningPreview/);
  assert.doesNotMatch(pageSource, /tools\.screenShare\.openLocalPreview/);
  assert.doesNotMatch(pageSource, /tools\.screenShare\.errPreviewFailed/);
  const tauriSource = readFileSync(join(__dirname, '..', 'lib', 'tauri.ts'), 'utf8');
  assert.doesNotMatch(tauriSource, /screen_share_(open|close)_local_preview/);
});
