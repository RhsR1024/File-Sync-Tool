<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import {
  ArrowUpRight,
  Check,
  CircleAlert,
  CircleDot,
  Eraser,
  Eye,
  Maximize2,
  Minimize2,
  MousePointer2,
  MonitorPlay,
  Pencil,
  Pause,
  Play,
  Radio,
  RefreshCw,
  Square,
  Snowflake,
  Trash2,
  Undo2,
  Wifi,
  WifiOff,
  ShieldCheck,
} from 'lucide-vue-next';

import AnnotationOverlay from './components/AnnotationOverlay.vue';
import RemoteControlOverlay from './components/RemoteControlOverlay.vue';
import { applyAnnotationApplied, applySnapshot, applyViewState, emptyDocument, normalizeDocument, resetForSource, visibleShapes } from './lib/annotation-state';
import { computeContainedRect, type ContainedRect } from './lib/coordinates';
import { MseH264Player, type MsePlayerState } from './lib/mse-player';
import {
  canHandleRemoteInput,
  decideRemoteKeyboardAction,
  mergeControlSnapshot,
  mergeHttpControlSnapshot,
  type ScreenShareTool,
} from './lib/remote-control';
import { ScreenShareSessionClient } from './lib/session-client';
import { detectLocale, messages, type ScreenShareLocale } from './messages';
import type {
  AnnotationAddPayload,
  AnnotationDocument,
  AnnotationShape,
  AnnotationUpdatePayload,
  ControlState,
  ControlStateSnapshot,
  ScreenShareHttpStatus,
  SessionConnectionState,
  SessionServerMessage,
} from './types';

type Tool = ScreenShareTool;

const locale = ref<ScreenShareLocale>(detectLocale());
const t = (key: keyof typeof messages.en): string => messages[locale.value][key] as string;

const stage = ref<HTMLElement | null>(null);
const screenImage = ref<HTMLImageElement | null>(null);
const screenVideo = ref<HTMLVideoElement | null>(null);
const imageSource = ref('/stream');
const imageLoadError = ref(false);
const imageReady = ref(false);
const naturalWidth = ref(0);
const naturalHeight = ref(0);
const geometry = ref<ContainedRect>(computeContainedRect(0, 0, 0, 0));
const mjpegConnected = ref(false);
let streamRetryTimer: number | null = null;
const streamRetryAttempt = ref(0);
const localPaused = ref(false);
const localFrameUrl = ref<string | null>(null);
const isFullscreen = ref(false);
const httpStatus = ref<ScreenShareHttpStatus>({});
const statusError = ref<string | null>(null);
const lastSessionError = ref<string | null>(null);
const viewerTick = ref(Date.now());
const h264PlayerState = ref<MsePlayerState>({
  status: 'idle',
  attempts: 0,
  lastError: null,
  width: 0,
  height: 0,
});
const h264DisabledForSession = ref(false);
let lastMediaSessionId = 0;

const documentState = ref<AnnotationDocument>(emptyDocument());
const clientId = ref('');
const annotationsEnabled = ref(true);
const sharedFreezeEnabled = ref(true);
const controlRequestsEnabled = ref(false);
const keyboardControlEnabled = ref(false);
const controlState = ref<ControlState>('disabled');
const controlSnapshot = ref<ControlStateSnapshot>({ state: 'disabled' });
const sessionState = ref<SessionConnectionState>({ status: 'idle', attempts: 0, lastError: null });
const tool = ref<Tool>('view');
const editMode = ref(false);
const selectedAnnotationId = ref<string | null>(null);
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
const controlStateLabelKeys: Record<ControlState, keyof typeof messages.en> = {
  disabled: 'controlDisabled',
  available: 'controlAvailable',
  requested: 'controlRequested',
  granted: 'controlGranted',
  revoked: 'controlRevoked',
};

const sessionClient = new ScreenShareSessionClient();
const h264Player = new MseH264Player();
let statusTimer: number | null = null;
let laserTimer: number | null = null;
let sessionNoticeTimer: number | null = null;
let refreshTimer: number | null = null;
let singleRefreshInFlight = false;
let resizeObserver: ResizeObserver | null = null;
let stopH264StateListener: (() => void) | null = null;
const forwardedKeys = new Set<string>();

