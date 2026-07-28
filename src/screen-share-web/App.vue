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
  RefreshCw,
  Square,
  Trash2,
  Undo2,
  Wifi,
  WifiOff,
  ShieldCheck,
} from 'lucide-vue-next';

import AnnotationOverlay from './components/AnnotationOverlay.vue';
import RemoteControlOverlay from './components/RemoteControlOverlay.vue';
import { applyAnnotationApplied, applySnapshot, emptyDocument, normalizeDocument, resetForSource, visibleShapes } from './lib/annotation-state';
import { computeContainedRect, type ContainedRect } from './lib/coordinates';
import { installScreenShareDiagnostics } from './lib/diagnostics';
import { MseH264Player, type MsePlayerState } from './lib/mse-player';
import {
  canHandleRemoteInput,
  decideRemoteKeyboardAction,
  mergeControlSnapshot,
  mergeHttpControlSnapshot,
  type ScreenShareTool,
} from './lib/remote-control';
import { ScreenShareSessionClient } from './lib/session-client';
import {
  WebCodecsH264Player,
  type WebCodecsPlayerState,
} from './lib/webcodecs-player';
import { WebRtcH264Player, type WebRtcPlayerState } from './lib/webrtc-player';
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
const screenCanvas = ref<HTMLCanvasElement | null>(null);
const imageSource = ref('/stream');
const imageLoadError = ref(false);
const imageReady = ref(false);
const naturalWidth = ref(0);
const naturalHeight = ref(0);
const geometry = ref<ContainedRect>(computeContainedRect(0, 0, 0, 0));
const mjpegConnected = ref(false);
let streamRetryTimer: number | null = null;
const streamRetryAttempt = ref(0);
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
const webCodecsPlayerState = ref<WebCodecsPlayerState>({
  status: 'idle',
  failureKind: null,
  attempts: 0,
  lastError: null,
  width: 0,
  height: 0,
});
const webRtcPlayerState = ref<WebRtcPlayerState>('idle');
let lastMediaSessionId = 0;

const documentState = ref<AnnotationDocument>(emptyDocument());
const clientId = ref('');
const annotationsEnabled = ref(true);
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
const webCodecsPlayer = new WebCodecsH264Player();
let webRtcPlayer: WebRtcH264Player | null = null;
let statusTimer: number | null = null;
let laserTimer: number | null = null;
let sessionNoticeTimer: number | null = null;
let resizeObserver: ResizeObserver | null = null;
let stopH264StateListener: (() => void) | null = null;
let stopWebCodecsStateListener: (() => void) | null = null;
let stopInputTraceListener: (() => void) | null = null;
let uninstallDiagnostics: (() => void) | null = null;
const forwardedKeys = new Set<string>();

const statusActive = computed(() => httpStatus.value.active ?? httpStatus.value.is_active ?? true);
const h264Desired = computed(() => (
  statusActive.value
  && httpStatus.value.transport === 'mse_h264'
  && !h264DisabledForSession.value
));
const webCodecsDesired = computed(() => (
  statusActive.value
  && httpStatus.value.transport === 'web_codecs'
  && !h264DisabledForSession.value
));
const webRtcDesired = computed(() => (
  statusActive.value
  && httpStatus.value.transport === 'web_rtc'
  && !h264DisabledForSession.value
));
const h264Ready = computed(() => h264PlayerState.value.status === 'ready');
const showH264Video = computed(() => (
  (h264Desired.value && h264Ready.value)
  || (webRtcDesired.value && webRtcPlayerState.value === 'ready')
));
const showWebCodecsCanvas = computed(() => (
  webCodecsDesired.value && webCodecsPlayerState.value.status === 'ready'
));
const showPrimaryMedia = computed(() => showH264Video.value || showWebCodecsCanvas.value);
const streamConnected = computed(() => (
  showPrimaryMedia.value || (mjpegConnected.value && imageReady.value)
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
  if (
    imageLoadError.value
    || streamRetryAttempt.value > 0
    || h264PlayerState.value.status === 'reconnecting'
    || webCodecsPlayerState.value.status === 'reconnecting'
    || webRtcPlayerState.value === 'connecting'
    || webRtcPlayerState.value === 'connected'
    || webRtcPlayerState.value === 'disconnected'
  ) return t('reconnecting');
  if (sessionState.value.status === 'reconnecting' || sessionState.value.status === 'connecting') return t('reconnecting');
  return t('noConnection');
});
const streamDotClass = computed(() => {
  if (!statusActive.value || sessionState.value.status === 'closed') return 'is-off';
  if (captureIssue.value === 'privacy_mode_or_display_off') return 'is-warn';
  if (streamConnected.value) return 'is-on';
  return 'is-retry';
});
const isController = computed(() => controlState.value === 'granted'
  && controlSnapshot.value.controller_client_id === clientId.value);
