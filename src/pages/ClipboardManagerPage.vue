<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import type { UnlistenFn } from '@tauri-apps/api/event';

import { useClipboardContextMenu } from '@/composables/useClipboardContextMenu';
import { useClipboardStore } from '@/composables/useClipboardStore';
import type { ClipboardContextActionId } from '@/composables/clipboardContextMenuHelpers';
import ClipboardCardMenu from '@/components/clipboard/ClipboardCardMenu.vue';
import ClipboardFileDetailsDialog from '@/components/clipboard/ClipboardFileDetailsDialog.vue';
import ClipboardList from '@/components/clipboard/ClipboardList.vue';
import ClipboardMergePasteDialog from '@/components/clipboard/ClipboardMergePasteDialog.vue';
import ClipboardStats from '@/components/clipboard/ClipboardStats.vue';
import ClipboardSettingsPanel from '@/components/clipboard/ClipboardSettingsPanel.vue';
import { clipboardApi } from '@/lib/tauri';
import type { ClipboardFilter } from '@/lib/clipboardTypes';

defineOptions({ name: 'ClipboardManagerPage' });

const { t } = useI18n();
const store = useClipboardStore();
const selectedId = ref<number | null>(null);

const reloadCounter = ref(0);
const copyToast = ref<string | null>(null);
let copyToastTimer: number | null = null;

function resetBatchSelection() {
  store.setBatchMode(false);
}

function setClipboardActionError(error: unknown, action: ClipboardContextActionId) {
  store.error.value = `${t('clipboard.errors.actionFailed', {
    action: t(`clipboard.actionNames.${action}`),
  })} ${error}`;
}

function flashCopyToast(message: string) {
  copyToast.value = message;
  if (copyToastTimer !== null) window.clearTimeout(copyToastTimer);
  copyToastTimer = window.setTimeout(() => {
    copyToast.value = null;
    copyToastTimer = null;
  }, 1600);
}

async function copyToClipboard(id: number) {
  try {
    await clipboardApi.copy(id);
    flashCopyToast(t('clipboard.actions.copied'));
  } catch (e) {
    console.error('[clipboard] copy failed:', e);
    store.error.value = `${t('clipboard.errors.saveFailed')} — ${e}`;
  }
}

const filters: ClipboardFilter[] = ['all', 'text', 'image', 'file', 'favorite'];

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  unlisten = await store.startListening();
  await store.reload();
});

onBeforeUnmount(() => {
  unlisten?.();
  if (copyToastTimer !== null) {
    window.clearTimeout(copyToastTimer);
    copyToastTimer = null;
  }
});

function setFilter(f: ClipboardFilter) {
  store.filter.value = f;
  selectedId.value = null;
  store.clearSelection();
  void store.reload();
}

function onSearchInput(e: Event) {
  store.search.value = (e.target as HTMLInputElement).value;
  void store.reload();
}

function toggleBatchMode() {
  store.toggleBatchMode();
  closeMenu();
}

function toggleSelect(payload: { id: number; shiftKey: boolean }) {
  selectedId.value = payload.id;
  store.toggleSelection(payload.id, payload.shiftKey);
}

function selectAll() {
  store.selectAllVisible();
}

function clearSelection() {
  store.clearSelection();
}

async function batchDelete() {
  const ids = store.orderedSelectedIds.value;
  if (ids.length === 0) return;
  const msg = t('clipboard.actions.batchDeleteConfirm', { n: ids.length });
  if (!window.confirm(msg)) return;
  try {
    await clipboardApi.deleteBatch(ids);
    clearSelection();
    await store.reload();
    reloadCounter.value++;
  } catch (e) {
    console.error('[clipboard] batchDelete failed:', e);
    store.error.value = `${t('clipboard.errors.saveFailed')} — ${e}`;
  }
}

