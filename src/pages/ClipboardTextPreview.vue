<script setup lang="ts">
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { X } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';

import {
  TEXT_PREVIEW_UPDATE_EVENT,
  type ClipboardTextPreviewPayload,
} from '@/lib/clipboardPreviewHelpers';
import { clipboardApi } from '@/lib/tauri';

defineOptions({ name: 'ClipboardTextPreview' });

const { t } = useI18n();
const payload = ref<ClipboardTextPreviewPayload | null>(null);

let unlisten: UnlistenFn | null = null;

const kindLabel = computed(() => payload.value?.kind.toUpperCase() ?? 'TEXT');

async function refreshPayload() {
  const cachedPayload = await clipboardApi.getTextPreviewPayload();
  payload.value = cachedPayload;
}

function onWindowFocus() {
  void refreshPayload();
}

function onVisibilityChange() {
  if (document.visibilityState === 'visible') {
    void refreshPayload();
  }
}

function closePreview() {
  void getCurrentWindow().hide();
}

function onWindowKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    closePreview();
  }
}

onMounted(async () => {
  unlisten = await listen<ClipboardTextPreviewPayload>(
    TEXT_PREVIEW_UPDATE_EVENT,
    (event) => {
      payload.value = event.payload;
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
  <div class="flex h-screen flex-col overflow-hidden bg-[linear-gradient(180deg,_rgba(255,255,255,0.98),_rgba(241,245,249,0.96))] text-slate-900">
    <header class="flex items-center justify-between gap-3 border-b border-slate-200/80 px-4 py-3">
      <div class="min-w-0">
        <p class="text-[11px] font-semibold uppercase tracking-[0.24em] text-slate-500">
          {{ t('clipboard.preview.titleText') }}
        </p>
        <p
          v-if="payload?.source_app"
          class="truncate text-sm text-slate-600"
        >
          {{ payload.source_app }}
        </p>
      </div>

      <div class="flex items-center gap-2">
        <span class="rounded-full bg-slate-900 px-2.5 py-1 text-[11px] font-semibold tracking-[0.18em] text-white">
          {{ kindLabel }}
        </span>
        <button
          type="button"
          class="inline-flex h-8 w-8 items-center justify-center rounded-full border border-slate-200 bg-white text-slate-500 transition-colors hover:bg-slate-50 hover:text-slate-800"
          :aria-label="t('clipboard.preview.close')"
          :title="t('clipboard.preview.close')"
          @click="closePreview"
        >
          <X class="h-4 w-4" />
        </button>
      </div>
    </header>

    <main class="flex-1 overflow-auto p-4">
      <pre
        v-if="payload"
        class="min-h-full whitespace-pre-wrap break-words rounded-2xl border border-slate-200 bg-white p-4 font-mono text-sm leading-6 text-slate-700 shadow-[0_24px_60px_-48px_rgba(15,23,42,0.85)]"
      >{{ payload.content }}</pre>
      <div
        v-else
        class="flex h-full items-center justify-center text-sm text-slate-400"
      >
        {{ t('clipboard.preview.emptyText') }}
      </div>
    </main>
  </div>
</template>