const canRequestControl = computed(() => (
  interactionConnected.value
  && controlRequestsEnabled.value
  && (controlState.value === 'available' || controlState.value === 'revoked')
));
const canReleaseControl = computed(() => interactionConnected.value && isController.value);
const remoteControlEnabled = computed(() => canHandleRemoteInput({
  tool: tool.value,
  isController: isController.value,
  connected: interactionConnected.value,
  localPaused: false,
  sharedFrozen: false,
}));
const remoteKeyboardEnabled = computed(() => remoteControlEnabled.value && keyboardControlEnabled.value);
const canAnnotate = computed(() => (
  interactionConnected.value
  && annotationsEnabled.value
  && !isController.value
  && tool.value !== 'control'
));
const canEditAnnotations = computed(() => canAnnotate.value && hasOwnPersistentAnnotations.value);

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
    naturalWidth.value || screenImage.value?.naturalWidth || screenVideo.value?.videoWidth || screenCanvas.value?.width || 0,
    naturalHeight.value || screenImage.value?.naturalHeight || screenVideo.value?.videoHeight || screenCanvas.value?.height || 0,
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
  if (wasReady && statusActive.value) {
    mjpegConnected.value = false;
    setImageSource(`/stream?reconnect=1&t=${Date.now()}`);
  }
}

function handleWebCodecsPlayerState(state: WebCodecsPlayerState) {
  const wasReady = webCodecsPlayerState.value.status === 'ready';
  webCodecsPlayerState.value = state;
  if (state.status === 'ready') {
    naturalWidth.value = state.width;
    naturalHeight.value = state.height;
    mjpegConnected.value = false;
    imageReady.value = false;
    nextTick(updateGeometry);
    return;
  }
  if (
    state.status === 'unsupported'
    || (state.status === 'error' && state.failureKind === 'fatal')
  ) {
    h264DisabledForSession.value = true;
    if (state.lastError) showSessionNotice(`${state.lastError}; falling back to MJPEG`);
  }
  if (wasReady && statusActive.value) {
    mjpegConnected.value = false;
    setImageSource(`/stream?reconnect=1&t=${Date.now()}`);
  }
}

function handleWebRtcPlayerState(state: WebRtcPlayerState) {
  const wasReady = webRtcPlayerState.value === 'ready';
  webRtcPlayerState.value = state;
  if (state === 'ready') {
    mjpegConnected.value = false;
    imageReady.value = false;
    nextTick(updateGeometry);
    return;
  }
  if (state === 'failed') {
    h264DisabledForSession.value = true;
    showSessionNotice('WebRTC prototype failed; falling back to MJPEG');
  }
  if (wasReady && statusActive.value) {
    mjpegConnected.value = false;
    setImageSource(`/stream?reconnect=1&t=${Date.now()}`);
  }
}

function stopInactiveMediaPlayers(active: 'mse_h264' | 'web_codecs' | 'web_rtc' | null) {
  if (active !== 'mse_h264' && !['idle', 'closed'].includes(h264PlayerState.value.status)) {
    h264Player.stop();
  }
  if (active !== 'web_codecs' && !['idle', 'closed'].includes(webCodecsPlayerState.value.status)) {
    webCodecsPlayer.stop();
  }
  if (active !== 'web_rtc' && webRtcPlayer) {
    webRtcPlayer.stop();
    webRtcPlayer = null;
    webRtcPlayerState.value = 'closed';
  }
}

