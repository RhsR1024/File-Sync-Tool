<script setup lang="ts">
import { convertFileSrc } from '@tauri-apps/api/core';
import { Check, ClipboardCopy, Crop, ImagePlus, RotateCcw } from 'lucide-vue-next';
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import { clipboardApi, type ImageCopyCrop } from '@/lib/tauri';

withDefaults(defineProps<{ embedded?: boolean }>(), {
  embedded: false,
});

const { t } = useI18n();

const imagePath = ref('');
const imageElement = ref<HTMLImageElement | null>(null);
const naturalWidth = ref(0);
const naturalHeight = ref(0);
const cropMode = ref(false);
const crop = ref<ImageCopyCrop | null>(null);
const dragStart = ref<{ x: number; y: number } | null>(null);
const copying = ref(false);
const notice = ref<{ kind: 'success' | 'error'; text: string } | null>(null);

const imageUrl = computed(() => (imagePath.value ? convertFileSrc(imagePath.value) : ''));
const fileName = computed(() => imagePath.value.split(/[\\/]/).at(-1) ?? imagePath.value);
const selectionStyle = computed(() => {
  if (!crop.value || !naturalWidth.value || !naturalHeight.value) return {};
  return {
    left: `${(crop.value.x / naturalWidth.value) * 100}%`,
    top: `${(crop.value.y / naturalHeight.value) * 100}%`,
    width: `${(crop.value.width / naturalWidth.value) * 100}%`,
    height: `${(crop.value.height / naturalHeight.value) * 100}%`,
  };
});
const dimensionsLabel = computed(() => {
  if (!naturalWidth.value || !naturalHeight.value) return '';
  if (crop.value) return `${crop.value.width} × ${crop.value.height}`;
  return `${naturalWidth.value} × ${naturalHeight.value}`;
});

function friendlyError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function chooseImage() {
  try {
    const selected = await clipboardApi.pickImageFile();
    if (!selected) return;
    imagePath.value = selected;
    crop.value = null;
    cropMode.value = false;
    notice.value = null;
  } catch (error) {
    notice.value = { kind: 'error', text: friendlyError(error) };
  }
}

function onImageLoad() {
  naturalWidth.value = imageElement.value?.naturalWidth ?? 0;
  naturalHeight.value = imageElement.value?.naturalHeight ?? 0;
  crop.value = null;
}

function imagePoint(event: PointerEvent): { x: number; y: number } | null {
  const image = imageElement.value;
  if (!image || !naturalWidth.value || !naturalHeight.value) return null;
  const rect = image.getBoundingClientRect();
  if (!rect.width || !rect.height) return null;
  return {
    x: Math.round(Math.min(Math.max(event.clientX - rect.left, 0), rect.width) / rect.width * naturalWidth.value),
    y: Math.round(Math.min(Math.max(event.clientY - rect.top, 0), rect.height) / rect.height * naturalHeight.value),
  };
}

function updateSelection(current: { x: number; y: number }) {
  if (!dragStart.value) return;
  const left = Math.min(dragStart.value.x, current.x);
  const top = Math.min(dragStart.value.y, current.y);
  const right = Math.max(dragStart.value.x, current.x);
  const bottom = Math.max(dragStart.value.y, current.y);
  if (right === left || bottom === top) {
    crop.value = null;
    return;
  }
  crop.value = {
    x: left,
    y: top,
    width: right - left,
    height: bottom - top,
  };
}

function onPointerDown(event: PointerEvent) {
  if (!cropMode.value) return;
  const point = imagePoint(event);
  if (!point) return;
  event.preventDefault();
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  dragStart.value = point;
  crop.value = null;
}

function onPointerMove(event: PointerEvent) {
  if (!cropMode.value || !dragStart.value) return;
  const point = imagePoint(event);
  if (point) updateSelection(point);
}

function onPointerUp(event: PointerEvent) {
  if (!dragStart.value) return;
  const point = imagePoint(event);
  if (point) updateSelection(point);
  dragStart.value = null;
}

function toggleCropMode() {
  cropMode.value = !cropMode.value;
  if (!cropMode.value) crop.value = null;
  notice.value = null;
}

function resetCrop() {
  crop.value = null;
  notice.value = null;
}

async function copyImage() {
  if (!imagePath.value || copying.value) return;
  copying.value = true;
  notice.value = null;
  try {
    await clipboardApi.copyImageFile(imagePath.value, crop.value);
    notice.value = { kind: 'success', text: t('clipboard.imageCopy.copied') };
  } catch (error) {
    notice.value = { kind: 'error', text: friendlyError(error) };
  } finally {
    copying.value = false;
  }
}
</script>