const statusActive = computed(() => httpStatus.value.active ?? httpStatus.value.is_active ?? true);
const h264Desired = computed(() => (
  statusActive.value
  && httpStatus.value.transport === 'mse_h264'
  && !h264DisabledForSession.value
));
const h264Ready = computed(() => h264PlayerState.value.status === 'ready');
const showH264Video = computed(() => (
  h264Desired.value
  && h264Ready.value
  && !localPaused.value
  && !sharedFrozen.value
));
const streamConnected = computed(() => (
  (h264Desired.value && h264Ready.value) || (mjpegConnected.value && imageReady.value)
));
const viewerCount = computed(() => httpStatus.value.viewers ?? httpStatus.value.viewer_count ?? 0);
const captureIssue = computed(() => httpStatus.value.capture_issue ?? null);
const visibleAnnotationShapes = computed(() => visibleShapes(documentState.value, viewerTick.value));
const hasOwnAnnotations = computed(() => Boolean(clientId.value)
  && visibleAnnotationShapes.value.some((shape) => shape.ownerClientId === clientId.value));
const ownPersistentAnnotations = computed(() => clientId.value
  ? documentState.value.shapes.filter((shape) => (
    shape.ownerClientId === clientId.value && shape.kind !== 'laser'
  ))
  : []);
const hasOwnPersistentAnnotations = computed(() => ownPersistentAnnotations.value.length > 0);
const latestOwnPersistentAnnotation = computed<AnnotationShape | null>(() => (
  ownPersistentAnnotations.value[ownPersistentAnnotations.value.length - 1] ?? null
));
const selectedAnnotation = computed<AnnotationShape | null>(() => {
  const selected = documentState.value.shapes.find((shape) => shape.id === selectedAnnotationId.value);
  if (!selected || selected.kind === 'laser' || selected.ownerClientId !== clientId.value) return null;
  return selected;
});
const selectedAnnotationKindLabel = computed(() => {
  if (selectedAnnotation.value?.kind === 'arrow') return t('arrow');
  if (selectedAnnotation.value?.kind === 'rect') return t('rectangle');
  return '';
});
const interactionConnected = computed(() => sessionState.value.status === 'connected');
const interactionLabel = computed(() => interactionConnected.value ? t('interactionConnected') : t('interactionOffline'));
const streamLabel = computed(() => {
  if (!statusActive.value) return t('stopped');
  if (captureIssue.value === 'privacy_mode_or_display_off') return t('capturePrivacy');
  if (captureIssue.value) return t('captureRetrying');
  if (streamConnected.value) return t('connected');
  if (imageLoadError.value || streamRetryAttempt.value > 0 || h264PlayerState.value.status === 'reconnecting') return t('reconnecting');
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
const isController = computed(() => controlState.value === 'granted'
  && controlSnapshot.value.controller_client_id === clientId.value);
const canRequestControl = computed(() => interactionConnected.value
  && controlRequestsEnabled.value
  && (controlState.value === 'available' || controlState.value === 'revoked')
  && !localPaused.value
  && !sharedFrozen.value);
const canReleaseControl = computed(() => interactionConnected.value && isController.value);
const remoteControlEnabled = computed(() => canHandleRemoteInput({
  tool: tool.value,
  isController: isController.value,
  connected: interactionConnected.value,
  localPaused: localPaused.value,
  sharedFrozen: sharedFrozen.value,
}));
const remoteKeyboardEnabled = computed(() => remoteControlEnabled.value && keyboardControlEnabled.value);
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
  && !isController.value
  && tool.value !== 'control'
));
const canEditAnnotations = computed(() => canAnnotate.value && hasOwnPersistentAnnotations.value);
const canFreeze = computed(() => interactionConnected.value && sharedFreezeEnabled.value && !sharedFrozen.value);
const canResumeShared = computed(() => interactionConnected.value && sharedFreezeEnabled.value && sharedFrozen.value);

