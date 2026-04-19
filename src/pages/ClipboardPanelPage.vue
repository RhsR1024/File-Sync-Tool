<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useI18n } from 'vue-i18n';

import { useClipboardStore } from '@/composables/useClipboardStore';
import { useClipboardHotkey } from '@/composables/useClipboardHotkey';
import ClipboardList from '@/components/clipboard/ClipboardList.vue';
import { clipboardApi } from '@/lib/tauri';
import type { ClipboardFilter } from '@/lib/clipboardTypes';

defineOptions({ name: 'ClipboardPanelPage' });

const { t } = useI18n();
const store = useClipboardStore();

const selectedIndex = ref(0);
const searchInput = ref<HTMLInputElement | null>(null);

const filters: ClipboardFilter[] = ['all', 'text', 'image', 'file', 'favorite'];

const selectedId = computed<number | null>(
  () => store.items.value[selectedIndex.value]?.id ?? null,
);

async function paste(id: number, plain: boolean) {
  try {
    if (plain) await clipboardApi.pastePlain(id);
    else await clipboardApi.paste(id);
  } catch (e) {
    store.error.value = String(e);
  }
}

function close() {
  void getCurrentWindow().hide();
}

function changeFilter(dir: 1 | -1) {
  const cur = store.filter.value;
  const curIdx = filters.indexOf(cur);
  const next = filters[(curIdx + dir + filters.length) % filters.length];
  store.filter.value = next;
  selectedIndex.value = 0;
  void store.reload();
}

function setFilter(f: ClipboardFilter) {
  store.filter.value = f;
  selectedIndex.value = 0;
  void store.reload();
}

function selectById(id: number) {
  const idx = store.items.value.findIndex((it) => it.id === id);
  if (idx >= 0) selectedIndex.value = idx;
}

useClipboardHotkey({
  items: store.items,
  selectedIndex,
  filter: store.filter,
  searchValue: store.search,
  onPaste: paste,
  onDelete: (id) => store.remove(id),
  onFavorite: (id) => store.toggleFavorite(id),
  onClose: close,
  onFocusSearch: () => searchInput.value?.focus(),
  onFilterChange: changeFilter,
});

// Keep selection in-bounds when the list shrinks.
watch(
  () => store.items.value.length,
  (len) => {
    if (selectedIndex.value >= len) {
      selectedIndex.value = Math.max(0, len - 1);
    }
  },
);

let unlistenShown: UnlistenFn | null = null;
let unlistenItemAdded: UnlistenFn | null = null;

onMounted(async () => {
  unlistenShown = await listen('clipboard-panel-shown', async () => {
    store.search.value = '';
    selectedIndex.value = 0;
    await store.reload();
    await nextTick();
    searchInput.value?.focus();
  });
  unlistenItemAdded = await store.startListening();
  await store.reload();
});

onBeforeUnmount(() => {
  unlistenShown?.();
  unlistenItemAdded?.();
});
</script>

<template>
  <div class="flex h-screen w-screen flex-col overflow-hidden rounded-2xl bg-white/85 shadow-2xl backdrop-blur-xl">
    <header class="flex items-center justify-between border-b border-slate-200/60 px-4 py-3">
      <span class="text-sm font-semibold text-slate-700">{{ t('clipboard.tool.title') }}</span>
      <button
        type="button"
        class="text-xs text-slate-400 transition-colors hover:text-slate-700"
        :title="t('clipboard.actions.close')"
        @click="close"
      >
        ✕
      </button>
    </header>

    <div class="px-3 pt-3 pb-2">
      <input
        ref="searchInput"
        v-model="store.search.value"
        type="search"
        :placeholder="t('clipboard.search.placeholder')"
        class="w-full rounded-lg border border-slate-200/70 bg-white/60 px-3 py-1.5 text-sm outline-none focus:border-slate-400"
        @input="store.reload()"
      />
    </div>

    <div class="flex flex-wrap gap-1 px-3 pb-2">
      <button
        v-for="f in filters"
        :key="f"
        type="button"
        class="rounded-full px-2.5 py-0.5 text-xs transition-colors"
        :class="store.filter.value === f
          ? 'bg-slate-900 text-white shadow-sm'
          : 'bg-slate-200/60 text-slate-600 hover:bg-slate-200'"
        @click="setFilter(f)"
      >
        {{ t(`clipboard.filter.${f}`) }}
      </button>
    </div>

    <div class="flex-1 overflow-hidden px-2 pb-2">
      <div v-if="store.items.value.length === 0" class="flex h-full items-center justify-center p-6 text-center text-sm text-slate-400">
        {{ t('clipboard.panel.empty') }}
      </div>
      <ClipboardList
        v-else
        :items="store.items.value"
        :selected-id="selectedId"
        :compact="true"
        @select="selectById"
        @activate="(id) => paste(id, false)"
      />
    </div>
  </div>
</template>
