<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import {
  ArrowUpRight,
  CircleAlert,
  CircleDot,
  Eraser,
  Eye,
  Maximize2,
  Minimize2,
  MonitorPlay,
  Pause,
  Play,
  Radio,
  RefreshCw,
  Square,
  Snowflake,
  Undo2,
  Wifi,
  WifiOff,
} from 'lucide-vue-next';

import AnnotationOverlay from './components/AnnotationOverlay.vue';
import { applyAnnotationApplied, applySnapshot, applyViewState, emptyDocument, normalizeDocument, resetForSource, visibleShapes } from './lib/annotation-state';
import { computeContainedRect, type ContainedRect } from './lib/coordinates';
import { ScreenShareSessionClient } from './lib/session-client';
import { detectLocale, messages, type ScreenShareLocale } from './messages';
import type {
  AnnotationAddPayload,
  AnnotationKind,
  AnnotationDocument,
  ScreenShareHttpStatus,
  SessionConnectionState,
  SessionServerMessage,
} from './types';

type Tool = AnnotationKind | 'view';

const locale = ref<ScreenShareLocale>(detectLocale());
const t = (key: keyof typeof messages.en): string => messages[locale.value][key] as string;

const stage = ref<HTMLElement | null>(null);
const screen = ref<HTMLImageElement | null>(null);
const imageSource = ref('/stream');
const naturalWidth = ref(0);
const naturalHeight = ref(0);
const geometry = ref<ContainedRect>(computeContainedRect(0, 0, 0, 0));
const streamConnected = ref(false);
let streamRetryTimer: number | null = null;
const streamRetryAttempt = ref(0);
const localPaused = ref(false);
const localFrameUrl = ref<string | null>(null);
const isFullscreen = ref(false);
const httpStatus = ref<ScreenShareHttpStatus>({});
const statusError = ref<string | null>(null);
const lastSessionError = ref<string | null>(null);
const viewerTick = ref(Date.now());

const documentState = ref<AnnotationDocument>(emptyDocument());
const clientId = ref('');
const annotationsEnabled = ref(true);
const sharedFreezeEnabled = ref(true);
const sessionState = ref<SessionConnectionState>({ status: 'idle', attempts: 0, lastError: null });
const tool = ref<Tool>('view');
const color = ref('#f59e0b');
const annotationWidth = ref(4);
const colors = ['#f59e0b', '#ef4444', '#22c55e', '#38bdf8', '#f8fafc'];
const annotationWidths = [2, 4, 7] as const;
const annotationWidthLabelKeys: Record<number, keyof typeof messages.en> = {
  2: 'lineWidthThin',
  4: 'lineWidthMedium',
  7: 'lineWidthThick',
};
const refreshRateMs = ref(0);
const refreshOptions = [
  { value: 0, labelKey: 'refreshOriginal' },
  { value: 500, labelKey: 'refresh2Fps' },
  { value: 1000, labelKey: 'refresh1Fps' },
  { value: 2000, labelKey: 'refreshHalfFps' },
  { value: 5000, labelKey: 'refresh5Sec' },
] as const;
const colorLabelKeys: Record<string, keyof typeof messages.en> = {
  '#f59e0b': 'colorAmber',
  '#ef4444': 'colorRed',
  '#22c55e': 'colorGreen',
  '#38bdf8': 'colorCyan',
  '#f8fafc': 'colorWhite',
};

const sessionClient = new ScreenShareSessionClient();
let statusTimer: number | null = null;
let laserTimer: number | null = null;
let sessionNoticeTimer: number | null = null;
let refreshTimer: number | null = null;
let singleRefreshInFlight = false;
let resizeObserver: ResizeObserver | null = null;

const statusActive = computed(() => httpStatus.value.active ?? httpStatus.value.is_active ?? true);
const viewerCount = computed(() => httpStatus.value.viewers ?? httpStatus.value.viewer_count ?? 0);
const captureIssue = computed(() => httpStatus.value.capture_issue ?? null);
const visibleAnnotationShapes = computed(() => visibleShapes(documentState.value, viewerTick.value));
const hasOwnAnnotations = computed(() => Boolean(clientId.value)
  && visibleAnnotationShapes.value.some((shape) => shape.ownerClientId === clientId.value));