function normalizeStatus(value: unknown): ScreenShareHttpStatus {
  return value && typeof value === 'object' ? value as ScreenShareHttpStatus : {};
}

function applyControlSnapshot(value: unknown) {
  if (!value || typeof value !== 'object') return;
  const next = value as ControlStateSnapshot;
  if (typeof next.state !== 'string') return;
  const state = next.state as ControlState;
  if (!['disabled', 'available', 'requested', 'granted', 'revoked'].includes(state)) return;
  controlSnapshot.value = mergeControlSnapshot(controlSnapshot.value, {
    state,
    request_id: typeof next.request_id === 'string' ? next.request_id : undefined,
    requester_client_id: typeof next.requester_client_id === 'string' ? next.requester_client_id : undefined,
    controller_client_id: typeof next.controller_client_id === 'string' ? next.controller_client_id : undefined,
    controller_ip: typeof next.controller_ip === 'string' ? next.controller_ip : undefined,
  });
  controlState.value = state;
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
    naturalWidth.value || screenImage.value?.naturalWidth || screenVideo.value?.videoWidth || 0,
    naturalHeight.value || screenImage.value?.naturalHeight || screenVideo.value?.videoHeight || 0,
    window.devicePixelRatio || 1,
  );
}

function markImageLoaded() {
  imageLoadError.value = false;
  imageReady.value = true;
  if (screenImage.value) {
    naturalWidth.value = screenImage.value.naturalWidth;
    naturalHeight.value = screenImage.value.naturalHeight;
  }
  singleRefreshInFlight = false;
  if (streamRetryTimer !== null) {
    window.clearTimeout(streamRetryTimer);
    streamRetryTimer = null;
  }
  mjpegConnected.value = true;
  streamRetryAttempt.value = 0;
  updateGeometry();
}

function setImageSource(source: string) {
  // Keep the media hidden until the replacement URL has produced a frame.
  // Otherwise browsers briefly paint their broken-image fallback between a
  // failed reconnect request and the following `error` event.
  imageReady.value = false;
  imageSource.value = source;
}

function handleH264PlayerState(state: MsePlayerState) {
  const wasReady = h264PlayerState.value.status === 'ready';
  h264PlayerState.value = state;
  if (state.status === 'ready') {
    naturalWidth.value = state.width;
    naturalHeight.value = state.height;
    mjpegConnected.value = false;
    imageReady.value = false;
    clearRefreshTimer();
    singleRefreshInFlight = false;
    if (streamRetryTimer !== null) {
      window.clearTimeout(streamRetryTimer);
      streamRetryTimer = null;
    }
    nextTick(updateGeometry);
    return;
  }
  if (state.status === 'unsupported' || state.status === 'error') {
    h264DisabledForSession.value = true;
  }
  if (wasReady && !localPaused.value && !sharedFrozen.value && statusActive.value) {
    mjpegConnected.value = false;
    setImageSource(`/stream?reconnect=1&t=${Date.now()}`);
  }
}

function ensureH264Playback() {
  if (!h264Desired.value) {
    if (!['idle', 'closed'].includes(h264PlayerState.value.status)) h264Player.stop();
    return;
  }
  const video = screenVideo.value;
  if (!video) return;
  if (['idle', 'closed'].includes(h264PlayerState.value.status)) h264Player.start(video);
}

function scheduleStreamReconnect() {
  if (localPaused.value || sharedFrozen.value || !statusActive.value || streamRetryTimer !== null) return;
  mjpegConnected.value = false;
  streamRetryAttempt.value += 1;
  const delay = Math.min(8000, 500 * 2 ** Math.min(streamRetryAttempt.value - 1, 4));
  streamRetryTimer = window.setTimeout(() => {
    streamRetryTimer = null;
    if (refreshRateMs.value > 0) requestSingleFrame();
    else setImageSource(`/stream?reconnect=1&t=${Date.now()}`);
  }, delay);
}