<template>
  <section
    class="overflow-hidden"
    :class="embedded ? '' : 'rounded-2xl border border-slate-200 bg-white shadow-sm'"
  >
    <header
      class="flex flex-wrap items-start justify-between gap-3 border-b border-slate-100"
      :class="embedded ? 'px-1 pb-4' : 'px-5 py-4'"
    >
      <div class="flex items-start gap-3">
        <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-sky-50 text-sky-700">
          <ClipboardCopy class="h-5 w-5" aria-hidden="true" />
        </span>
        <div>
          <h2 class="text-base font-semibold text-slate-950">{{ t('clipboard.imageCopy.title') }}</h2>
          <p class="mt-1 text-sm leading-5 text-slate-500">{{ t('clipboard.imageCopy.description') }}</p>
        </div>
      </div>
      <span class="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-xs font-medium text-slate-600">
        PNG · JPEG
      </span>
    </header>

    <div
      class="grid gap-5 lg:grid-cols-[minmax(0,1.45fr)_minmax(280px,0.55fr)]"
      :class="embedded ? 'px-1 pt-5 pb-1' : 'p-5'"
    >
      <div
        class="flex min-h-[310px] items-center justify-center overflow-hidden rounded-2xl border border-dashed bg-slate-50 p-4"
        :class="cropMode ? 'border-sky-300' : 'border-slate-300'"
      >
        <div v-if="imagePath" class="max-h-[480px] max-w-full overflow-auto rounded-xl bg-[linear-gradient(45deg,#e2e8f0_25%,transparent_25%),linear-gradient(-45deg,#e2e8f0_25%,transparent_25%),linear-gradient(45deg,transparent_75%,#e2e8f0_75%),linear-gradient(-45deg,transparent_75%,#e2e8f0_75%)] bg-[length:20px_20px] bg-[position:0_0,0_10px,10px_-10px,-10px_0px] p-2 shadow-inner">
          <div
            class="relative inline-block select-none align-top"
            :class="cropMode ? 'cursor-crosshair touch-none' : 'cursor-context-menu'"
            @pointerdown="onPointerDown"
            @pointermove="onPointerMove"
            @pointerup="onPointerUp"
            @pointercancel="dragStart = null"
            @contextmenu.prevent="copyImage"
          >
            <img
              ref="imageElement"
              :src="imageUrl"
              :alt="fileName"
              class="block max-h-[440px] max-w-full rounded-lg object-contain"
              draggable="false"
              @load="onImageLoad"
            >
            <div
              v-if="crop"
              class="pointer-events-none absolute border-2 border-sky-400 bg-sky-400/15 shadow-[0_0_0_9999px_rgba(15,23,42,0.34)]"
              :style="selectionStyle"
              aria-hidden="true"
            ></div>
          </div>
        </div>

        <button
          v-else
          type="button"
          class="flex min-h-44 w-full cursor-pointer flex-col items-center justify-center rounded-xl text-center text-slate-500 transition-colors duration-200 hover:bg-white hover:text-sky-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50"
          @click="chooseImage"
        >
          <ImagePlus class="h-9 w-9" aria-hidden="true" />
          <span class="mt-3 text-sm font-semibold">{{ t('clipboard.imageCopy.choose') }}</span>
          <span class="mt-1 text-xs text-slate-400">{{ t('clipboard.imageCopy.chooseHint') }}</span>
        </button>
      </div>

      <div class="flex min-w-0 flex-col">
        <div class="rounded-xl border border-slate-200 bg-slate-50 px-4 py-3">
          <p class="text-xs font-semibold uppercase tracking-[0.14em] text-slate-400">{{ t('clipboard.imageCopy.current') }}</p>
          <p class="mt-2 truncate text-sm font-medium text-slate-800" :title="imagePath">
            {{ imagePath ? fileName : t('clipboard.imageCopy.none') }}
          </p>
          <p v-if="dimensionsLabel" class="mt-1 text-xs text-slate-500">{{ dimensionsLabel }} px</p>
        </div>

        <p class="mt-3 text-xs leading-5 text-slate-500">
          {{ cropMode ? t('clipboard.imageCopy.cropHint') : t('clipboard.imageCopy.rightClickHint') }}
        </p>

        <div class="mt-4 grid grid-cols-2 gap-2">
          <button
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-xl border border-slate-200 bg-white px-3 text-sm font-medium text-slate-700 transition-colors duration-200 hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50"
            @click="chooseImage"
          >
            <ImagePlus class="h-4 w-4" aria-hidden="true" />
            {{ t('clipboard.imageCopy.choose') }}
          </button>
          <button
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-xl border px-3 text-sm font-medium transition-colors duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50 disabled:cursor-not-allowed disabled:opacity-50"
            :class="cropMode ? 'border-sky-300 bg-sky-50 text-sky-700' : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50'"
            :disabled="!imagePath"
            :aria-pressed="cropMode"
            @click="toggleCropMode"
          >
            <Crop class="h-4 w-4" aria-hidden="true" />
            {{ t('clipboard.imageCopy.crop') }}
          </button>
          <button
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-xl border border-slate-200 bg-white px-3 text-sm font-medium text-slate-700 transition-colors duration-200 hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="!crop"
            @click="resetCrop"
          >
            <RotateCcw class="h-4 w-4" aria-hidden="true" />
            {{ t('clipboard.imageCopy.resetCrop') }}
          </button>
          <button
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-xl bg-sky-700 px-3 text-sm font-semibold text-white shadow-sm transition-colors duration-200 hover:bg-sky-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/60 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:bg-slate-300"
            :disabled="!imagePath || copying"
            @click="copyImage"
          >
            <ClipboardCopy class="h-4 w-4" aria-hidden="true" />
            {{ copying ? t('clipboard.imageCopy.copying') : t('clipboard.imageCopy.copy') }}
          </button>
        </div>

        <div
          v-if="notice"
          class="mt-4 flex items-start gap-2 rounded-xl border px-3 py-2.5 text-sm"
          :class="notice.kind === 'success'
            ? 'border-emerald-200 bg-emerald-50 text-emerald-700'
            : 'border-rose-200 bg-rose-50 text-rose-700'"
          role="status"
        >
          <Check v-if="notice.kind === 'success'" class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
          <span class="break-words">{{ notice.text }}</span>
        </div>
      </div>
    </div>
  </section>
</template>