const hasOwnPersistentAnnotations = computed(() => Boolean(clientId.value)
  && documentState.value.shapes.some((shape) => (
    shape.ownerClientId === clientId.value && shape.kind !== 'laser'
  )));
const interactionConnected = computed(() => sessionState.value.status === 'connected');
const interactionLabel = computed(() => interactionConnected.value ? t('interactionConnected') : t('interactionOffline'));
const streamLabel = computed(() => {
  if (!statusActive.value) return t('stopped');
  if (captureIssue.value === 'privacy_mode_or_display_off') return t('capturePrivacy');
  if (captureIssue.value) return t('captureRetrying');
  if (streamConnected.value) return t('connected');
  if (sessionState.value.status === 'reconnecting' || sessionState.value.status === 'connecting') return t('reconnecting');
  return t('noConnection');
});
const streamDotClass = computed(() => {
  if (!statusActive.value || sessionState.value.status === 'closed') return 'is-off';
  if (captureIssue.value === 'privacy_mode_or_display_off') return 'is-warn';
  if (streamConnected.value) return 'is-on';
  return 'is-retry';
});
const sharedFrozen = computed(() => documentState.value.mode === 'frozen');
const isLocallyHeld = computed(() => localPaused.value);
const stageMessage = computed(() => {
  if (localPaused.value) return t('localPaused');
  if (sharedFrozen.value) return t('frozenFrame');
  return '';
});
const canAnnotate = computed(() => (
  interactionConnected.value
  && annotationsEnabled.value
  && !localPaused.value
));
const canFreeze = computed(() => interactionConnected.value && sharedFreezeEnabled.value && !sharedFrozen.value);
const canResumeShared = computed(() => interactionConnected.value && sharedFreezeEnabled.value && sharedFrozen.value);

function normalizeStatus(value: unknown): ScreenShareHttpStatus {
  return value && typeof value === 'object' ? value as ScreenShareHttpStatus : {};
}

function showSessionNotice(message: string) {
  lastSessionError.value = message;
  if (sessionNoticeTimer !== null) window.clearTimeout(sessionNoticeTimer);
  sessionNoticeTimer = window.setTimeout(() => {
    lastSessionError.value = null;
    sessionNoticeTimer = null;
  }, 5000);
}

function updateGeometry() {
  const container = stage.value;
  if (!container) return;
  geometry.value = computeContainedRect(
    container.clientWidth,
    container.clientHeight,
    naturalWidth.value || screen.value?.naturalWidth || 0,
    naturalHeight.value || screen.value?.naturalHeight || 0,
    window.devicePixelRatio || 1,
  );
}

function markImageLoaded() {
  if (screen.value) {
    naturalWidth.value = screen.value.naturalWidth;
    naturalHeight.value = screen.value.naturalHeight;
  }
  singleRefreshInFlight = false;
  if (streamRetryTimer !== null) {
    window.clearTimeout(streamRetryTimer);
    streamRetryTimer = null;
  }
  streamConnected.value = true;
  streamRetryAttempt.value = 0;
  updateGeometry();
}

function scheduleStreamReconnect() {
  if (localPaused.value || sharedFrozen.value || !statusActive.value || streamRetryTimer !== null) return;
  streamConnected.value = false;
  streamRetryAttempt.value += 1;
  const delay = Math.min(8000, 500 * 2 ** Math.min(streamRetryAttempt.value - 1, 4));
  streamRetryTimer = window.setTimeout(() => {
    streamRetryTimer = null;
    if (refreshRateMs.value > 0) requestSingleFrame();
    else imageSource.value = `/stream?t=${Date.now()}`;
  }, delay);
}

function handleImageError() {
  singleRefreshInFlight = false;
  streamConnected.value = false;
  scheduleStreamReconnect();
}

function clearRefreshTimer() {
  if (refreshTimer !== null) window.clearInterval(refreshTimer);
  refreshTimer = null;
}

function requestSingleFrame() {
  if (
    localPaused.value
    || sharedFrozen.value
    || !statusActive.value
    || refreshRateMs.value <= 0
    || singleRefreshInFlight
  ) return;
  singleRefreshInFlight = true;
  imageSource.value = `/stream?single=1&t=${Date.now()}`;
}

function startSingleFramePolling() {
  clearRefreshTimer();
  requestSingleFrame();
  refreshTimer = window.setInterval(requestSingleFrame, refreshRateMs.value);
}

