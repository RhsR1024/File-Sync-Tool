<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useI18n } from 'vue-i18n';
import {
  Trash2,
  X,
} from 'lucide-vue-next';

import { useClipboardStore } from '@/composables/useClipboardStore';
import { useClipboardContextMenu } from '@/composables/useClipboardContextMenu';
import { useClipboardHotkey } from '@/composables/useClipboardHotkey';
import { useHoverPreview } from '@/composables/useHoverPreview';
import type { ClipboardContextActionId } from '@/composables/clipboardContextMenuHelpers';
import ClipboardCardMenu from '@/components/clipboard/ClipboardCardMenu.vue';
import ClipboardFileDetailsDialog from '@/components/clipboard/ClipboardFileDetailsDialog.vue';
import ClipboardList from '@/components/clipboard/ClipboardList.vue';
import ClipboardMergePasteDialog from '@/components/clipboard/ClipboardMergePasteDialog.vue';
import ClipboardSearchBox from '@/components/clipboard/ClipboardSearchBox.vue';
import ClipboardToolbar from '@/components/clipboard/ClipboardToolbar.vue';
import { buildClipboardToolbarLayout } from '@/lib/clipboardSettingsUi';
import { clipboardApi } from '@/lib/tauri';
import type { ClipboardFilter } from '@/lib/clipboardTypes';

defineOptions({ name: 'ClipboardPanelPage' });

const { t } = useI18n();
const store = useClipboardStore();

const selectedIndex = ref(0);
const searchInput = ref<{ focus: () => void } | null>(null);
const previewDelayMs = ref(500);

const filters: ClipboardFilter[] = ['all', 'text', 'image', 'file', 'favorite'];

const selectedId = computed<number | null>(
  () => store.items.value[selectedIndex.value]?.id ?? null,
);

// Toolbar state ----------------------------------------------------------
const pinned = ref(false);
const clearDialogOpen = ref(false);

function resetBatchSelection() {
  store.setBatchMode(false);
}

function setClipboardActionError(error: unknown, action: ClipboardContextActionId) {
  store.error.value = `${t('clipboard.errors.actionFailed', {
    action: t(`clipboard.actionNames.${action}`),
  })} ${error}`;
}

async function togglePinned() {
  const next = !pinned.value;
  try {
    await clipboardApi.setPanelPinned(next);
    pinned.value = next;
  } catch (e) {
    console.error('[clipboard] setPanelPinned failed:', e);
  }
}

function toggleBatchMode() {
  preview.hideNow();
  store.toggleBatchMode();
  closeMenu();
}

function onToggleSelect(payload: { id: number; shiftKey: boolean }) {
  selectById(payload.id);
  store.toggleSelection(payload.id, payload.shiftKey);
}

async function onBatchDelete() {
  const ids = store.orderedSelectedIds.value;
  if (ids.length === 0) return;
  preview.hideNow();
  try {
    await clipboardApi.deleteBatch(ids);
    resetBatchSelection();
    await store.reload();
  } catch (e) {
    console.error('[clipboard] batch delete failed:', e);
    store.error.value = `${t('clipboard.errors.loadFailed')} — ${e}`;
  }
}

async function onBatchFavorite(nextFavorite: boolean) {
  const ids = store.orderedSelectedIds.value;
  if (ids.length === 0) return;

  for (const id of ids) {
    try {
      const item = await clipboardApi.get(id);
      if (nextFavorite && !item.is_favorite) await clipboardApi.toggleFavorite(id);
      if (!nextFavorite && item.is_favorite) await clipboardApi.toggleFavorite(id);
    } catch (error) {
      console.error('[clipboard] batch favorite failed:', error);
      store.error.value = `${t('clipboard.errors.saveFailed')} 鈥?${error}`;
      return;
    }
  }

  resetBatchSelection();
  await store.reload();
}

async function onConfirmClear() {
  preview.hideNow();
  try {
    await clipboardApi.clear(true); // keep_favorites = true
    clearDialogOpen.value = false;
    selectedIndex.value = 0;
    await store.reload();
  } catch (e) {
    console.error('[clipboard] clear failed:', e);
    store.error.value = `${t('clipboard.errors.loadFailed')} — ${e}`;
    clearDialogOpen.value = false;
  }
}

async function openSettings() {
  try {
    preview.hideNow();
    await clipboardApi.openSettings();
  } catch (e) {
    console.error('[clipboard] openSettings failed:', e);
  }
}

// ------------------------------------------------------------------------