function ensureMediaPlayback() {
  if (!h264Desired.value) {
    if (!['idle', 'closed'].includes(h264PlayerState.value.status)) h264Player.stop();
  } else {
    stopInactiveMediaPlayers('mse_h264');
    const video = screenVideo.value;
    if (video && ['idle', 'closed'].includes(h264PlayerState.value.status)) h264Player.start(video);
    return;
  }
  if (webCodecsDesired.value) {
    stopInactiveMediaPlayers('web_codecs');
    const canvas = screenCanvas.value;
    if (canvas && ['idle', 'closed'].includes(webCodecsPlayerState.value.status)) {
      webCodecsPlayer.start(canvas);
    }
    return;
  }
  if (webRtcDesired.value) {
    stopInactiveMediaPlayers('web_rtc');
    const video = screenVideo.value;
    if (video && (!webRtcPlayer || ['idle', 'closed'].includes(webRtcPlayerState.value))) {
      webRtcPlayer?.stop();
      webRtcPlayer = new WebRtcH264Player(video, { onStateChange: handleWebRtcPlayerState });
      void webRtcPlayer.start().catch((error: unknown) => {
        showSessionNotice(`${error instanceof Error ? error.message : String(error)}; falling back to MJPEG`);
      });
    }
    return;
  }
  stopInactiveMediaPlayers(null);
}

function handleVideoMetadata() {
  if (!screenVideo.value) return;
  naturalWidth.value = screenVideo.value.videoWidth;
  naturalHeight.value = screenVideo.value.videoHeight;
  updateGeometry();
}

function scheduleStreamReconnect() {
  if (!statusActive.value || streamRetryTimer !== null) return;
  mjpegConnected.value = false;
  streamRetryAttempt.value += 1;
  const delay = Math.min(8000, 500 * 2 ** Math.min(streamRetryAttempt.value - 1, 4));
  streamRetryTimer = window.setTimeout(() => {
    streamRetryTimer = null;
    setImageSource(`/stream?reconnect=1&t=${Date.now()}`);
  }, delay);
}

function handleImageError() {
  mjpegConnected.value = false;
  imageReady.value = false;
  imageLoadError.value = true;
  scheduleStreamReconnect();
}

function startLiveStream() {
  if (!statusActive.value) return;
  ensureMediaPlayback();
  if (streamRetryTimer !== null) {
    window.clearTimeout(streamRetryTimer);
    streamRetryTimer = null;
  }
  setImageSource(`/stream?t=${Date.now()}`);
}

function applyDocument(next: unknown) {
  const current = documentState.value;
  const normalized = normalizeDocument(next, current);
  // Freeze is no longer a viewer feature. Continue accepting legacy document
  // fields for protocol compatibility, but keep media and collaboration live.
  const incoming: AnnotationDocument = {
    ...normalized,
    mode: 'live',
    frozenFrameId: null,
  };
  const sourceChanged = incoming.sourceEpoch !== current.sourceEpoch
    && current.sourceEpoch !== 0;
  const mediaStateChanged = incoming.sessionId !== current.sessionId
    || incoming.sourceEpoch !== current.sourceEpoch;
  documentState.value = applySnapshot(current, incoming);
  if (sourceChanged) showSessionNotice(t('sourceChanged'));
  sessionClient.updateContext(documentState.value.sessionId, documentState.value.sourceEpoch);
  viewerTick.value = Date.now();
  if (mediaStateChanged) startLiveStream();
}