function startLiveStream() {
  if (localPaused.value || sharedFrozen.value) return;
  if (streamRetryTimer !== null) {
    window.clearTimeout(streamRetryTimer);
    streamRetryTimer = null;
  }
  if (refreshRateMs.value > 0) {
    startSingleFramePolling();
  } else {
    clearRefreshTimer();
    singleRefreshInFlight = false;
    imageSource.value = `/stream?t=${Date.now()}`;
  }
}

function revokeLocalFrame() {
  if (localFrameUrl.value?.startsWith('blob:')) URL.revokeObjectURL(localFrameUrl.value);
  localFrameUrl.value = null;
}

function captureCurrentFrame(): string | null {
  const image = screen.value;
  if (!image || !image.naturalWidth || !image.naturalHeight) return null;
  try {
    const dpr = window.devicePixelRatio || 1;
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(image.naturalWidth * dpr));
    canvas.height = Math.max(1, Math.round(image.naturalHeight * dpr));
    const context = canvas.getContext('2d');
    if (!context) return null;
    context.scale(dpr, dpr);
    context.drawImage(image, 0, 0, image.naturalWidth, image.naturalHeight);
    return canvas.toDataURL('image/jpeg', 0.9);
  } catch {
    return null;
  }
}

async function toggleLocalPause() {
  if (localPaused.value) {
    localPaused.value = false;
    revokeLocalFrame();
    if (sharedFrozen.value) {
      loadSharedFrame();
    } else {
      startLiveStream();
    }
    return;
  }
  const captured = captureCurrentFrame();
  if (!captured) return;
  clearRefreshTimer();
  singleRefreshInFlight = false;
  revokeLocalFrame();
  localFrameUrl.value = captured;
  localPaused.value = true;
  imageSource.value = captured;
  await nextTick();
  updateGeometry();
}

function loadSharedFrame(snapshotUrl?: string | null) {
  if (localPaused.value) return;
  clearRefreshTimer();
  singleRefreshInFlight = false;
  const frameId = documentState.value.frozenFrameId;
  const url = snapshotUrl || (frameId === null ? null : `/snapshot/${frameId}`);
  if (!url) return;
  imageSource.value = `${url}${url.includes('?') ? '&' : '?'}t=${Date.now()}`;
}

function applyDocument(next: unknown) {
  const incoming = normalizeDocument(next, documentState.value);
  const sourceChanged = incoming.sourceEpoch !== documentState.value.sourceEpoch
    && documentState.value.sourceEpoch !== 0;
  documentState.value = applySnapshot(documentState.value, incoming);
  if (sourceChanged) showSessionNotice(t('sourceChanged'));
  sessionClient.updateContext(documentState.value.sessionId, documentState.value.sourceEpoch);
  viewerTick.value = Date.now();
  if (!localPaused.value) {
    if (documentState.value.mode === 'frozen') loadSharedFrame();
    else startLiveStream();
  }
}

function handleSessionMessage(message: SessionServerMessage) {
  if (message.type === 'session.hello') {
    const payload = (message.payload ?? {}) as Record<string, unknown>;
    if (typeof payload.client_id === 'string') clientId.value = payload.client_id;
    const features = payload.features && typeof payload.features === 'object'
      ? payload.features as Record<string, unknown>
      : {};
    if (typeof features.annotations_enabled === 'boolean') annotationsEnabled.value = features.annotations_enabled;
    if (typeof features.shared_freeze_enabled === 'boolean') sharedFreezeEnabled.value = features.shared_freeze_enabled;
    sessionClient.updateContext(message.session_id, message.source_epoch);
    if (documentState.value.sessionId === 0) {
      documentState.value = emptyDocument(message.session_id, message.source_epoch);
    }
    return;
  }
  if (message.type === 'session.snapshot') {
    const payload = (message.payload ?? {}) as Record<string, unknown>;
    applyDocument(payload.document ?? payload);
    return;
  }
  if (message.type === 'annotation.applied') {
    const payload = (message.payload ?? {}) as Record<string, unknown>;
    if (payload.document) applyDocument(payload.document);
    else {
      const result = applyAnnotationApplied(documentState.value, payload, message.revision);
      if (result.needsSnapshot) reconnectInteraction();
      else documentState.value = result.document;
    }
    viewerTick.value = Date.now();
    return;
  }
  if (message.type === 'view.state') {
    const payload = (message.payload ?? {}) as Record<string, unknown>;
    if (payload.document) applyDocument(payload.document);
    else {
      documentState.value = applyViewState(documentState.value, payload.mode, payload.frame_id ?? payload.frozen_frame_id);
      if (documentState.value.mode === 'frozen') loadSharedFrame(typeof payload.snapshot_url === 'string' ? payload.snapshot_url : null);
      else if (!localPaused.value) startLiveStream();
    }
    return;
  }
  if (message.type === 'source.changed') {
    const payload = (message.payload ?? {}) as Record<string, unknown>;
    if (payload.document) applyDocument(payload.document);
    else documentState.value = resetForSource(documentState.value, message.source_epoch);
    showSessionNotice(t('sourceChanged'));
    return;
  }
  if (message.type === 'session.error') {
    const payload = (message.payload ?? {}) as Record<string, unknown>;
    const text = payload.code === 'frame_unavailable'
      ? t('frameUnavailable')
      : (typeof payload.message === 'string' ? payload.message : t('sessionError'));
    showSessionNotice(text);
  }
}