async function paste(id: number, plain: boolean) {
  try {
    if (plain) await clipboardApi.pastePlain(id);
    else await clipboardApi.paste(id);
  } catch (e) {
    console.error('[clipboard] paste failed:', e);
    store.error.value = `${t('clipboard.errors.pasteFailed')} — ${e}`;
  }
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

function close() {
  preview.hideNow();
  void getCurrentWindow().hide();
}

// Explicit drag handler (stable on opaque undecorated windows).
function onHeaderMouseDown(e: MouseEvent) {
  if (e.button !== 0) return;
  const target = e.target as HTMLElement | null;
  if (target && target.closest('button, input, a, [data-no-drag]')) return;
  void getCurrentWindow().startDragging();
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

function onSearchChange(value: string) {
  store.search.value = value;
  void store.reload();
}

function selectById(id: number) {
  const idx = store.items.value.findIndex((it) => it.id === id);
  if (idx >= 0) selectedIndex.value = idx;
}

async function refreshPreviewSettings() {
  try {
    const settings = await clipboardApi.getSettings();
    previewDelayMs.value = Math.max(0, settings.preview.delay_ms);
  } catch (error) {
    console.error('[clipboard] getSettings for preview failed:', error);
  }
}

const preview = useHoverPreview({
  delayMs: () => previewDelayMs.value,
  onError: (error) => {
    console.error('[clipboard] preview command failed:', error);
  },
});

function onListSelect(id: number) {
  selectById(id);
  preview.onItemChange(store.items.value.find((it) => it.id === id) ?? null);
}

async function onRemoveItem(id: number) {
  preview.hideNow();
  await store.remove(id);
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
  onPaste: paste,
  onCopy: (id: number) => clipboardApi.copy(id),
  onDelete: (id: number) => onRemoveItem(id),
  onToggleFavorite: (id: number) => store.toggleFavorite(id),
  onError: setClipboardActionError,
  onMergeSuccess: async () => {
    preview.hideNow();
    resetBatchSelection();
    await store.reload();
  },
});

function onListMenu(payload: { item: (typeof store.items.value)[number]; x: number; y: number }) {
  selectById(payload.item.id);
  openMenu(payload.item, { x: payload.x, y: payload.y });
}

async function onOpenDetailPath(path: string) {
  try {
    await clipboardApi.openInExplorer(path);
  } catch (error) {
    setClipboardActionError(error, 'openInExplorer');
  }
}

useClipboardHotkey({
  items: store.items,
  selectedIndex,
  filter: store.filter,
  searchValue: store.search,
  enabled: computed(() => store.settings.value.navigation.enabled),
  onPaste: paste,
  onDelete: onRemoveItem,
  onFavorite: (id) => store.toggleFavorite(id),
  onClose: close,
  onFocusSearch: () => searchInput.value?.focus(),
  onFilterChange: changeFilter,
  onSearchChange,
});

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
const showCounter = ref(0);
const listKey = computed(() => `${store.filter.value}-${showCounter.value}`);
const toolbarLayout = computed(() =>
  buildClipboardToolbarLayout(store.settings.value.toolbar, ['batch', 'settings', 'lock']),
);

onMounted(async () => {
  await refreshPreviewSettings();
  // Read initial pinned state from backend so the toolbar reflects reality.
  clipboardApi
    .isPanelPinned()
    .then((p) => {
      pinned.value = p;
    })
    .catch(() => {});

  unlistenShown = await listen('clipboard-panel-shown', async () => {
    preview.hideNow();
    await refreshPreviewSettings();
    store.search.value = '';
    selectedIndex.value = 0;
    resetBatchSelection();
    await store.reload();
    await nextTick();
    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });
    showCounter.value += 1;
    searchInput.value?.focus();
  });
  unlistenItemAdded = await store.startListening();
});

onBeforeUnmount(() => {
  preview.hideNow();
  unlistenShown?.();
  unlistenItemAdded?.();
});
</script>