function handleSessionMessage(message: SessionServerMessage) {
  if (message.type === 'session.hello') {
    const payload = (message.payload ?? {}) as Record<string, unknown>;
    if (typeof payload.client_id === 'string') clientId.value = payload.client_id;
    const features = payload.features && typeof payload.features === 'object'
      ? payload.features as Record<string, unknown>
      : {};
    if (typeof features.annotations_enabled === 'boolean') annotationsEnabled.value = features.annotations_enabled;
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
    const text = typeof payload.message === 'string' ? payload.message : t('sessionError');
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
      webCodecsPlayer.stop();
      webRtcPlayer?.stop();
      webRtcPlayer = null;
    }
    if (sessionId !== documentState.value.sessionId || sourceEpoch !== documentState.value.sourceEpoch) {
      documentState.value = emptyDocument(sessionId, sourceEpoch);
      sessionClient.updateContext(sessionId, sourceEpoch);
    }
    if (statusActive.value && sessionState.value.status === 'idle') sessionClient.connect(sessionId, sourceEpoch);
    await nextTick();
    ensureMediaPlayback();
    if (!statusActive.value) {
      mjpegConnected.value = false;
      h264Player.stop();
      webCodecsPlayer.stop();
      webRtcPlayer?.stop();
      webRtcPlayer = null;
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

function sendPointerMove(point: { x: number; y: number }, eventOccurredAtMs: number) {
  if (!remoteControlEnabled.value) return;
  sessionClient.send('input.pointer_move', point, eventOccurredAtMs);
}

function sendPointerButton(
  payload: { button: 'left' | 'right'; pressed: boolean },
  eventOccurredAtMs?: number,
) {
  if (!remoteControlEnabled.value) return;
  sessionClient.send('input.pointer_button', payload, eventOccurredAtMs);
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
  stopInactiveMediaPlayers(null);
  ensureMediaPlayback();
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
  stopInputTraceListener = sessionClient.onInputTrace((event) => {
    if (event.phase === 'sent') {
      h264Player.recordInputTrace(event.clientSequence, event.occurredAtClientUnixMs);
      webCodecsPlayer.recordInputTrace(event.clientSequence, event.occurredAtClientUnixMs);
    } else {
      h264Player.recordInputQueueAcknowledged(event.clientSequence, event.observedAtClientUnixMs);
      webCodecsPlayer.recordInputQueueAcknowledged(event.clientSequence, event.observedAtClientUnixMs);
    }
  });
  stopH264StateListener = h264Player.onState(handleH264PlayerState);
  stopWebCodecsStateListener = webCodecsPlayer.onState(handleWebCodecsPlayerState);
  uninstallDiagnostics = installScreenShareDiagnostics(() => ({
    capturedAtUnixMs: Date.now(),
    transport: httpStatus.value.transport ?? null,
    server: {
      media_metrics: httpStatus.value.media_metrics ?? null,
      h264_media: httpStatus.value.h264_media ?? null,
      input_metrics: httpStatus.value.input_metrics ?? null,
      webrtc: httpStatus.value.webrtc ?? null,
      transport_degradation_reason: httpStatus.value.transport_degradation_reason ?? null,
    },
    client: {
      mse: {
        state: h264PlayerState.value,
        metrics: h264Player.getMetrics(),
      },
      web_codecs: {
        state: webCodecsPlayerState.value,
        metrics: webCodecsPlayer.getMetrics(),
      },
      web_rtc: {
        state: webRtcPlayerState.value,
        metrics: webRtcPlayer?.getMetrics() ?? null,
      },
      interaction: {
        state: sessionState.value,
        metrics: sessionClient.getMetrics(),
      },
    },
  }));
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
  webCodecsPlayer.stop();
  webRtcPlayer?.stop();
  webRtcPlayer = null;
  stopH264StateListener?.();
  stopH264StateListener = null;
  stopWebCodecsStateListener?.();
  stopWebCodecsStateListener = null;
  uninstallDiagnostics?.();
  uninstallDiagnostics = null;
  stopInputTraceListener?.();
  stopInputTraceListener = null;
  if (statusTimer !== null) window.clearInterval(statusTimer);
  if (laserTimer !== null) window.clearInterval(laserTimer);
  if (streamRetryTimer !== null) window.clearTimeout(streamRetryTimer);
  if (sessionNoticeTimer !== null) window.clearTimeout(sessionNoticeTimer);
  resizeObserver?.disconnect();
  document.removeEventListener('fullscreenchange', updateFullscreenState);
  document.removeEventListener('visibilitychange', onVisibilityChange);
  document.removeEventListener('keydown', onKeydown);
  document.removeEventListener('keyup', onKeyup);
  window.removeEventListener('blur', onWindowBlur);
  window.removeEventListener('pagehide', onWindowBlur);
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

    <section ref="stage" class="viewer-stage">
      <video
        ref="screenVideo"
        class="screen-image"
        :class="{ 'is-hidden-media': !showH264Video }"
        muted
        autoplay
        playsinline
        @loadedmetadata="handleVideoMetadata"
      />
      <canvas
        ref="screenCanvas"
        class="screen-image"
        :class="{ 'is-hidden-media': !showWebCodecsCanvas }"
        aria-hidden="true"
      />
      <img
        v-if="!showPrimaryMedia && statusActive"
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
      <div v-if="statusActive && !showPrimaryMedia && !imageReady" class="stream-empty-state" role="status" aria-live="polite">
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

      <div class="tool-group" role="toolbar" :aria-label="t('fullscreen')">
        <button type="button" class="icon-button" :title="isFullscreen ? t('exitFullscreen') : t('fullscreen')" :aria-label="isFullscreen ? t('exitFullscreen') : t('fullscreen')" :aria-pressed="isFullscreen" @click="toggleFullscreen">
          <Minimize2 v-if="isFullscreen" :size="19" />
          <Maximize2 v-else :size="19" />
        </button>
      </div>
    </footer>
  </main>
</template>