async function refreshStatus() {
  try {
    const response = await fetch(`/status?t=${Date.now()}`, { cache: 'no-store' });
    if (!response.ok) throw new Error(`status ${response.status}`);
    const nextStatus = normalizeStatus(await response.json());
    httpStatus.value = nextStatus;
    statusError.value = null;
    const sessionId = typeof nextStatus.session_id === 'number' ? nextStatus.session_id : documentState.value.sessionId;
    const sourceEpoch = typeof nextStatus.source_epoch === 'number' ? nextStatus.source_epoch : documentState.value.sourceEpoch;
    if (sessionId !== documentState.value.sessionId || sourceEpoch !== documentState.value.sourceEpoch) {
      documentState.value = emptyDocument(sessionId, sourceEpoch);
      sessionClient.updateContext(sessionId, sourceEpoch);
    }
    if (statusActive.value && sessionState.value.status === 'idle') sessionClient.connect(sessionId, sourceEpoch);
    if (!statusActive.value) {
      streamConnected.value = false;
      clearRefreshTimer();
      singleRefreshInFlight = false;
      if (streamRetryTimer !== null) {
        window.clearTimeout(streamRetryTimer);
        streamRetryTimer = null;
      }
    }
  } catch {
    statusError.value = t('statusUnavailable');
    if (sessionState.value.status === 'idle') sessionClient.connect(documentState.value.sessionId, documentState.value.sourceEpoch);
  }
}

function addAnnotation(payload: AnnotationAddPayload) {
  if (!canAnnotate.value) return;
  sessionClient.send('annotation.add', payload);
}

function undoOwn() {
  if (hasOwnPersistentAnnotations.value) sessionClient.send('annotation.undo');
}

function clearOwn() {
  if (hasOwnAnnotations.value) sessionClient.send('annotation.clear_own');
}

function toggleSharedFreeze() {
  if (canFreeze.value) sessionClient.send('view.freeze');
  else if (canResumeShared.value) sessionClient.send('view.resume');
}

function onRefreshRateChange() {
  if (localPaused.value || sharedFrozen.value || !statusActive.value) return;
  startLiveStream();
}

async function toggleFullscreen() {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await stage.value?.requestFullscreen();
  } catch {
    /* Browser policy can reject fullscreen without a user gesture. */
  }
}

function updateFullscreenState() {
  isFullscreen.value = Boolean(document.fullscreenElement);
}

function reconnectInteraction() {
  sessionClient.close();
  sessionClient.connect(documentState.value.sessionId, documentState.value.sourceEpoch);
}

function onVisibilityChange() {
  if (!document.hidden && statusActive.value) refreshStatus();
}

watch([naturalWidth, naturalHeight], updateGeometry);

onMounted(async () => {
  document.documentElement.lang = locale.value === 'zh' ? 'zh-CN' : 'en';
  document.title = t('title');
  sessionClient.onMessage(handleSessionMessage);
  sessionClient.onState((state) => { sessionState.value = state; });
  document.addEventListener('fullscreenchange', updateFullscreenState);
  document.addEventListener('visibilitychange', onVisibilityChange);
  resizeObserver = new ResizeObserver(updateGeometry);
  if (stage.value) resizeObserver.observe(stage.value);
  laserTimer = window.setInterval(() => { viewerTick.value = Date.now(); }, 250);
  statusTimer = window.setInterval(refreshStatus, 3000);
  await refreshStatus();
  if (sessionState.value.status === 'idle') sessionClient.connect(documentState.value.sessionId, documentState.value.sourceEpoch);
});