<template>
  <div class="flex h-screen w-screen flex-col overflow-hidden bg-white">
    <header
      class="flex select-none items-center justify-between border-b border-slate-200 px-3 py-2.5"
      data-tauri-drag-region
      @mousedown="onHeaderMouseDown"
    >
      <span class="pointer-events-none truncate text-sm font-semibold text-slate-700">
        {{ t('clipboard.tool.title') }}
      </span>

      <div class="flex items-center gap-1" data-no-drag>
        <button
          type="button"
          data-no-drag
          class="inline-flex h-7 w-7 items-center justify-center rounded text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-800"
          :title="t('clipboard.actions.clearHistory')"
          @click="clearDialogOpen = true"
        >
          <Trash2 class="h-4 w-4" />
        </button>
        <ClipboardToolbar
          :items="toolbarLayout.actionItems"
          :batch-mode="store.batchMode.value"
          :locked="pinned"
          compact
          @batch="toggleBatchMode"
          @lock="togglePinned"
          @settings="openSettings"
        />
        <span class="mx-0.5 h-5 w-px bg-slate-200" aria-hidden />
        <button
          type="button"
          data-no-drag
          class="inline-flex h-7 w-7 items-center justify-center rounded text-slate-500 transition-colors hover:bg-red-50 hover:text-red-600"
          :title="t('clipboard.actions.close')"
          @click="close"
        >
          <X class="h-4 w-4" />
        </button>
      </div>
    </header>

    <div v-if="store.batchMode.value" class="flex items-center justify-between gap-2 border-b border-blue-200 bg-blue-50/80 px-3 py-1.5">
      <span class="text-xs text-slate-600">
        {{ t('clipboard.batchBar.selected', { n: store.selectedIds.value.size }) }}
        <span class="ml-1.5 text-slate-400">{{ t('clipboard.batchBar.shiftHint') }}</span>
      </span>
      <div class="flex items-center gap-1">
        <button
          type="button"
          class="rounded bg-amber-500/10 px-2 py-1 text-xs text-amber-700 transition-colors hover:bg-amber-500/20 disabled:opacity-40"
          :disabled="store.selectedIds.value.size === 0"
          @click="onBatchFavorite(true)"
        >
          {{ t('clipboard.actions.batchFavorite') }}
        </button>
        <button
          type="button"
          class="rounded bg-slate-900/10 px-2 py-1 text-xs text-slate-700 transition-colors hover:bg-slate-900/20 disabled:opacity-40"
          :disabled="store.selectedIds.value.size === 0"
          @click="onBatchFavorite(false)"
        >
          {{ t('clipboard.actions.batchUnfavorite') }}
        </button>
        <button
          type="button"
          class="rounded bg-slate-900/10 px-2 py-1 text-xs text-slate-700 transition-colors hover:bg-slate-900/20 disabled:opacity-40"
          :disabled="!canMergeSelection"
          @click="openMergeDialog"
        >
          {{ t('clipboard.actions.mergePaste') }}
        </button>
        <button
          type="button"
          class="rounded bg-red-500/10 px-2 py-1 text-xs text-red-600 transition-colors hover:bg-red-500/20 disabled:opacity-40"
          :disabled="store.selectedIds.value.size === 0"
          @click="onBatchDelete"
        >
          {{ t('clipboard.actions.batchDelete') }}
        </button>
        <button
          type="button"
          class="rounded px-2 py-1 text-xs text-slate-600 transition-colors hover:bg-slate-100"
          @click="toggleBatchMode"
        >
          {{ t('clipboard.confirm.cancel') }}
        </button>
      </div>
    </div>

    <div v-if="toolbarLayout.showSearch" class="px-3 pt-2.5 pb-2">
      <ClipboardSearchBox
        ref="searchInput"
        :model-value="store.search.value"
        :placeholder="t('clipboard.search.placeholder')"
        @update:model-value="onSearchChange"
        @clear="onSearchChange('')"
      />
    </div>

    <div v-if="toolbarLayout.showFilter" class="flex flex-wrap gap-1 px-3 pb-2">
      <button
        v-for="f in filters"
        :key="f"
        type="button"
        class="rounded-full px-2.5 py-0.5 text-xs transition-colors"
        :class="store.filter.value === f
          ? 'bg-slate-900 text-white shadow-sm'
          : 'bg-slate-100 text-slate-600 hover:bg-slate-200'"
        @click="setFilter(f)"
      >
        {{ t(`clipboard.filter.${f}`) }}
      </button>
    </div>

    <div
      class="flex-1 overflow-hidden px-1 pb-2"
      @mouseleave="preview.onLeave()"
    >
      <div v-if="store.items.value.length === 0" class="flex h-full items-center justify-center p-6 text-center text-sm text-slate-400">
        {{ store.search.value ? t('clipboard.panel.noMatch') : t('clipboard.panel.empty') }}
      </div>
      <ClipboardList
        v-else
        :key="listKey"
        :items="store.items.value"
        :selected-id="selectedId"
        :display-settings="store.settings.value.display"
        :highlight-keywords="store.searchKeywords.value"
        :compact="true"
        :draggable="!store.batchMode.value && store.filter.value === 'favorite'"
        :batch-mode="store.batchMode.value"
        :selected-ids="store.selectedIds.value"
        :show-delete-button="!store.batchMode.value"
        :show-favorite-button="!store.batchMode.value"
        @select="onListSelect"
        @activate="(id: number) => paste(id, false)"
        @toggle="onToggleSelect"
        @favorite="(id: number) => store.toggleFavorite(id)"
        @remove="onRemoveItem"
        @menu="onListMenu"
        @reorder="onReorder"
      />
    </div>

    <!-- Clear-history confirmation dialog -->
    <div
      v-if="clearDialogOpen"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/30"
      @click.self="clearDialogOpen = false"
    >
      <div class="w-[320px] rounded-xl bg-white p-4 shadow-2xl">
        <h3 class="text-sm font-semibold text-slate-800">
          {{ t('clipboard.confirm.clearTitle') }}
        </h3>
        <p class="mt-2 text-xs leading-relaxed text-slate-500">
          {{ t('clipboard.confirm.clearBody') }}
        </p>
        <div class="mt-4 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-md px-3 py-1.5 text-xs text-slate-600 transition-colors hover:bg-slate-100"
            @click="clearDialogOpen = false"
          >
            {{ t('clipboard.confirm.cancel') }}
          </button>
          <button
            type="button"
            class="rounded-md bg-red-500 px-3 py-1.5 text-xs text-white transition-colors hover:bg-red-600"
            @click="onConfirmClear"
          >
            {{ t('clipboard.confirm.clearConfirm') }}
          </button>
        </div>
      </div>
    </div>
  </div>

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
</template>
