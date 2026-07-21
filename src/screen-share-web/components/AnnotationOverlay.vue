<script setup lang="ts">
import { computed, ref } from 'vue';

import { clampNormalized, pointToNormalized, type ContainedRect } from '../lib/coordinates';
import type {
  AnnotationAddPayload,
  AnnotationKind,
  AnnotationShape,
  AnnotationUpdatePayload,
  NormalizedPoint,
} from '../types';

const props = defineProps<{
  shapes: AnnotationShape[];
  geometry: ContainedRect;
  tool: AnnotationKind | 'view' | 'control';
  color: string;
  width: number;
  enabled: boolean;
  editMode: boolean;
  selectedId: string | null;
  clientId: string;
}>();

const emit = defineEmits<{
  add: [payload: AnnotationAddPayload];
  select: [shapeId: string | null];
  update: [payload: AnnotationUpdatePayload];
}>();

const layer = ref<HTMLElement | null>(null);
const draft = ref<{ kind: 'arrow' | 'rect'; start: NormalizedPoint; end: NormalizedPoint } | null>(null);
const activePointerId = ref<number | null>(null);
const editDraft = ref<AnnotationShape | null>(null);
const editDrag = ref<{
  pointerId: number;
  shapeId: string;
  mode: 'move' | 'point';
  pointIndex: number;
  origin: NormalizedPoint;
  originalPoints: NormalizedPoint[];
} | null>(null);

const displayShapes = computed(() => props.shapes.map((shape) => (
  editDraft.value?.id === shape.id ? editDraft.value : shape
)));

const layerStyle = computed<Record<string, string>>(() => ({
  left: `${props.geometry.left}px`,
  top: `${props.geometry.top}px`,
  width: `${props.geometry.width}px`,
  height: `${props.geometry.height}px`,
  pointerEvents: props.enabled && (props.editMode || (props.tool !== 'view' && props.tool !== 'control')) ? 'auto' : 'none',
}));

const svgViewBox = computed(() => `0 0 ${Math.max(1, props.geometry.width)} ${Math.max(1, props.geometry.height)}`);

function safeColor(value: string): string {
  return /^#[0-9a-f]{3,8}$/i.test(value) ? value : '#f59e0b';
}

function cssPoint(point: NormalizedPoint): { x: number; y: number } {
  return {
    x: point.x * props.geometry.width,
    y: point.y * props.geometry.height,
  };
}

function rectFor(shape: AnnotationShape) {
  const first = shape.points[0] ?? { x: 0, y: 0 };
  const second = shape.points[1] ?? first;
  const a = cssPoint(first);
  const b = cssPoint(second);
  return {
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(a.x - b.x),
    height: Math.abs(a.y - b.y),
  };
}

function arrowHead(shape: { points: NormalizedPoint[]; width?: number }): string {
  const first = cssPoint(shape.points[0] ?? { x: 0, y: 0 });
  const last = cssPoint(shape.points[shape.points.length - 1] ?? { x: 0, y: 0 });
  const dx = last.x - first.x;
  const dy = last.y - first.y;
  const distance = Math.hypot(dx, dy) || 1;
  const ux = dx / distance;
  const uy = dy / distance;
  const size = Math.max(9, (shape.width ?? props.width) * 3.5);
  const baseX = last.x - ux * size;
  const baseY = last.y - uy * size;
  const sideX = -uy * size * 0.55;
  const sideY = ux * size * 0.55;
  return `${last.x},${last.y} ${baseX + sideX},${baseY + sideY} ${baseX - sideX},${baseY - sideY}`;
}

function mapEvent(event: PointerEvent): NormalizedPoint | null {
  const parent = layer.value?.parentElement;
  if (!parent) return null;
  const parentRect = parent.getBoundingClientRect();
  return pointToNormalized(event.clientX, event.clientY, parentRect, props.geometry);
}

function canEditShape(shape: AnnotationShape): boolean {
  return props.editMode && shape.kind !== 'laser' && shape.ownerClientId === props.clientId;
}

function cloneShape(shape: AnnotationShape): AnnotationShape {
  return {
    ...shape,
    points: shape.points.map((point) => ({ ...point })),
  };
}

function capturePointer(event: PointerEvent) {
  try { layer.value?.setPointerCapture(event.pointerId); } catch { /* pointer already released */ }
}

function releasePointer(event: PointerEvent) {
  try { layer.value?.releasePointerCapture(event.pointerId); } catch { /* pointer already released */ }
}