onUnmounted(() => {
  sessionClient.close();
  if (statusTimer !== null) window.clearInterval(statusTimer);
  if (laserTimer !== null) window.clearInterval(laserTimer);
  if (streamRetryTimer !== null) window.clearTimeout(streamRetryTimer);
  clearRefreshTimer();
  singleRefreshInFlight = false;
  if (sessionNoticeTimer !== null) window.clearTimeout(sessionNoticeTimer);
  resizeObserver?.disconnect();
  document.removeEventListener('fullscreenchange', updateFullscreenState);
  document.removeEventListener('visibilitychange', onVisibilityChange);
  revokeLocalFrame();
});
</script>

<template>
  <main class="screen-share-viewer">
    <header class="viewer-status-bar">
      <div class="status-cluster">
        <span class="brand-mark" aria-hidden="true"><MonitorPlay :size="17" /></span>
        <span class="viewer-title">{{ t('title') }}</span>
        <span class="status-pill" :class="streamDotClass">
          <span class="status-dot" aria-hidden="true" />
          <span>{{ streamLabel }}</span>
        </span>
        <span class="viewer-count" v-if="statusActive">
          <Wifi :size="14" aria-hidden="true" />
          {{ viewerCount }} {{ viewerCount === 1 ? t('viewer') : t('viewers') }}
        </span>
      </div>
      <div class="status-cluster status-cluster-right">
        <span class="interaction-status" :class="{ offline: !interactionConnected }" aria-live="polite">
          <CircleDot :size="13" aria-hidden="true" />
          {{ interactionLabel }}
        </span>
        <button
          v-if="!interactionConnected"
          type="button"
          class="icon-button compact"
          :title="t('reconnect')"
          :aria-label="t('reconnect')"
          @click="reconnectInteraction"
        >
          <RefreshCw :size="16" />
        </button>
      </div>
    </header>

    <section ref="stage" class="viewer-stage" :class="{ 'is-frozen': sharedFrozen, 'is-local-paused': isLocallyHeld }">
      <img
        ref="screen"
        class="screen-image"
        :src="imageSource"
        :alt="t('title')"
        draggable="false"
        @load="markImageLoaded"
        @error="handleImageError"
      />
      <AnnotationOverlay
        :shapes="visibleAnnotationShapes"
        :geometry="geometry"
        :tool="tool"
        :color="color"
        :width="annotationWidth"
        :enabled="canAnnotate"
        @add="addAnnotation"
      />
      <div v-if="stageMessage" class="stage-badge" role="status" aria-live="polite">
        <Snowflake v-if="sharedFrozen" :size="17" />
        <Pause v-else :size="17" />
        <span>{{ stageMessage }}</span>
      </div>
      <div v-if="captureIssue || statusError" class="capture-notice" :class="{ danger: captureIssue === 'privacy_mode_or_display_off' }" role="status" aria-live="polite">
        <CircleAlert :size="16" />
        <span>{{ captureIssue === 'privacy_mode_or_display_off' ? t('capturePrivacy') : (statusError || t('captureRetrying')) }}</span>
      </div>
      <div v-if="lastSessionError" class="session-notice" role="alert">
        <CircleAlert :size="16" />
        <span>{{ lastSessionError }}</span>
      </div>
      <div v-if="!statusActive" class="stopped-state" role="status">
        <WifiOff :size="24" />
        <span>{{ t('stopped') }}</span>
      </div>
    </section>

    <footer class="viewer-toolbar" :aria-label="t('toolbar')">
      <div class="tool-group" role="toolbar" :aria-label="t('view')">
        <button type="button" class="icon-button" :class="{ active: tool === 'view' }" :aria-pressed="tool === 'view'" :title="t('view')" :aria-label="t('view')" @click="tool = 'view'"><Eye :size="19" /></button>
        <button type="button" class="icon-button" :class="{ active: tool === 'laser' }" :aria-pressed="tool === 'laser'" :title="t('laser')" :aria-label="t('laser')" :disabled="!canAnnotate" @click="tool = 'laser'"><CircleDot :size="19" /></button>
        <button type="button" class="icon-button" :class="{ active: tool === 'arrow' }" :aria-pressed="tool === 'arrow'" :title="t('arrow')" :aria-label="t('arrow')" :disabled="!canAnnotate" @click="tool = 'arrow'"><ArrowUpRight :size="19" /></button>
        <button type="button" class="icon-button" :class="{ active: tool === 'rect' }" :aria-pressed="tool === 'rect'" :title="t('rectangle')" :aria-label="t('rectangle')" :disabled="!canAnnotate" @click="tool = 'rect'"><Square :size="18" /></button>
      </div>

      <div class="toolbar-divider" aria-hidden="true" />

      <div class="color-group" role="group" :aria-label="t('color')">
        <button
          v-for="swatch in colors"
          :key="swatch"
          type="button"
          class="color-button"
          :class="{ active: color === swatch }"
          :aria-label="`${t('color')}: ${t(colorLabelKeys[swatch])}`"
          :aria-pressed="color === swatch"
          :title="`${t('color')}: ${t(colorLabelKeys[swatch])}`"
          :disabled="!canAnnotate"
          @click="color = swatch"
        ><span :style="{ backgroundColor: swatch }" /></button>
      </div>

      <div class="toolbar-divider" aria-hidden="true" />

      <div class="width-group" role="group" :aria-label="t('lineWidth')">
        <button
          v-for="widthOption in annotationWidths"
          :key="widthOption"
          type="button"
          class="width-button"
          :class="{ active: annotationWidth === widthOption }"
          :aria-label="`${t('lineWidth')}: ${t(annotationWidthLabelKeys[widthOption])}`"
          :aria-pressed="annotationWidth === widthOption"
          :title="`${t('lineWidth')}: ${t(annotationWidthLabelKeys[widthOption])}`"
          :disabled="!canAnnotate"
          @click="annotationWidth = widthOption"
        ><span class="width-swatch" :style="{ height: `${widthOption}px` }" /></button>
      </div>

      <div class="toolbar-divider" aria-hidden="true" />

      <div class="tool-group" role="toolbar" :aria-label="t('undo')">
        <button type="button" class="icon-button" :title="t('undo')" :aria-label="t('undo')" :disabled="!hasOwnPersistentAnnotations || !interactionConnected" @click="undoOwn"><Undo2 :size="19" /></button>
        <button type="button" class="icon-button" :title="t('clearOwn')" :aria-label="t('clearOwn')" :disabled="!hasOwnAnnotations || !interactionConnected" @click="clearOwn"><Eraser :size="19" /></button>
      </div>

      <div class="toolbar-spacer" />

      <label class="refresh-control" :title="t('refresh')">
        <RefreshCw :size="15" aria-hidden="true" />
        <select v-model.number="refreshRateMs" :aria-label="t('refresh')" @change="onRefreshRateChange">
          <option v-for="option in refreshOptions" :key="option.value" :value="option.value">{{ t(option.labelKey) }}</option>
        </select>
      </label>

      <div class="tool-group" role="toolbar" :aria-label="t('freeze')">
        <button type="button" class="icon-button" :class="{ active: sharedFrozen }" :title="sharedFrozen ? t('resumeShared') : t('freeze')" :aria-label="sharedFrozen ? t('resumeShared') : t('freeze')" :aria-pressed="sharedFrozen" :disabled="!canFreeze && !canResumeShared" @click="toggleSharedFreeze">
          <Radio v-if="sharedFrozen" :size="19" />
          <Snowflake v-else :size="19" />
        </button>
        <button type="button" class="icon-button" :class="{ active: localPaused }" :title="localPaused ? t('resumeLocal') : t('pauseLocal')" :aria-label="localPaused ? t('resumeLocal') : t('pauseLocal')" :aria-pressed="localPaused" :disabled="!streamConnected" @click="toggleLocalPause">
          <Play v-if="localPaused" :size="19" />
          <Pause v-else :size="19" />
        </button>
        <button type="button" class="icon-button" :title="isFullscreen ? t('exitFullscreen') : t('fullscreen')" :aria-label="isFullscreen ? t('exitFullscreen') : t('fullscreen')" :aria-pressed="isFullscreen" @click="toggleFullscreen">
          <Minimize2 v-if="isFullscreen" :size="19" />
          <Maximize2 v-else :size="19" />
        </button>
      </div>
    </footer>
  </main>
</template>