function handleImageError() {
  singleRefreshInFlight = false;
  mjpegConnected.value = false;
  imageReady.value = false;
  imageLoadError.value = true;
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
  setImageSource(`/stream?single=1&t=${Date.now()}`);
}

function startSingleFramePolling() {
  clearRefreshTimer();
  requestSingleFrame();
  refreshTimer = window.setInterval(requestSingleFrame, refreshRateMs.value);
}

function startLiveStream() {
  if (!statusActive.value || localPaused.value || sharedFrozen.value) return;
  ensureH264Playback();
  if (streamRetryTimer !== null) {
    window.clearTimeout(streamRetryTimer);
    streamRetryTimer = null;
  }
  if (refreshRateMs.value > 0) {
    startSingleFramePolling();
  } else {
    clearRefreshTimer();
    singleRefreshInFlight = false;
    setImageSource(`/stream?t=${Date.now()}`);
  }
}

function revokeLocalFrame() {
  if (localFrameUrl.value?.startsWith('blob:')) URL.revokeObjectURL(localFrameUrl.value);
  localFrameUrl.value = null;
}

function captureCurrentFrame(): string | null {
  const video = showH264Video.value ? screenVideo.value : null;
  const image = video ? null : screenImage.value;
  const width = video?.videoWidth || image?.naturalWidth || 0;
  const height = video?.videoHeight || image?.naturalHeight || 0;
  const source = video ?? image;
  if (!source || !width || !height) return null;
  try {
    const dpr = window.devicePixelRatio || 1;
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(width * dpr));
    canvas.height = Math.max(1, Math.round(height * dpr));
    const context = canvas.getContext('2d');
    if (!context) return null;
    context.scale(dpr, dpr);
    context.drawImage(source, 0, 0, width, height);
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
  setImageSource(captured);
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
  setImageSource(`${url}${url.includes('?') ? '&' : '?'}t=${Date.now()}`);
}

