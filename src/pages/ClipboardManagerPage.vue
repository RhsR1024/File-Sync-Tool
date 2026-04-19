<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import type { UnlistenFn } from '@tauri-apps/api/event';

import { useClipboardStore } from '@/composables/useClipboardStore';
import ClipboardList from '@/components/clipboard/ClipboardList.vue';
import ClipboardStats from '@/components/clipboard/ClipboardStats.vue';
import { clipboardApi } from '@/lib/tauri';
import type { ClipboardFilter } from '@/lib/clipboardTypes';

defineOptions({ name: 'ClipboardManagerPage' });

const { t } = useI18n();
const store = useClipboardStore();
const selectedId = ref<number | null>(null);

const batchMode = ref(false);
const selectedIds = ref<Set<number>>(new Set());
const reloadCounter = ref(0);

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
  selectedId.value = null;
  clearSelection();
  void store.reload();
}

function onSearchInput(e: Event) {
  store.search.value = (e.target as HTMLInputElement).value;
  void store.reload();
}

function toggleBatchMode() {
  batchMode.value = !batchMode.value;
  if (!batchMode.value) clearSelection();
}

function toggleSelect(id: number) {
  const next = new Set(selectedIds.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  selectedIds.value = next;
}

function selectAll() {
  selectedIds.value = new Set(store.items.value.map((it) => it.id));
}

function clearSelection() {
  selectedIds.value = new Set();
}

async function batchDelete() {
  const ids = [...selectedIds.value];
  if (ids.length === 0) return;
  const msg = t('clipboard.actions.batchDeleteConfirm', { n: ids.length });
  if (!window.confirm(msg)) return;
  try {
    await clipboardApi.deleteBatch(ids);
    clearSelection();
    await store.reload();
    reloadCounter.value++;
  } catch (e) {
    store.error.value = String(e);
  }
}

async function batchFavorite(forward: boolean) {
  const ids = [...selectedIds.value];
  for (const id of ids) {
    try {
      const item = await clipboardApi.get(id);
      if (forward && !item.is_favorite) await clipboardApi.toggleFavorite(id);
      if (!forward && item.is_favorite) await clipboardApi.toggleFavorite(id);
    } catch (e) {
      store.error.value = String(e);
    }
  }
  clearSelection();
  await store.reload();
  reloadCounter.value++;
}

async function onReorder(ids: number[]) {
  try {
    await clipboardApi.reorderFavorites(ids);
    await store.reload();
  } catch (e) {
    store.error.value = String(e);
  }
}

const selectionCount = computed(() => selectedIds.value.size);
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-gradient-to-b from-slate-50 to-white">
    <div class="mx-auto flex h-full w-full max-w-6xl flex-col gap-4 px-6 py-6 pb-10">
      <header class="space-y-2">
        <h1 class="text-2xl font-bold tracking-tight text-slate-950">
          {{ t('clipboard.tool.title') }}
        </h1>
        <p class="text-sm text-slate-500">{{ t('clipboard.tool.description') }}</p>
      </header>

      <ClipboardStats :reload-signal="reloadCounter" />

      <div class="flex items-center gap-3">
        <input
          type="search"
          :placeholder="t('clipboard.search.placeholder')"
          class="flex-1 rounded-xl border border-slate-200 bg-white px-3 py-2 text-sm shadow-sm outline-none focus:border-slate-400"
          @input="onSearchInput"
        />
        <span class="text-xs text-slate-400">{{ store.total.value }} {{ t('clipboard.totalSuffix') }}</span>
        <button
          type="button"
          class="rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium transition-colors"
          :class="batchMode
            ? 'bg-slate-900 text-white border-slate-900'
            : 'bg-white text-slate-600 hover:bg-slate-100'"
          @click="toggleBatchMode"
        >
          {{ batchMode ? t('clipboard.actions.clearSelection') : t('clipboard.actions.selectAll') }}
        </button>
      </div>

      <div v-if="batchMode" class="flex flex-wrap items-center gap-2 rounded-xl border border-slate-200 bg-white px-3 py-2 text-xs">
        <span class="text-slate-500">{{ selectionCount }} / {{ store.items.value.length }}</span>
        <button type="button" class="rounded-full bg-slate-100 px-2.5 py-0.5 hover:bg-slate-200" @click="selectAll">
          {{ t('clipboard.actions.selectAll') }}
        </button>
        <button type="button" class="rounded-full bg-slate-100 px-2.5 py-0.5 hover:bg-slate-200" @click="clearSelection">
          {{ t('clipboard.actions.clearSelection') }}
        </button>
        <button type="button" class="rounded-full bg-amber-100 px-2.5 py-0.5 text-amber-700 hover:bg-amber-200" @click="batchFavorite(true)">
          {{ t('clipboard.actions.batchFavorite') }}
        </button>
        <button type="button" class="rounded-full bg-slate-100 px-2.5 py-0.5 hover:bg-slate-200" @click="batchFavorite(false)">
          {{ t('clipboard.actions.batchUnfavorite') }}
        </button>
        <button type="button" class="rounded-full bg-rose-100 px-2.5 py-0.5 text-rose-700 hover:bg-rose-200" @click="batchDelete">
          {{ t('clipboard.actions.batchDelete') }}
        </button>
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

      <section class="min-h-[400px] flex-1 rounded-2xl border border-slate-200 bg-white shadow-sm">
        <div v-if="store.error.value" class="p-5 text-sm text-rose-500">
          {{ store.error.value }}
        </div>
        <div v-else-if="store.loading.value" class="p-6 text-sm text-slate-400">
          {{ t('clipboard.loading') }}
        </div>
        <div v-else-if="store.items.value.length === 0" class="p-8 text-center text-sm text-slate-400">
          {{ t('clipboard.panel.empty') }}
        </div>
        <div v-else-if="batchMode" class="max-h-[60vh] overflow-y-auto">
          <label
            v-for="it in store.items.value"
            :key="it.id"
            class="flex cursor-pointer items-center gap-3 border-b border-slate-100 px-4 py-2 hover:bg-slate-50"
          >
            <input
              type="checkbox"
              class="h-4 w-4 shrink-0"
              :checked="selectedIds.has(it.id)"
              @change="toggleSelect(it.id)"
            />
            <span class="inline-flex shrink-0 rounded bg-slate-200/60 px-1.5 py-0.5 text-[10px] uppercase text-slate-600">
              {{ it.kind }}
            </span>
            <span v-if="it.is_favorite" class="shrink-0 text-xs text-amber-500">★</span>
            <span class="flex-1 truncate text-sm text-slate-700">{{ it.content_preview }}</span>
          </label>
        </div>
        <ClipboardList
          v-else
          :items="store.items.value"
          :selected-id="selectedId"
          :draggable="store.filter.value === 'favorite'"
          @select="(id) => (selectedId = id)"
          @activate="(id) => store.toggleFavorite(id)"
          @reorder="onReorder"
        />
      </section>
    </div>
  </div>
</template>