function begin(event: PointerEvent) {
  if (props.editMode) {
    emit('select', null);
    return;
  }
  if (!props.enabled || props.tool === 'view' || props.tool === 'control') return;
  const point = mapEvent(event);
  if (!point) return;
  event.preventDefault();
  if (props.tool === 'laser') {
    emit('add', {
      kind: 'laser',
      points: [point],
      color: safeColor(props.color),
      width: props.width,
      expires_at_ms: Date.now() + 2000,
    });
    return;
  }
  draft.value = { kind: props.tool, start: point, end: point };
  activePointerId.value = event.pointerId;
  capturePointer(event);
}

function move(event: PointerEvent) {
  if (editDrag.value && editDraft.value && editDrag.value.pointerId === event.pointerId) {
    const point = mapEvent(event);
    if (!point) return;
    const drag = editDrag.value;
    const next = cloneShape(editDraft.value);
    if (drag.mode === 'point') {
      next.points[drag.pointIndex] = point;
    } else {
      const deltaX = point.x - drag.origin.x;
      const deltaY = point.y - drag.origin.y;
      const minX = Math.min(...drag.originalPoints.map((item) => item.x));
      const maxX = Math.max(...drag.originalPoints.map((item) => item.x));
      const minY = Math.min(...drag.originalPoints.map((item) => item.y));
      const maxY = Math.max(...drag.originalPoints.map((item) => item.y));
      const boundedX = Math.min(1 - maxX, Math.max(-minX, deltaX));
      const boundedY = Math.min(1 - maxY, Math.max(-minY, deltaY));
      next.points = drag.originalPoints.map((item) => clampNormalized({
        x: item.x + boundedX,
        y: item.y + boundedY,
      }));
    }
    editDraft.value = next;
    return;
  }
  if (!draft.value || activePointerId.value !== event.pointerId) return;
  const point = mapEvent(event);
  if (point) draft.value.end = point;
}

function finish(event: PointerEvent) {
  if (editDrag.value && editDraft.value && editDrag.value.pointerId === event.pointerId) {
    const updated = editDraft.value;
    const original = props.shapes.find((shape) => shape.id === updated.id);
    editDrag.value = null;
    editDraft.value = null;
    releasePointer(event);
    if (original && JSON.stringify(original.points) !== JSON.stringify(updated.points)) {
      emit('update', {
        shape_id: updated.id,
        points: updated.points,
        color: updated.color,
        width: updated.width,
      });
    }
    return;
  }
  if (!draft.value || activePointerId.value !== event.pointerId) return;
  const current = draft.value;
  const dx = current.end.x - current.start.x;
  const dy = current.end.y - current.start.y;
  draft.value = null;
  activePointerId.value = null;
  releasePointer(event);
  if (Math.hypot(dx, dy) < 0.005) return;
  emit('add', {
    kind: current.kind,
    points: [current.start, current.end],
    color: safeColor(props.color),
    width: props.width,
    expires_at_ms: null,
  });
}

function cancel(event: PointerEvent) {
  if (editDrag.value?.pointerId === event.pointerId) {
    editDrag.value = null;
    editDraft.value = null;
    releasePointer(event);
    return;
  }
  if (activePointerId.value !== event.pointerId) return;
  draft.value = null;
  activePointerId.value = null;
  releasePointer(event);
}

function beginShapeDrag(shape: AnnotationShape, event: PointerEvent) {
  if (!canEditShape(shape)) return;
  event.preventDefault();
  event.stopPropagation();
  emit('select', shape.id);
  const origin = mapEvent(event);
  if (!origin) return;
  editDraft.value = cloneShape(shape);
  editDrag.value = {
    pointerId: event.pointerId,
    shapeId: shape.id,
    mode: 'move',
    pointIndex: -1,
    origin,
    originalPoints: shape.points.map((point) => ({ ...point })),
  };
  capturePointer(event);
}

function beginHandleDrag(shape: AnnotationShape, pointIndex: number, event: PointerEvent) {
  if (!canEditShape(shape)) return;
  event.preventDefault();
  event.stopPropagation();
  emit('select', shape.id);
  const origin = mapEvent(event);
  if (!origin) return;
  editDraft.value = cloneShape(shape);
  editDrag.value = {
    pointerId: event.pointerId,
    shapeId: shape.id,
    mode: 'point',
    pointIndex,
    origin,
    originalPoints: shape.points.map((point) => ({ ...point })),
  };
  capturePointer(event);
}
</script>