function applyDocument(next: unknown) {
  const current = documentState.value;
  const incoming = normalizeDocument(next, current);
  const sourceChanged = incoming.sourceEpoch !== current.sourceEpoch
    && current.sourceEpoch !== 0;
  const mediaStateChanged = incoming.sessionId !== current.sessionId
    || incoming.sourceEpoch !== current.sourceEpoch
    || incoming.mode !== current.mode
    || incoming.frozenFrameId !== current.frozenFrameId;
  documentState.value = applySnapshot(current, incoming);
  if (sourceChanged) showSessionNotice(t('sourceChanged'));
  sessionClient.updateContext(documentState.value.sessionId, documentState.value.sourceEpoch);
  viewerTick.value = Date.now();
  if (!localPaused.value && mediaStateChanged) {
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
    if (typeof features.control_requests_enabled === 'boolean') controlRequestsEnabled.value = features.control_requests_enabled;
    if (typeof features.keyboard_control_enabled === 'boolean') keyboardControlEnabled.value = features.keyboard_control_enabled;
    sessionClient.updateContext(message.session_id, message.source_epoch);
    if (documentState.value.sessionId === 0) {
      documentState.value = emptyDocument(message.session_id, message.source_epoch);
    }
    return;
  }
  if (message.type === 'session.snapshot') {
    const payload = (message.payload ?? {}) as Record<string, unknown>;
    applyDocument(payload.document ?? payload);
    applyControlSnapshot(payload.control);
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
    applyControlSnapshot(payload.control);
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
    applyControlSnapshot(payload.control);
    showSessionNotice(t('sourceChanged'));
    return;
  }
  if (message.type === 'control.requested') {
    controlState.value = 'requested';
    return;
  }
  if (message.type === 'control.state') {
    const payload = (message.payload ?? {}) as Record<string, unknown>;
    applyControlSnapshot(payload.control ?? payload);
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
    // The interaction WebSocket carries the authoritative control snapshot,
    // including the controller client id. Do not let a delayed, partial HTTP
    // status response overwrite it while the socket is connected.
    if (nextStatus.control_state) {
      const fallback = mergeHttpControlSnapshot(
        controlSnapshot.value,
        nextStatus.control_state,
        nextStatus.controller_ip,
        interactionConnected.value,
      );
      if (fallback) applyControlSnapshot(fallback);
    }
    statusError.value = null;
    const sessionId = typeof nextStatus.session_id === 'number' ? nextStatus.session_id : documentState.value.sessionId;
    const sourceEpoch = typeof nextStatus.source_epoch === 'number' ? nextStatus.source_epoch : documentState.value.sourceEpoch;
    if (sessionId !== lastMediaSessionId) {
      lastMediaSessionId = sessionId;
      h264DisabledForSession.value = false;
      h264Player.stop();
    }
    if (sessionId !== documentState.value.sessionId || sourceEpoch !== documentState.value.sourceEpoch) {
      documentState.value = emptyDocument(sessionId, sourceEpoch);
      sessionClient.updateContext(sessionId, sourceEpoch);
    }
    if (statusActive.value && sessionState.value.status === 'idle') sessionClient.connect(sessionId, sourceEpoch);
    await nextTick();
    ensureH264Playback();
    if (!statusActive.value) {
      mjpegConnected.value = false;
      h264Player.stop();
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

function selectTool(nextTool: Tool) {
  editMode.value = false;
  selectedAnnotationId.value = null;
  tool.value = nextTool;
}

function selectAnnotation(shapeId: string | null) {
  if (!shapeId) {
    selectedAnnotationId.value = null;
    return;
  }
  const shape = documentState.value.shapes.find((item) => item.id === shapeId);
  if (!shape || shape.kind === 'laser' || shape.ownerClientId !== clientId.value) return;
  selectedAnnotationId.value = shape.id;
  color.value = shape.color;
  annotationWidth.value = shape.width;
}

function toggleEditMode() {
  if (editMode.value) {
    editMode.value = false;
    selectedAnnotationId.value = null;
    return;
  }
  if (!canEditAnnotations.value) return;
  const latest = latestOwnPersistentAnnotation.value;
  if (!latest) return;
  tool.value = 'view';
  editMode.value = true;
  selectAnnotation(latest.id);
}

function updateAnnotation(payload: AnnotationUpdatePayload) {
  if (!canAnnotate.value || payload.shape_id !== selectedAnnotationId.value) return;
  sessionClient.send('annotation.update', payload);
}

function updateSelectedStyle(nextColor = color.value, nextWidth = annotationWidth.value) {
  const selected = selectedAnnotation.value;
  if (!editMode.value || !selected) return;
  sessionClient.send('annotation.update', {
    shape_id: selected.id,
    points: selected.points,
    color: nextColor,
    width: nextWidth,
  } satisfies AnnotationUpdatePayload);
}

function setAnnotationColor(nextColor: string) {
  color.value = nextColor;
  updateSelectedStyle(nextColor, annotationWidth.value);
}

function setAnnotationWidth(nextWidth: number) {
  annotationWidth.value = nextWidth;
  updateSelectedStyle(color.value, nextWidth);
}

function removeSelectedAnnotation() {
  const selected = selectedAnnotation.value;
  if (!selected || !interactionConnected.value) return;
  sessionClient.send('annotation.remove', { shape_id: selected.id });
  selectedAnnotationId.value = null;
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

function requestControl() {
  if (!canRequestControl.value) return;
  selectTool('control');
  if (!sessionClient.send('control.request')) {
    selectTool('view');
    showSessionNotice(t('interactionOffline'));
    reconnectInteraction();
    return;
  }
  controlState.value = 'requested';
}

function releaseControl() {
  if (!canReleaseControl.value) return;
  releaseForwardedKeys();
  sessionClient.send('control.release');
  selectTool('view');
}

function sendPointerMove(point: { x: number; y: number }) {
  if (!remoteControlEnabled.value) return;
  sessionClient.send('input.pointer_move', point);
}

function sendPointerButton(payload: { button: 'left' | 'right'; pressed: boolean }) {
  if (!remoteControlEnabled.value) return;
  sessionClient.send('input.pointer_button', payload);
}

function sendWheel(deltaY: number) {
  if (!remoteControlEnabled.value) return;
  sessionClient.send('input.wheel', { delta_y: deltaY });
}

function isLocalControlTarget(target: EventTarget | null): boolean {
  return target instanceof HTMLInputElement
    || target instanceof HTMLTextAreaElement
    || target instanceof HTMLSelectElement
    || target instanceof HTMLButtonElement
    || (target instanceof HTMLElement && target.isContentEditable);
}

function releaseForwardedKeys(force = false) {
  const shouldSend = force || forwardedKeys.size > 0;
  forwardedKeys.clear();
  if (shouldSend && interactionConnected.value) {
    sessionClient.send('input.release_all');
  }
}

function forwardRemoteKeyboardEvent(event: KeyboardEvent, pressed: boolean): boolean {
  if (!remoteKeyboardEnabled.value || isLocalControlTarget(event.target)) return false;
  const action = decideRemoteKeyboardAction({
    code: event.code,
    pressed,
    metaKey: event.metaKey,
    composing: event.isComposing,
  }, forwardedKeys);
  if (action.type === 'ignore') return false;

  event.preventDefault();
  if (action.type === 'release_all') {
    releaseForwardedKeys(true);
    return true;
  }

  const sent = sessionClient.send('input.key', {
    code: action.code,
    pressed: action.pressed,
  });
  if (sent) {
    if (action.pressed) forwardedKeys.add(action.code);
    else forwardedKeys.delete(action.code);
  }
  return true;
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
  releaseForwardedKeys();
  sessionClient.close();
  sessionClient.connect(documentState.value.sessionId, documentState.value.sourceEpoch);
  h264DisabledForSession.value = false;
  if (statusActive.value && httpStatus.value.transport === 'mse_h264' && screenVideo.value) {
    h264Player.start(screenVideo.value);
  }
}

function onVisibilityChange() {
  if (document.hidden) {
    releaseForwardedKeys();
    return;
  }
  if (statusActive.value) refreshStatus();
}

function onWindowBlur() {
  releaseForwardedKeys();
}

watch([naturalWidth, naturalHeight], updateGeometry);
watch([controlState, isController], () => {
  if (tool.value === 'control' && !isController.value && controlState.value !== 'requested') {
    tool.value = 'view';
  }
});

watch(remoteKeyboardEnabled, (enabled) => {
  if (!enabled) releaseForwardedKeys();
});

watch(selectedAnnotation, (shape) => {
  if (!shape) selectedAnnotationId.value = null;
});

watch(hasOwnPersistentAnnotations, (hasAny) => {
  if (!hasAny && editMode.value) {
    editMode.value = false;
    selectedAnnotationId.value = null;
  }
});

function onKeydown(event: KeyboardEvent) {
  if (forwardRemoteKeyboardEvent(event, true)) return;
  const target = event.target;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || (target instanceof HTMLElement && target.isContentEditable)) return;
  if (!isController.value && (event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 'z' && hasOwnPersistentAnnotations.value) {
    event.preventDefault();
    undoOwn();
    return;
  }
  if (event.key === 'Escape' && editMode.value) {
    event.preventDefault();
    toggleEditMode();
    return;
  }
  if (editMode.value && (event.key === 'Delete' || event.key === 'Backspace') && selectedAnnotation.value) {
    event.preventDefault();
    removeSelectedAnnotation();
  }
}

function onKeyup(event: KeyboardEvent) {
  forwardRemoteKeyboardEvent(event, false);
}

onMounted(async () => {
  document.documentElement.lang = locale.value === 'zh' ? 'zh-CN' : 'en';
  document.title = t('title');
  sessionClient.onMessage(handleSessionMessage);
  sessionClient.onState((state) => { sessionState.value = state; });
  stopH264StateListener = h264Player.onState(handleH264PlayerState);
  document.addEventListener('fullscreenchange', updateFullscreenState);
  document.addEventListener('visibilitychange', onVisibilityChange);
  document.addEventListener('keydown', onKeydown);
  document.addEventListener('keyup', onKeyup);
  window.addEventListener('blur', onWindowBlur);
  window.addEventListener('pagehide', onWindowBlur);
  resizeObserver = new ResizeObserver(updateGeometry);
  if (stage.value) resizeObserver.observe(stage.value);
  laserTimer = window.setInterval(() => { viewerTick.value = Date.now(); }, 250);
  statusTimer = window.setInterval(refreshStatus, 3000);
  await refreshStatus();
  if (sessionState.value.status === 'idle') sessionClient.connect(documentState.value.sessionId, documentState.value.sourceEpoch);
});

onUnmounted(() => {
  releaseForwardedKeys();
  sessionClient.close();
  h264Player.stop();
  stopH264StateListener?.();
  stopH264StateListener = null;
  if (statusTimer !== null) window.clearInterval(statusTimer);
  if (laserTimer !== null) window.clearInterval(laserTimer);
  if (streamRetryTimer !== null) window.clearTimeout(streamRetryTimer);
  clearRefreshTimer();
  singleRefreshInFlight = false;
  if (sessionNoticeTimer !== null) window.clearTimeout(sessionNoticeTimer);
  resizeObserver?.disconnect();
  document.removeEventListener('fullscreenchange', updateFullscreenState);
  document.removeEventListener('visibilitychange', onVisibilityChange);
  document.removeEventListener('keydown', onKeydown);
  document.removeEventListener('keyup', onKeyup);
  window.removeEventListener('blur', onWindowBlur);
  window.removeEventListener('pagehide', onWindowBlur);
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
        <span v-if="controlRequestsEnabled" class="interaction-status" :class="{ offline: controlState !== 'granted' }" aria-live="polite">
          <MousePointer2 :size="13" aria-hidden="true" />
          {{ t(controlStateLabelKeys[controlState]) }}
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
      <video
        ref="screenVideo"
        class="screen-image"
        :class="{ 'is-hidden-media': !showH264Video }"
        muted
        autoplay
        playsinline
      />
      <img
        v-if="!showH264Video && statusActive"
        ref="screenImage"
        class="screen-image"
        :class="{ 'is-image-error': !imageReady }"
        :src="imageSource"
        alt=""
        aria-hidden="true"
        draggable="false"
        @load="markImageLoaded"
        @error="handleImageError"
      />
      <div v-if="statusActive && !showH264Video && !imageReady" class="stream-empty-state" role="status" aria-live="polite">
        <WifiOff :size="28" aria-hidden="true" />
        <span>{{ streamLabel }}</span>
      </div>
      <AnnotationOverlay
        :shapes="visibleAnnotationShapes"
        :geometry="geometry"
        :tool="tool"
        :color="color"
        :width="annotationWidth"
        :enabled="canAnnotate"
        :edit-mode="editMode"
        :selected-id="selectedAnnotationId"
        :client-id="clientId"
        @add="addAnnotation"
        @select="selectAnnotation"
        @update="updateAnnotation"
      />
      <RemoteControlOverlay
        :geometry="geometry"
        :enabled="remoteControlEnabled"
        @move="sendPointerMove"
        @button="sendPointerButton"
        @wheel="sendWheel"
      />
      <div v-if="editMode" class="annotation-edit-bar" role="status" aria-live="polite">
        <Pencil :size="17" aria-hidden="true" />
        <span class="annotation-edit-label">
          {{ selectedAnnotation ? `${t('selectedAnnotation')}: ${selectedAnnotationKindLabel}` : t('editAnnotations') }}
        </span>
        <button
          type="button"
          class="annotation-edit-action danger"
          :disabled="!selectedAnnotation || !interactionConnected"
          :title="t('removeSelectedAnnotation')"
          @click="removeSelectedAnnotation"
        >
          <Trash2 :size="17" aria-hidden="true" />
          <span>{{ t('deleteShort') }}</span>
        </button>
        <button
          type="button"
          class="annotation-edit-action"
          :title="t('finishEditing')"
          @click="toggleEditMode"
        >
          <Check :size="17" aria-hidden="true" />
          <span>{{ t('finishEditing') }}</span>
        </button>
      </div>
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
        <button type="button" class="icon-button" :class="{ active: tool === 'view' && !editMode }" :aria-pressed="tool === 'view' && !editMode" :title="t('view')" :aria-label="t('view')" @click="selectTool('view')"><Eye :size="19" /></button>
        <button type="button" class="icon-button" :class="{ active: editMode }" :aria-pressed="editMode" :title="t('editAnnotations')" :aria-label="t('editAnnotations')" :disabled="!canEditAnnotations && !editMode" @click="toggleEditMode"><Pencil :size="18" /></button>
        <button type="button" class="icon-button" :class="{ active: tool === 'laser' && !editMode }" :aria-pressed="tool === 'laser' && !editMode" :title="t('laser')" :aria-label="t('laser')" :disabled="!canAnnotate" @click="selectTool('laser')"><CircleDot :size="19" /></button>
        <button type="button" class="icon-button" :class="{ active: tool === 'arrow' && !editMode }" :aria-pressed="tool === 'arrow' && !editMode" :title="t('arrow')" :aria-label="t('arrow')" :disabled="!canAnnotate" @click="selectTool('arrow')"><ArrowUpRight :size="19" /></button>
        <button type="button" class="icon-button" :class="{ active: tool === 'rect' && !editMode }" :aria-pressed="tool === 'rect' && !editMode" :title="t('rectangle')" :aria-label="t('rectangle')" :disabled="!canAnnotate" @click="selectTool('rect')"><Square :size="18" /></button>
      </div>

      <div class="toolbar-divider" aria-hidden="true" />

      <div class="tool-group annotation-actions" role="toolbar" :aria-label="t('annotationActions')">
        <button type="button" class="toolbar-action-button" :title="t('undo')" :aria-label="t('undo')" :disabled="!hasOwnPersistentAnnotations || !interactionConnected" @click="undoOwn">
          <Undo2 :size="18" aria-hidden="true" />
          <span>{{ t('undoShort') }}</span>
        </button>
        <button type="button" class="toolbar-action-button" :title="t('clearOwn')" :aria-label="t('clearOwn')" :disabled="!hasOwnAnnotations || !interactionConnected" @click="clearOwn">
          <Eraser :size="18" aria-hidden="true" />
          <span>{{ t('clearOwnShort') }}</span>
        </button>
        <button type="button" class="toolbar-action-button danger" :title="t('removeSelectedAnnotation')" :aria-label="t('removeSelectedAnnotation')" :disabled="!selectedAnnotation || !interactionConnected" @click="removeSelectedAnnotation">
          <Trash2 :size="17" aria-hidden="true" />
          <span>{{ t('deleteShort') }}</span>
        </button>
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
          @click="setAnnotationColor(swatch)"
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
          @click="setAnnotationWidth(widthOption)"
        ><span class="width-swatch" :style="{ height: `${widthOption}px` }" /></button>
      </div>

      <div class="toolbar-divider" aria-hidden="true" />

      <div class="tool-group" role="toolbar" :aria-label="t('control')">
        <button
          v-if="!isController"
          type="button"
          class="icon-button"
          :class="{ active: controlState === 'requested' }"
          :title="controlState === 'requested' ? t('controlRequested') : t('requestControl')"
          :aria-label="controlState === 'requested' ? t('controlRequested') : t('requestControl')"
          :aria-pressed="controlState === 'requested'"
          :disabled="!canRequestControl && controlState !== 'requested'"
          @click="requestControl"
        ><MousePointer2 :size="19" /></button>
        <button
          v-else
          type="button"
          class="icon-button active"
          :title="t('releaseControl')"
          :aria-label="t('releaseControl')"
          @click="releaseControl"
        ><ShieldCheck :size="19" /></button>
      </div>

      <div class="toolbar-spacer" />

      <label class="refresh-control" :title="t('refresh')">
        <RefreshCw :size="15" aria-hidden="true" />
        <select v-model.number="refreshRateMs" :aria-label="t('refresh')" :disabled="showH264Video" @change="onRefreshRateChange">
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