async function batchFavorite(forward: boolean) {
  const ids = store.orderedSelectedIds.value;
  for (const id of ids) {
    try {
      const item = await clipboardApi.get(id);
      if (forward && !item.is_favorite) await clipboardApi.toggleFavorite(id);
      if (!forward && item.is_favorite) await clipboardApi.toggleFavorite(id);
    } catch (e) {
      console.error('[clipboard] batchFavorite failed:', e);
      store.error.value = `${t('clipboard.errors.saveFailed')} — ${e}`;
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
    console.error('[clipboard] reorder failed:', e);
    store.error.value = `${t('clipboard.errors.saveFailed')} — ${e}`;
  }
}

const selectionCount = computed(() => store.selectedIds.value.size);

async function pasteFromContextMenu(id: number, plain: boolean) {
  try {
    if (plain) await clipboardApi.pastePlain(id);
    else await clipboardApi.paste(id);
  } catch (error) {
    setClipboardActionError(error, plain ? 'pastePlain' : 'paste');
  }
}

const {
  canMergeSelection,
  closeMenu,
  closeMergeDialog,
  confirmMergePaste,
  fileDetailsItem,
  fileDetailsOpen,
  fileDetailsStatuses,
  fileStatusLoading,
  menuItems,
  menuOpen,
  menuPosition,
  mergeDialogOpen,
  mergePending,
  mergeSeparatorInput,
  openMenu,
  openMergeDialog,
  runAction,
} = useClipboardContextMenu({
  selectedIds: store.selectedIds,
  selectedIdOrder: store.orderedSelectedIds,
  onPaste: pasteFromContextMenu,
  onCopy: copyToClipboard,
  onDelete: async (id: number) => {
    await store.remove(id);
    reloadCounter.value++;
  },
  onToggleFavorite: async (id: number) => {
    await store.toggleFavorite(id);
    reloadCounter.value++;
  },
  onError: setClipboardActionError,
  onMergeSuccess: async () => {
    resetBatchSelection();
    await store.reload();
    reloadCounter.value++;
  },
});

function onListMenu(payload: { item: (typeof store.items.value)[number]; x: number; y: number }) {
  selectedId.value = payload.item.id;
  openMenu(payload.item, { x: payload.x, y: payload.y });
}

async function onOpenDetailPath(path: string) {
  try {
    await clipboardApi.openInExplorer(path);
  } catch (error) {
    setClipboardActionError(error, 'openInExplorer');
  }
}
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

      <details class="rounded-2xl border border-slate-200 bg-white">
        <summary class="cursor-pointer px-5 py-3 text-sm font-medium text-slate-700">
          {{ t('clipboard.settings.title') }}
        </summary>
        <div class="border-t border-slate-100">
          <ClipboardSettingsPanel />
        </div>
      </details>

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
          :class="store.batchMode.value
            ? 'bg-slate-900 text-white border-slate-900'
            : 'bg-white text-slate-600 hover:bg-slate-100'"
          @click="toggleBatchMode"
        >
          {{ store.batchMode.value ? t('clipboard.actions.clearSelection') : t('clipboard.actions.selectAll') }}
        </button>
      </div>

      <div v-if="store.batchMode.value" class="flex flex-wrap items-center gap-2 rounded-xl border border-slate-200 bg-white px-3 py-2 text-xs">
        <span class="text-slate-500">{{ selectionCount }} / {{ store.items.value.length }}</span>
        <span class="text-slate-400">{{ t('clipboard.batchBar.shiftHint') }}</span>
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
        <button
          type="button"
          class="rounded-full bg-slate-900/10 px-2.5 py-0.5 text-slate-700 hover:bg-slate-900/20 disabled:opacity-40"
          :disabled="!canMergeSelection"
          @click="openMergeDialog"
        >
          {{ t('clipboard.actions.mergePaste') }}
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
          {{ store.search.value ? t('clipboard.panel.noMatch') : t('clipboard.panel.empty') }}
        </div>
        <div v-else-if="store.batchMode.value" class="max-h-[60vh] overflow-y-auto">
          <label
            v-for="it in store.items.value"
            :key="it.id"
            class="flex cursor-pointer items-center gap-3 border-b border-slate-100 px-4 py-2 hover:bg-slate-50"
            @click.prevent="toggleSelect({ id: it.id, shiftKey: $event.shiftKey })"
          >
            <input
              type="checkbox"
              class="h-4 w-4 shrink-0"
              :checked="store.selectedIds.value.has(it.id)"
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
          :show-favorite-button="true"
          @select="(id) => (selectedId = id)"
          @activate="(id) => copyToClipboard(id)"
          @favorite="(id) => store.toggleFavorite(id)"
          @menu="onListMenu"
          @reorder="onReorder"
        />
      </section>
    </div>

    <transition
      enter-active-class="transition-all duration-200 ease-out"
      enter-from-class="opacity-0 translate-y-2"
      enter-to-class="opacity-100 translate-y-0"
      leave-active-class="transition-all duration-150 ease-in"
      leave-from-class="opacity-100 translate-y-0"
      leave-to-class="opacity-0 translate-y-2"
    >
      <div
        v-if="copyToast"
        class="fixed bottom-6 left-1/2 z-50 -translate-x-1/2 rounded-lg bg-slate-900/90 px-4 py-2 text-sm font-medium text-white shadow-lg"
        role="status"
      >
        {{ copyToast }}
      </div>
    </transition>

    <ClipboardCardMenu
      :open="menuOpen"
      :x="menuPosition.x"
      :y="menuPosition.y"
      :items="menuItems"
      @close="closeMenu"
      @select="runAction"
    />

    <ClipboardFileDetailsDialog
      :open="fileDetailsOpen"
      :item="fileDetailsItem"
      :statuses="fileDetailsStatuses"
      :busy="fileStatusLoading"
      @close="fileDetailsOpen = false"
      @open-path="onOpenDetailPath"
    />

    <ClipboardMergePasteDialog
      v-model="mergeSeparatorInput"
      :open="mergeDialogOpen"
      :selected-count="store.selectedIds.value.size"
      :pending="mergePending"
      @close="closeMergeDialog"
      @confirm="confirmMergePaste"
    />
  </div>
</template>
