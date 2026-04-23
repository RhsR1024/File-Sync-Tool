<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useI18n } from 'vue-i18n';
import { Trash2, X } from 'lucide-vue-next';

import { useClipboardStore } from '@/composables/useClipboardStore';
import { useClipboardContextMenu } from '@/composables/useClipboardContextMenu';
import { useClipboardHotkey } from '@/composables/useClipboardHotkey';
import { useHoverPreview } from '@/composables/useHoverPreview';
import type { ClipboardContextActionId } from '@/composables/clipboardContextMenuHelpers';
import ClipboardCardMenu from '@/components/clipboard/ClipboardCardMenu.vue';
import ClipboardFileDetailsDialog from '@/components/clipboard/ClipboardFileDetailsDialog.vue';
import ClipboardList from '@/components/clipboard/ClipboardList.vue';
import ClipboardMergePasteDialog from '@/components/clipboard/ClipboardMergePasteDialog.vue';
import ClipboardPanelGroupMenu from '@/components/clipboard/ClipboardPanelGroupMenu.vue';
import ClipboardPinnedSection from '@/components/clipboard/ClipboardPinnedSection.vue';
import ClipboardSearchBox from '@/components/clipboard/ClipboardSearchBox.vue';
import ClipboardToolbar from '@/components/clipboard/ClipboardToolbar.vue';
import {
  CLIPBOARD_PANEL_USE_NATIVE_DRAG_REGION,
  shouldStartClipboardPanelDrag,
} from '@/lib/clipboardPanelDrag';
import { buildClipboardToolbarLayout } from '@/lib/clipboardSettingsUi';
import { clipboardApi } from '@/lib/tauri';
import type { ClipboardFilter, ClipboardGroup, ClipboardItem } from '@/lib/clipboardTypes';

defineOptions({ name: 'ClipboardPanelPage' });

const { t } = useI18n();
const store = useClipboardStore();

const selectedIndex = ref(0);
const searchInput = ref<{ focus: () => void } | null>(null);
const previewDelayMs = ref(500);
const clearDialogOpen = ref(false);
const panelLocked = ref(false);
const filters: ClipboardFilter[] = ['all', 'text', 'image', 'file', 'favorite'];

const selectedId = computed<number | null>(
  () => store.visibleItems.value[selectedIndex.value]?.id ?? null,
);
const hasVisibleItems = computed(() => store.visibleItems.value.length > 0);
const toolbarLayout = computed(() =>
  buildClipboardToolbarLayout(store.settings.value.toolbar, ['batch', 'settings', 'lock']),
);

function resetBatchSelection() {
  store.setBatchMode(false);
}

function setClipboardActionError(error: unknown, action: ClipboardContextActionId) {
  const actionNameKey = action.startsWith('moveToGroup:')
    ? 'clipboard.actionNames.moveToGroup'
    : `clipboard.actionNames.${action}`;
  store.error.value = `${t('clipboard.errors.actionFailed', {
    action: t(actionNameKey),
  })} ${error}`;
}

async function togglePanelLocked() {
  const next = !panelLocked.value;
  try {
    await clipboardApi.setPanelPinned(next);
    panelLocked.value = next;
  } catch (error) {
    console.error('[clipboard] setPanelPinned failed:', error);
  }
}

function toggleBatchMode() {
  preview.hideNow();
  store.toggleBatchMode();
  closeMenu();
}

function selectById(id: number) {
  const nextIndex = store.visibleItems.value.findIndex((item) => item.id === id);
  if (nextIndex >= 0) selectedIndex.value = nextIndex;
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
  } catch (error) {
    console.error('[clipboard] batch delete failed:', error);
    store.error.value = `${t('clipboard.errors.loadFailed')} - ${error}`;
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
      store.error.value = `${t('clipboard.errors.saveFailed')} - ${error}`;
      return;
    }
  }

  resetBatchSelection();
  await store.reload();
}

async function onConfirmClear() {
  preview.hideNow();
  try {
    await clipboardApi.clear(true);
    clearDialogOpen.value = false;
    selectedIndex.value = 0;
    await store.reload();
  } catch (error) {
    console.error('[clipboard] clear failed:', error);
    store.error.value = `${t('clipboard.errors.loadFailed')} - ${error}`;
    clearDialogOpen.value = false;
  }
}

async function openSettings() {
  try {
    preview.hideNow();
    await clipboardApi.openSettings();
  } catch (error) {
    console.error('[clipboard] openSettings failed:', error);
  }
}

async function paste(id: number, plain: boolean) {
  try {
    if (plain) await clipboardApi.pastePlain(id);
    else await clipboardApi.paste(id);
  } catch (error) {
    console.error('[clipboard] paste failed:', error);
    store.error.value = `${t('clipboard.errors.pasteFailed')} - ${error}`;
  }
}

