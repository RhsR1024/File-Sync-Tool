<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import type { UnlistenFn } from '@tauri-apps/api/event';

import { useClipboardStore } from '@/composables/useClipboardStore';
import type { ClipboardFilter } from '@/lib/clipboardTypes';

defineOptions({ name: 'ClipboardManagerPage' });

const { t } = useI18n();
const store = useClipboardStore();

const filters: ClipboardFilter[] = ['all', 'text', 'image', 'file', 'favorite'];

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  unlisten = await store.startListening();
  await store.reload();
});

onBeforeUnmount(() => {
  unlisten?.();
});

function setFilter(f: ClipboardFilter) {
  store.filter.value = f;
  void store.reload();
}

function onSearchInput(e: Event) {
  store.search.value = (e.target as HTMLInputElement).value;
  void store.reload();
}
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-gradient-to-b from-slate-50 to-white">
    <div class="mx-auto flex w-full max-w-6xl flex-col gap-6 px-6 py-6 pb-10">
      <header class="space-y-2">
        <h1 class="text-2xl font-bold tracking-tight text-slate-950">
          {{ t('clipboard.tool.title') }}
        </h1>
        <p class="text-sm text-slate-500">{{ t('clipboard.tool.description') }}</p>
      </header>

      <div class="flex items-center gap-3">
        <input
          type="search"
          :placeholder="t('clipboard.search.placeholder')"
          class="flex-1 rounded-xl border border-slate-200 bg-white px-3 py-2 text-sm shadow-sm outline-none focus:border-slate-400"
          @input="onSearchInput"
        />
        <span class="text-xs text-slate-400">{{ store.total.value }} {{ t('clipboard.totalSuffix') }}</span>
      </div>

      <div class="flex flex-wrap gap-2">
        <button
          v-for="f in filters"
          :key="f"
          type="button"
          class="rounded-full px-3 py-1 text-xs font-medium transition-colors"
          :class="store.filter.value === f
            ? 'bg-slate-900 text-white shadow-sm'
            : 'bg-slate-100 text-slate-600 hover:bg-slate-200'"
          @click="setFilter(f)"
        >
          {{ t(`clipboard.filter.${f}`) }}
        </button>
      </div>

      <section class="rounded-2xl border border-slate-200 bg-white shadow-sm">
        <div v-if="store.error.value" class="p-5 text-sm text-rose-500">
          {{ store.error.value }}
        </div>
        <div v-else-if="store.loading.value" class="p-6 text-sm text-slate-400">
          {{ t('clipboard.loading') }}
        </div>
        <div v-else-if="store.items.value.length === 0" class="p-8 text-center text-sm text-slate-400">
          {{ t('clipboard.panel.empty') }}
        </div>
        <ul v-else class="divide-y divide-slate-100">
          <li
            v-for="it in store.items.value"
            :key="it.id"
            class="flex items-center gap-3 px-4 py-3"
          >
            <span class="inline-flex shrink-0 rounded bg-slate-100 px-2 py-0.5 text-[11px] uppercase tracking-[0.12em] text-slate-500">
              {{ it.kind }}
            </span>
            <span class="flex-1 truncate text-sm text-slate-800">
              {{ it.content_preview }}
            </span>
            <button
              type="button"
              class="text-xs transition-colors"
              :class="it.is_favorite ? 'text-amber-500' : 'text-slate-400 hover:text-amber-500'"
              :title="t(it.is_favorite ? 'clipboard.actions.unfavorite' : 'clipboard.actions.favorite')"
              @click="store.toggleFavorite(it.id)"
            >
              {{ it.is_favorite ? '★' : '☆' }}
            </button>
            <button
              type="button"
              class="text-xs text-slate-400 transition-colors hover:text-rose-500"
              :title="t('clipboard.actions.delete')"
              @click="store.remove(it.id)"
            >
              ×
            </button>
          </li>
        </ul>
      </section>
    </div>
  </div>
</template>