<template>
  <div
    ref="layer"
    class="annotation-layer"
    :style="layerStyle"
    :aria-hidden="!enabled || (!editMode && (tool === 'view' || tool === 'control'))"
    @pointerdown="begin"
    @pointermove="move"
    @pointerup="finish"
    @pointercancel="cancel"
  >
    <svg
      class="annotation-svg"
      :viewBox="svgViewBox"
      preserveAspectRatio="none"
      focusable="false"
      aria-hidden="true"
    >
      <g
        v-for="shape in displayShapes"
        :key="shape.id"
        class="annotation-shape"
        :class="{ 'is-selected': selectedId === shape.id, 'is-editable': canEditShape(shape) }"
        :style="{ color: safeColor(shape.color) }"
        @pointerdown="beginShapeDrag(shape, $event)"
      >
        <line
          v-if="shape.kind === 'arrow' && canEditShape(shape)"
          :x1="cssPoint(shape.points[0] ?? { x: 0, y: 0 }).x"
          :y1="cssPoint(shape.points[0] ?? { x: 0, y: 0 }).y"
          :x2="cssPoint(shape.points[shape.points.length - 1] ?? { x: 0, y: 0 }).x"
          :y2="cssPoint(shape.points[shape.points.length - 1] ?? { x: 0, y: 0 }).y"
          :stroke-width="Math.max(18, shape.width + 12)"
          class="annotation-hit-target"
          vector-effect="non-scaling-stroke"
        />
        <rect
          v-if="shape.kind === 'rect' && canEditShape(shape)"
          v-bind="rectFor(shape)"
          :stroke-width="Math.max(18, shape.width + 12)"
          class="annotation-hit-target"
          vector-effect="non-scaling-stroke"
        />
        <line
          v-if="shape.kind === 'arrow'"
          :x1="cssPoint(shape.points[0] ?? { x: 0, y: 0 }).x"
          :y1="cssPoint(shape.points[0] ?? { x: 0, y: 0 }).y"
          :x2="cssPoint(shape.points[shape.points.length - 1] ?? { x: 0, y: 0 }).x"
          :y2="cssPoint(shape.points[shape.points.length - 1] ?? { x: 0, y: 0 }).y"
          :stroke="safeColor(shape.color)"
          :stroke-width="shape.width"
          stroke-linecap="round"
        />
        <polygon
          v-if="shape.kind === 'arrow'"
          :points="arrowHead(shape)"
          :fill="safeColor(shape.color)"
        />
        <rect
          v-else-if="shape.kind === 'rect'"
          v-bind="rectFor(shape)"
          :stroke="safeColor(shape.color)"
          :stroke-width="shape.width"
          fill="none"
          vector-effect="non-scaling-stroke"
        />
        <circle
          v-else
          :cx="cssPoint(shape.points[0] ?? { x: 0, y: 0 }).x"
          :cy="cssPoint(shape.points[0] ?? { x: 0, y: 0 }).y"
          :r="Math.max(7, shape.width * 2.2)"
          :fill="safeColor(shape.color)"
          class="laser-dot"
        />
        <g v-if="selectedId === shape.id && canEditShape(shape)" class="annotation-handles">
          <circle
            v-for="(point, pointIndex) in shape.points"
            :key="`${shape.id}-${pointIndex}`"
            :cx="cssPoint(point).x"
            :cy="cssPoint(point).y"
            r="8"
            class="annotation-handle"
            @pointerdown="beginHandleDrag(shape, pointIndex, $event)"
          />
        </g>
      </g>
      <g v-if="draft" class="annotation-draft" :style="{ color: safeColor(color) }">
        <line
          v-if="draft.kind === 'arrow'"
          :x1="cssPoint(draft.start).x"
          :y1="cssPoint(draft.start).y"
          :x2="cssPoint(draft.end).x"
          :y2="cssPoint(draft.end).y"
          :stroke="safeColor(color)"
          :stroke-width="width"
          stroke-linecap="round"
          stroke-dasharray="8 5"
        />
        <polygon
          v-if="draft.kind === 'arrow'"
          :points="arrowHead({ points: [draft.start, draft.end] })"
          :fill="safeColor(color)"
          opacity=".85"
        />
        <rect
          v-else
          v-bind="rectFor({ points: [draft.start, draft.end] } as AnnotationShape)"
          :stroke="safeColor(color)"
          :stroke-width="width"
          fill="none"
          stroke-dasharray="8 5"
        />
      </g>
    </svg>
  </div>
</template>
