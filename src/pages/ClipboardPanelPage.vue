<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useI18n } from 'vue-i18n';
import {
  Trash2,
  CheckSquare,
  Lock,
  LockOpen,
  Settings,
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
import ClipboardHoverPreview from '@/components/clipboard/ClipboardHoverPreview.vue';
import ClipboardMergePasteDialog from '@/components/clipboard/ClipboardMergePasteDialog.vue';
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

// Toolbar state ----------------------------------------------------------
const pinned = ref(false);
const batchMode = ref(false);
const selectedIds = ref<Set<number>>(new Set());
const clearDialogOpen = ref(false);

function resetBatchSelection() {
  batchMode.value = false;
  selectedIds.value = new Set();
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
  batchMode.value = !batchMode.value;
  closeMenu();
  if (!batchMode.value) selectedIds.value = new Set();
}

function onToggleSelect(id: number) {
  const s = new Set(selectedIds.value);
  if (s.has(id)) s.delete(id);
  else s.add(id);
  selectedIds.value = s;
}

async function onBatchDelete() {
  if (selectedIds.value.size === 0) return;
  const ids = Array.from(selectedIds.value);
  try {
    await clipboardApi.deleteBatch(ids);
    resetBatchSelection();
    await store.reload();
  } catch (e) {
    console.error('[clipboard] batch delete failed:', e);
    store.error.value = `${t('clipboard.errors.loadFailed')} — ${e}`;
  }
}

async function onConfirmClear() {
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

function selectById(id: number) {
  const idx = store.items.value.findIndex((it) => it.id === id);
  if (idx >= 0) selectedIndex.value = idx;
}

const preview = useHoverPreview();

function onListSelect(id: number) {
  selectById(id);
  const item = store.items.value.find((it) => it.id === id);
  if (item && item.kind === 'image') preview.onEnter(item);
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
  selectedIds,
  onPaste: paste,
  onCopy: (id: number) => clipboardApi.copy(id),
  onDelete: (id: number) => store.remove(id),
  onToggleFavorite: (id: number) => store.toggleFavorite(id),
  onError: setClipboardActionError,
  onMergeSuccess: async () => {
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
  onPaste: paste,
  onDelete: (id) => store.remove(id),
  onFavorite: (id) => store.toggleFavorite(id),
  onClose: close,
  onFocusSearch: () => searchInput.value?.focus(),
  onFilterChange: changeFilter,
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

onMounted(async () => {
  // Read initial pinned state from backend so the toolbar reflects reality.
  clipboardApi
    .isPanelPinned()
    .then((p) => {
      pinned.value = p;
    })
    .catch(() => {});

  unlistenShown = await listen('clipboard-panel-shown', async () => {
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

      <div class="flex items-center gap-0.5" data-no-drag>
        <button
          type="button"
          data-no-drag
          class="inline-flex h-7 w-7 items-center justify-center rounded text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-800"
          :title="t('clipboard.actions.clearHistory')"
          @click="clearDialogOpen = true"
        >
          <Trash2 class="h-4 w-4" />
        </button>
        <button
          type="button"
          data-no-drag
          class="inline-flex h-7 w-7 items-center justify-center rounded transition-colors"
          :class="batchMode
            ? 'bg-blue-50 text-blue-600'
            : 'text-slate-500 hover:bg-slate-100 hover:text-slate-800'"
          :title="batchMode ? t('clipboard.actions.exitBatch') : t('clipboard.actions.batchSelect')"
          @click="toggleBatchMode"
        >
          <CheckSquare class="h-4 w-4" />
        </button>
        <button
          type="button"
          data-no-drag
          class="inline-flex h-7 w-7 items-center justify-center rounded transition-colors"
          :class="pinned
            ? 'bg-amber-50 text-amber-600'
            : 'text-slate-500 hover:bg-slate-100 hover:text-slate-800'"
          :title="pinned ? t('clipboard.actions.unlockWindow') : t('clipboard.actions.lockWindow')"
          @click="togglePinned"
        >
          <Lock v-if="pinned" class="h-4 w-4" />
          <LockOpen v-else class="h-4 w-4" />
        </button>
        <button
          type="button"
          data-no-drag
          class="inline-flex h-7 w-7 items-center justify-center rounded text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-800"
          :title="t('clipboard.actions.openSettings')"
          @click="openSettings"
        >
          <Settings class="h-4 w-4" />
        </button>
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

    <div v-if="batchMode" class="flex items-center justify-between gap-2 border-b border-blue-200 bg-blue-50/80 px-3 py-1.5">
      <span class="text-xs text-slate-600">
        {{ t('clipboard.batchBar.selected', { n: selectedIds.size }) }}
        <span class="ml-1.5 text-slate-400">{{ t('clipboard.batchBar.shiftHint') }}</span>
      </span>
      <div class="flex items-center gap-1">
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
          :disabled="selectedIds.size === 0"
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

    <div class="px-3 pt-2.5 pb-2">
      <input
        ref="searchInput"
        v-model="store.search.value"
        type="search"
        :placeholder="t('clipboard.search.placeholder')"
        class="w-full rounded-lg border border-slate-200 bg-slate-50 px-3 py-1.5 text-sm outline-none focus:border-slate-400 focus:bg-white"
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
          : 'bg-slate-100 text-slate-600 hover:bg-slate-200'"
        @click="setFilter(f)"
      >
        {{ t(`clipboard.filter.${f}`) }}
      </button>
    </div>

    <div
      class="flex-1 overflow-hidden px-1 pb-2"
      @mouseleave="preview.onLeave()"
      @wheel="preview.onWheelZoom($event)"
    >
      <div v-if="store.items.value.length === 0" class="flex h-full items-center justify-center p-6 text-center text-sm text-slate-400">
        {{ store.search.value ? t('clipboard.panel.noMatch') : t('clipboard.panel.empty') }}
      </div>
      <ClipboardList
        v-else
        :key="listKey"
        :items="store.items.value"
        :selected-id="selectedId"
        :compact="true"
        :draggable="store.filter.value === 'favorite'"
        :batch-mode="batchMode"
        :selected-ids="selectedIds"
        :show-delete-button="!batchMode"
        :show-favorite-button="!batchMode"
        @select="onListSelect"
        @activate="(id: number) => paste(id, false)"
        @toggle="onToggleSelect"
        @favorite="(id: number) => store.toggleFavorite(id)"
        @remove="(id: number) => store.remove(id)"
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

  <ClipboardHoverPreview
    v-if="preview.activeItem.value"
    :item="preview.activeItem.value"
    :scale="preview.scale.value"
  />

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
    :selected-count="selectedIds.size"
    :pending="mergePending"
    @close="closeMergeDialog"
    @confirm="confirmMergePaste"
  />
</template>