async function onReorder(ids: number[]) {
  try {
    await clipboardApi.reorderFavorites(ids);
    await store.reload();
  } catch (error) {
    console.error('[clipboard] reorder failed:', error);
    store.error.value = `${t('clipboard.errors.saveFailed')} - ${error}`;
  }
}

function close() {
  preview.hideNow();
  void getCurrentWindow().hide();
}

function onHeaderMouseDown(event: MouseEvent) {
  if (!shouldStartClipboardPanelDrag(event)) return;
  void getCurrentWindow().startDragging();
}

function onSearchChange(value: string) {
  store.search.value = value;
  void store.reload();
}

function changeFilter(direction: 1 | -1) {
  const currentIndex = filters.indexOf(store.filter.value);
  const next = filters[(currentIndex + direction + filters.length) % filters.length];
  setFilter(next);
}

function setFilter(filter: ClipboardFilter) {
  preview.hideNow();
  store.filter.value = filter;
  store.clearSelection();
  selectedIndex.value = 0;
  void store.reload();
}

function setGroup(groupId: number | null) {
  preview.hideNow();
  store.selectGroup(groupId);
  store.clearSelection();
  selectedIndex.value = 0;
  void store.reload();
}

async function createGroup(name: string) {
  await store.createGroup(name);
  selectedIndex.value = 0;
}

async function renameGroup(payload: { id: number; name: string }) {
  await store.renameGroup(payload.id, payload.name);
}

async function deleteGroup(group: ClipboardGroup) {
  if (!window.confirm(t('clipboard.groups.deleteConfirm', { name: group.name }))) return;
  preview.hideNow();
  await store.deleteGroup(group.id);
  selectedIndex.value = 0;
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
  preview.onItemChange(store.visibleItems.value.find((item) => item.id === id) ?? null);
}

async function onRemoveItem(id: number) {
  preview.hideNow();
  await store.remove(id);
}

async function onToggleItemPin(id: number) {
  preview.hideNow();
  await store.togglePin(id);
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
  groups: store.groups,
  selectedIds: store.selectedIds,
  selectedIdOrder: store.orderedSelectedIds,
  onPaste: paste,
  onCopy: (id: number) => clipboardApi.copy(id),
  onDelete: (id: number) => onRemoveItem(id),
  onToggleFavorite: (id: number) => store.toggleFavorite(id),
  onTogglePin: (id: number) => onToggleItemPin(id),
  onMoveToGroup: async (id: number, groupId: number | null) => {
    preview.hideNow();
    await store.moveToGroup(id, groupId);
  },
  onError: setClipboardActionError,
  onMergeSuccess: async () => {
    preview.hideNow();
    resetBatchSelection();
    await store.reload();
  },
});

function onListMenu(payload: { item: ClipboardItem; x: number; y: number }) {
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
  items: store.visibleItems,
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
  () => store.visibleItems.value.length,
  (length) => {
    if (selectedIndex.value >= length) {
      selectedIndex.value = Math.max(0, length - 1);
    }
  },
);

let unlistenShown: UnlistenFn | null = null;
let unlistenStore: UnlistenFn | null = null;
const showCounter = ref(0);
const listKey = computed(
  () => `${store.filter.value}-${store.selectedGroupId.value ?? 'all'}-${showCounter.value}`,
);

