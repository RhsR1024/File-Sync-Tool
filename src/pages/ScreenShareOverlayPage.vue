<script setup lang="ts">
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { computed, onMounted, onUnmounted, ref } from 'vue';

import {
  screenShareDesktopOverlayReady,
  screenShareGetAnnotationState,
} from '@/lib/tauri';
import {
  emptyDocument,
  normalizeDocument,
  visibleShapes,
} from '@/screen-share-web/lib/annotation-state';
import type {
  AnnotationDocument,
  AnnotationShape,
  NormalizedPoint,
} from '@/screen-share-web/types';

defineOptions({ name: 'ScreenShareOverlayPage' });

const documentState = ref<AnnotationDocument>(emptyDocument());
const viewport = ref({ width: 1, height: 1 });
const now = ref(Date.now());

let unlistenAnnotationState: UnlistenFn | null = null;
let laserTimer: number | null = null;

const shapes = computed(() => visibleShapes(documentState.value, now.value));
const viewBox = computed(() => `0 0 ${viewport.value.width} ${viewport.value.height}`);

function applyDocument(value: unknown) {
  const next = normalizeDocument(value, documentState.value);
  const current = documentState.value;
  if (next.sessionId === current.sessionId) {
    if (next.sourceEpoch < current.sourceEpoch) return;
    if (next.sourceEpoch === current.sourceEpoch && next.revision < current.revision) return;
  }
  documentState.value = next;
}

function syncViewport() {
  viewport.value = {
    width: Math.max(1, window.innerWidth),
    height: Math.max(1, window.innerHeight),
  };
}

function safeColor(value: string): string {
  return /^#[0-9a-f]{3,8}$/i.test(value) ? value : '#f59e0b';
}

function cssPoint(point: NormalizedPoint): { x: number; y: number } {
  return {
    x: point.x * viewport.value.width,
    y: point.y * viewport.value.height,
  };
}

function rectFor(shape: AnnotationShape) {
  const first = cssPoint(shape.points[0] ?? { x: 0, y: 0 });
  const second = cssPoint(shape.points[1] ?? shape.points[0] ?? { x: 0, y: 0 });
  return {
    x: Math.min(first.x, second.x),
    y: Math.min(first.y, second.y),
    width: Math.abs(first.x - second.x),
    height: Math.abs(first.y - second.y),
  };
}

function arrowHead(shape: AnnotationShape): string {
  const first = cssPoint(shape.points[0] ?? { x: 0, y: 0 });
  const last = cssPoint(shape.points[shape.points.length - 1] ?? { x: 0, y: 0 });
  const dx = last.x - first.x;
  const dy = last.y - first.y;
  const distance = Math.hypot(dx, dy) || 1;
  const ux = dx / distance;
  const uy = dy / distance;
  const size = Math.max(10, shape.width * 3.5);
  const baseX = last.x - ux * size;
  const baseY = last.y - uy * size;
  const sideX = -uy * size * 0.55;
  const sideY = ux * size * 0.55;
  return `${last.x},${last.y} ${baseX + sideX},${baseY + sideY} ${baseX - sideX},${baseY - sideY}`;
}

onMounted(async () => {
  document.documentElement.classList.add('screen-share-overlay-window');
  document.body.classList.add('screen-share-overlay-window');
  syncViewport();
  window.addEventListener('resize', syncViewport);

  try {
    unlistenAnnotationState = await listen<unknown>('screen-share-annotation-state', (event) => {
      applyDocument(event.payload);
    });
    applyDocument(await screenShareGetAnnotationState());
    laserTimer = window.setInterval(() => {
      now.value = Date.now();
    }, 100);
    await screenShareDesktopOverlayReady();
  } catch {
    await getCurrentWindow().close();
  }
});

onUnmounted(() => {
  unlistenAnnotationState?.();
  if (laserTimer !== null) window.clearInterval(laserTimer);
  window.removeEventListener('resize', syncViewport);
  document.documentElement.classList.remove('screen-share-overlay-window');
  document.body.classList.remove('screen-share-overlay-window');
});
</script>

<template>
  <main class="overlay-root" aria-hidden="true">
    <svg
      class="overlay-svg"
      :viewBox="viewBox"
      preserveAspectRatio="none"
      focusable="false"
    >
      <g
        v-for="shape in shapes"
        :key="shape.id"
        class="overlay-shape"
        :style="{ color: safeColor(shape.color) }"
      >
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
          :r="Math.max(8, shape.width * 2.2)"
          :fill="safeColor(shape.color)"
          class="overlay-laser"
        />
      </g>
    </svg>
  </main>
</template>

<style scoped>
:global(html.screen-share-overlay-window),
:global(body.screen-share-overlay-window),
:global(body.screen-share-overlay-window #app) {
  background: transparent !important;
}

.overlay-root,
.overlay-svg {
  position: fixed;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: hidden;
  pointer-events: none;
  background: transparent;
}

.overlay-shape {
  filter: drop-shadow(0 1px 2px rgb(15 23 42 / 0.68));
}

.overlay-laser {
  filter: drop-shadow(0 0 5px currentColor) drop-shadow(0 1px 2px rgb(15 23 42 / 0.7));
}

@media (prefers-reduced-motion: reduce) {
  .overlay-shape,
  .overlay-laser {
    animation: none;
  }
}
</style>
