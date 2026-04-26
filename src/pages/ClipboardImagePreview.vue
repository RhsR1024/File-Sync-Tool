<script setup lang="ts">
import { convertFileSrc } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Plus, RotateCcw } from 'lucide-vue-next';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import {
  DEFAULT_IMAGE_PREVIEW_SCALE,
  IMAGE_PREVIEW_UPDATE_EVENT,
  stepImagePreviewScale,
  type ClipboardImagePreviewPayload,
} from '@/lib/clipboardPreviewHelpers';
import { clipboardApi } from '@/lib/tauri';

defineOptions({ name: 'ClipboardImagePreview' });

const { t } = useI18n();
const payload = ref<ClipboardImagePreviewPayload | null>(null);
const scale = ref(DEFAULT_IMAGE_PREVIEW_SCALE);

let unlisten: UnlistenFn | null = null;

const imageSrc = computed(() =>
  payload.value ? convertFileSrc(payload.value.image_path) : '',
);
const zoomLabel = computed(() => `${Math.round(scale.value * 100)}%`);

function applyPayload(next: ClipboardImagePreviewPayload) {
  payload.value = next;
  scale.value = DEFAULT_IMAGE_PREVIEW_SCALE;
}

function changeZoom(direction: 1 | -1) {
  if (!payload.value) return;
  scale.value = stepImagePreviewScale(
    scale.value,
    direction,
    payload.value.zoom_step,
  );
}

function resetZoom() {
  scale.value = DEFAULT_IMAGE_PREVIEW_SCALE;
}

function onWheel(event: WheelEvent) {
  if (!payload.value) return;
  event.preventDefault();
  changeZoom(event.deltaY < 0 ? 1 : -1);
}

function closePreview() {
  void getCurrentWindow().hide();
}

async function refreshPayload() {
  const cachedPayload = await clipboardApi.getImagePreviewPayload();
  if (cachedPayload) {
    applyPayload(cachedPayload);
    return;
  }
  payload.value = null;
}

function onWindowFocus() {
  void refreshPayload();
}

function onVisibilityChange() {
  if (document.visibilityState === 'visible') {
    void refreshPayload();
  }
}

function onWindowKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    closePreview();
  } else if (event.key === '+' || event.key === '=') {
    event.preventDefault();
    changeZoom(1);
  } else if (event.key === '-' || event.key === '_') {
    event.preventDefault();
    changeZoom(-1);
  }
}

onMounted(async () => {
  unlisten = await listen<ClipboardImagePreviewPayload>(
    IMAGE_PREVIEW_UPDATE_EVENT,
    (event) => {
      applyPayload(event.payload);
    },
  );

  window.addEventListener('focus', onWindowFocus);
  window.addEventListener('keydown', onWindowKeydown);
  document.addEventListener('visibilitychange', onVisibilityChange);
  await refreshPayload();
});

onBeforeUnmount(() => {
  unlisten?.();
  window.removeEventListener('focus', onWindowFocus);
  window.removeEventListener('keydown', onWindowKeydown);
  document.removeEventListener('visibilitychange', onVisibilityChange);
});
</script>

<template>
  <div class="flex h-screen flex-col overflow-hidden bg-[radial-gradient(circle_at_top,_rgba(148,163,184,0.18),_transparent_55%),linear-gradient(180deg,_rgba(248,250,252,0.98),_rgba(226,232,240,0.92))] text-slate-900">
    <header class="flex items-center justify-between gap-3 border-b border-white/60 px-4 py-3 backdrop-blur-sm">
      <div class="min-w-0">
        <p class="text-[11px] font-semibold uppercase tracking-[0.24em] text-slate-500">
          {{ t('clipboard.preview.titleImage') }}
        </p>
        <p
          v-if="payload?.source_app"
          class="truncate text-sm text-slate-600"
        >
          {{ payload.source_app }}
        </p>
      </div>

      <div class="flex items-center gap-2">
        <span class="rounded-full bg-white/80 px-2.5 py-1 text-xs font-semibold text-slate-600 shadow-sm">
          {{ zoomLabel }}
        </span>
        <button
          type="button"
          class="inline-flex h-8 w-8 items-center justify-center rounded-full border border-white/70 bg-white/70 text-slate-600 transition hover:bg-white hover:text-slate-900"
          :title="t('clipboard.preview.zoomOut')"
          :aria-label="t('clipboard.preview.zoomOut')"
          @click="changeZoom(-1)"
        >
          <Minus class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="inline-flex h-8 w-8 items-center justify-center rounded-full border border-white/70 bg-white/70 text-slate-600 transition hover:bg-white hover:text-slate-900"
          :title="t('clipboard.preview.resetZoom')"
          :aria-label="t('clipboard.preview.resetZoom')"
          @click="resetZoom"
        >
          <RotateCcw class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="inline-flex h-8 w-8 items-center justify-center rounded-full border border-white/70 bg-white/70 text-slate-600 transition hover:bg-white hover:text-slate-900"
          :title="t('clipboard.preview.zoomIn')"
          :aria-label="t('clipboard.preview.zoomIn')"
          @click="changeZoom(1)"
        >
          <Plus class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="inline-flex h-8 w-8 items-center justify-center rounded-full border border-white/70 bg-white/70 text-slate-600 transition hover:bg-white hover:text-slate-900"
          :title="t('clipboard.preview.close')"
          :aria-label="t('clipboard.preview.close')"
          @click="closePreview"
        >
          <span class="text-xs font-semibold">Esc</span>
        </button>
      </div>
    </header>

    <main class="flex-1 overflow-auto p-4" @wheel="onWheel">
      <div
        v-if="payload"
        class="flex min-h-full items-center justify-center"
      >
        <img
          :src="imageSrc"
          :style="{
            transform: `scale(${scale})`,
            transformOrigin: 'center center',
          }"
          class="max-w-none rounded-2xl border border-white/70 bg-white/65 shadow-[0_28px_80px_-36px_rgba(15,23,42,0.7)]"
          alt=""
        >
      </div>
      <div
        v-else
        class="flex h-full items-center justify-center text-sm text-slate-400"
      >
        {{ t('clipboard.preview.emptyImage') }}
      </div>
    </main>
  </div>
</template>
