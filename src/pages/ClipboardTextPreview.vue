<script setup lang="ts">
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';

import {
  TEXT_PREVIEW_UPDATE_EVENT,
  type ClipboardTextPreviewPayload,
} from '@/lib/clipboardPreviewHelpers';
import { clipboardApi } from '@/lib/tauri';

defineOptions({ name: 'ClipboardTextPreview' });

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

onMounted(async () => {
  unlisten = await listen<ClipboardTextPreviewPayload>(
    TEXT_PREVIEW_UPDATE_EVENT,
    (event) => {
      payload.value = event.payload;
    },
  );

  window.addEventListener('focus', onWindowFocus);
  document.addEventListener('visibilitychange', onVisibilityChange);
  await refreshPayload();
});

onBeforeUnmount(() => {
  unlisten?.();
  window.removeEventListener('focus', onWindowFocus);
  document.removeEventListener('visibilitychange', onVisibilityChange);
});
</script>

<template>
  <div class="flex h-screen flex-col overflow-hidden bg-[linear-gradient(180deg,_rgba(255,255,255,0.98),_rgba(241,245,249,0.96))] text-slate-900">
    <header class="flex items-center justify-between gap-3 border-b border-slate-200/80 px-4 py-3">
      <div class="min-w-0">
        <p class="text-[11px] font-semibold uppercase tracking-[0.24em] text-slate-500">
          Text Preview
        </p>
        <p
          v-if="payload?.source_app"
          class="truncate text-sm text-slate-600"
        >
          {{ payload.source_app }}
        </p>
      </div>

      <span class="rounded-full bg-slate-900 px-2.5 py-1 text-[11px] font-semibold tracking-[0.18em] text-white">
        {{ kindLabel }}
      </span>
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
        Hover a text item to preview it.
      </div>
    </main>
  </div>
</template>
