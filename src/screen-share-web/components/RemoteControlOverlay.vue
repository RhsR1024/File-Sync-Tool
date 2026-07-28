<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';

import { pointToNormalized, type ContainedRect } from '../lib/coordinates';
import { normalizeRemoteWheelDelta, remoteMouseButton } from '../lib/remote-control';
import type { NormalizedPoint } from '../types';

const props = defineProps<{
  geometry: ContainedRect;
  enabled: boolean;
}>();

const emit = defineEmits<{
  move: [point: NormalizedPoint, eventOccurredAtMs: number];
  button: [payload: { button: 'left' | 'right'; pressed: boolean }, eventOccurredAtMs?: number];
  wheel: [deltaY: number];
}>();

const layer = ref<HTMLElement | null>(null);
const activePointerId = ref<number | null>(null);
const heldButtons = new Set<'left' | 'right'>();
let moveFrame: number | null = null;
let queuedMove: { point: NormalizedPoint; eventOccurredAtMs: number } | null = null;

const layerStyle = computed<Record<string, string>>(() => ({
  left: `${props.geometry.left}px`,
  top: `${props.geometry.top}px`,
  width: `${props.geometry.width}px`,
  height: `${props.geometry.height}px`,
  pointerEvents: props.enabled ? 'auto' : 'none',
}));

function mapEvent(event: Pick<MouseEvent, 'clientX' | 'clientY'>): NormalizedPoint | null {
  const parent = layer.value?.parentElement;
  if (!parent) return null;
  return pointToNormalized(
    event.clientX,
    event.clientY,
    parent.getBoundingClientRect(),
    props.geometry,
  );
}

function flushMove() {
  moveFrame = null;
  if (queuedMove) {
    emit('move', queuedMove.point, queuedMove.eventOccurredAtMs);
    queuedMove = null;
  }
}

function sendMove(point: NormalizedPoint, eventOccurredAtMs: number) {
  queuedMove = { point, eventOccurredAtMs };
  if (moveFrame === null) moveFrame = window.requestAnimationFrame(flushMove);
}

function cancelQueuedMove() {
  if (moveFrame !== null) {
    window.cancelAnimationFrame(moveFrame);
    moveFrame = null;
  }
  queuedMove = null;
}

function sendMoveNow(point: NormalizedPoint, eventOccurredAtMs: number) {
  cancelQueuedMove();
  emit('move', point, eventOccurredAtMs);
}

function begin(event: PointerEvent) {
  if (!props.enabled) return;
  const point = mapEvent(event);
  if (!point) return;
  const button = remoteMouseButton(event.button);
  if (!button) return;
  event.preventDefault();
  event.stopPropagation();
  sendMoveNow(point, event.timeStamp);
  heldButtons.add(button);
  activePointerId.value = event.pointerId;
  layer.value?.setPointerCapture(event.pointerId);
  emit('button', { button, pressed: true }, event.timeStamp);
}

function move(event: PointerEvent) {
  if (!props.enabled) return;
  const point = mapEvent(event);
  if (!point) return;
  event.preventDefault();
  sendMove(point, event.timeStamp);
}

function finish(event: PointerEvent) {
  const button = remoteMouseButton(event.button);
  if (!button || !heldButtons.has(button)) return;
  event.preventDefault();
  event.stopPropagation();
  const point = mapEvent(event);
  if (point) sendMoveNow(point, event.timeStamp);
  heldButtons.delete(button);
  emit('button', { button, pressed: false }, event.timeStamp);
  if (activePointerId.value === event.pointerId) {
    activePointerId.value = null;
    try { layer.value?.releasePointerCapture(event.pointerId); } catch { /* already released */ }
  }
}

function cancel(event: PointerEvent) {
  finish(event);
  releaseAll();
}

function onWheel(event: WheelEvent) {
  if (!props.enabled) return;
  const rect = layer.value?.getBoundingClientRect();
  if (!rect || event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) return;
  const point = mapEvent(event);
  const delta = normalizeRemoteWheelDelta(event.deltaY);
  if (!point || delta === null) return;
  event.preventDefault();
  event.stopPropagation();
  sendMoveNow(point, event.timeStamp);
  emit('wheel', delta);
}

function releaseAll() {
  cancelQueuedMove();
  for (const button of heldButtons) emit('button', { button, pressed: false });
  heldButtons.clear();
  activePointerId.value = null;
}

function onWindowBlur() {
  releaseAll();
}

onMounted(() => {
  window.addEventListener('blur', onWindowBlur);
  document.addEventListener('visibilitychange', onWindowBlur);
});

watch(() => props.enabled, (enabled) => {
  if (!enabled) releaseAll();
});

onUnmounted(() => {
  releaseAll();
  window.removeEventListener('blur', onWindowBlur);
  document.removeEventListener('visibilitychange', onWindowBlur);
});
</script>

<template>
  <div
    ref="layer"
    class="remote-control-layer"
    :style="layerStyle"
    :aria-hidden="!enabled"
    @pointerdown="begin"
    @pointermove="move"
    @pointerup="finish"
    @pointercancel="cancel"
    @wheel="onWheel"
    @contextmenu.prevent
  />
</template>