onMounted(async () => {
  await refreshPreviewSettings();
  clipboardApi
    .isPanelPinned()
    .then((value) => {
      panelLocked.value = value;
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
  unlistenStore = await store.startListening();
});

onBeforeUnmount(() => {
  preview.hideNow();
  unlistenShown?.();
  unlistenStore?.();
});
</script>

<template>
  <div class="flex h-screen w-screen overflow-hidden bg-slate-200 p-px">
    <div class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[15px] bg-white">
      <header
        class="flex select-none items-center justify-between px-3 py-2.5"
        :data-tauri-drag-region="CLIPBOARD_PANEL_USE_NATIVE_DRAG_REGION ? '' : undefined"
        @mousedown="onHeaderMouseDown"
      >
      <span class="pointer-events-none truncate text-sm font-semibold text-slate-700">
        {{ t('clipboard.tool.title') }}
      </span>

      <div class="flex items-center gap-1" data-no-drag>
        <button
          type="button"
          class="inline-flex h-7 w-7 items-center justify-center rounded text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-800"
          :title="t('clipboard.actions.clearHistory')"
          @click="clearDialogOpen = true"
        >
          <Trash2 class="h-4 w-4" />
        </button>
        <ClipboardToolbar
          :items="toolbarLayout.actionItems"
          :batch-mode="store.batchMode.value"
          :locked="panelLocked"
          compact
          @batch="toggleBatchMode"
          @lock="togglePanelLocked"
          @settings="openSettings"
        />
        <span class="mx-0.5 h-5 w-px bg-slate-200" aria-hidden />
        <button
          type="button"
          class="inline-flex h-7 w-7 items-center justify-center rounded text-slate-500 transition-colors hover:bg-red-50 hover:text-red-600"
          :title="t('clipboard.actions.close')"
          @click="close"
        >
          <X class="h-4 w-4" />
        </button>
      </div>
    </header>

    <div
      v-if="store.batchMode.value"
      class="flex items-center justify-between gap-2 border-b border-blue-200 bg-blue-50/80 px-3 py-1.5"
    >
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

    <div class="flex min-h-0 flex-1 flex-col overflow-hidden border-t border-slate-100">
      <div v-if="toolbarLayout.showSearch" class="px-3 pt-2.5 pb-2">
        <ClipboardSearchBox
          ref="searchInput"
          :model-value="store.search.value"
          :placeholder="t('clipboard.search.placeholder')"
          @update:model-value="onSearchChange"
          @clear="onSearchChange('')"
        />
      </div>

      <div
        class="min-h-0 flex-1 overflow-hidden px-2"
        @mouseleave="preview.onLeave()"
      >
        <div v-if="store.error.value" class="flex h-full items-center justify-center p-6 text-center text-sm text-rose-500">
          {{ store.error.value }}
        </div>

        <div v-else-if="!hasVisibleItems" class="flex h-full items-center justify-center p-6 text-center text-sm text-slate-400">
          {{ store.search.value ? t('clipboard.panel.noMatch') : t('clipboard.panel.empty') }}
        </div>

        <div v-else-if="store.batchMode.value" class="h-full pb-2">
          <ClipboardList
            :key="listKey"
            :items="store.visibleItems.value"
            :selected-id="selectedId"
            :display-settings="store.settings.value.display"
            :highlight-keywords="store.searchKeywords.value"
            :compact="true"
            :draggable="false"
            :batch-mode="true"
            :selected-ids="store.selectedIds.value"
            @select="onListSelect"
            @activate="(id: number) => paste(id, false)"
            @toggle="onToggleSelect"
            @favorite="(id: number) => store.toggleFavorite(id)"
            @pin="onToggleItemPin"
            @remove="onRemoveItem"
            @menu="onListMenu"
          />
        </div>

        <div v-else class="flex h-full flex-col gap-2 overflow-hidden pb-2">
          <ClipboardPinnedSection
            :items="store.pinnedItems.value"
            :selected-id="selectedId"
            :display-settings="store.settings.value.display"
            :highlight-keywords="store.searchKeywords.value"
            compact
            :show-delete-button="true"
            :show-favorite-button="true"
            :show-pin-button="true"
            @select="onListSelect"
            @activate="(id: number) => paste(id, false)"
            @favorite="(id: number) => store.toggleFavorite(id)"
            @pin="onToggleItemPin"
            @remove="onRemoveItem"
            @menu="onListMenu"
          />

          <div class="min-h-0 flex-1 overflow-hidden">
            <ClipboardList
              v-if="store.items.value.length"
              :key="listKey"
              :items="store.items.value"
              :selected-id="selectedId"
              :display-settings="store.settings.value.display"
              :highlight-keywords="store.searchKeywords.value"
              :compact="true"
              :draggable="store.filter.value === 'favorite'"
              :show-delete-button="true"
              :show-favorite-button="true"
              :show-pin-button="true"
              :index-offset="store.pinnedItems.value.length"
              @select="onListSelect"
              @activate="(id: number) => paste(id, false)"
              @favorite="(id: number) => store.toggleFavorite(id)"
              @pin="onToggleItemPin"
              @remove="onRemoveItem"
              @menu="onListMenu"
              @reorder="onReorder"
            />
          </div>
        </div>
      </div>

      <div class="shrink-0 px-3 pb-2 pt-1 select-none">
        <div class="flex items-center gap-1 rounded-xl bg-slate-100 p-1" data-no-drag>
          <div
            v-if="toolbarLayout.showFilter"
            class="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
          >
            <button
              v-for="filter in filters"
              :key="filter"
              type="button"
              class="shrink-0 rounded-lg px-2.5 py-1 text-xs font-medium transition-colors"
              :class="store.filter.value === filter
                ? 'bg-slate-900 text-white shadow-sm'
                : 'text-slate-600 hover:bg-white hover:text-slate-900'"
              @click="setFilter(filter)"
            >
              {{ t(`clipboard.filter.${filter}`) }}
            </button>
          </div>
          <div v-else class="flex-1" />

          <span class="mx-0.5 h-4 w-px shrink-0 bg-slate-200" aria-hidden />

          <ClipboardPanelGroupMenu
            :groups="store.groups.value"
            :selected-group-id="store.selectedGroupId.value"
            @select="setGroup"
            @create="createGroup"
            @rename="renameGroup"
            @delete="deleteGroup"
          />
        </div>
      </div>
    </div>
    </div>

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
