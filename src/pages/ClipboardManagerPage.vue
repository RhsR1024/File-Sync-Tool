<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { useI18n } from 'vue-i18n';
import { Pin, Star } from 'lucide-vue-next';

import { useClipboardContextMenu } from '@/composables/useClipboardContextMenu';
import { useClipboardStore } from '@/composables/useClipboardStore';
import type { ClipboardContextActionId } from '@/composables/clipboardContextMenuHelpers';
import ClipboardCardMenu from '@/components/clipboard/ClipboardCardMenu.vue';
import ClipboardFileDetailsDialog from '@/components/clipboard/ClipboardFileDetailsDialog.vue';
import ClipboardGroupSidebar from '@/components/clipboard/ClipboardGroupSidebar.vue';
import ClipboardList from '@/components/clipboard/ClipboardList.vue';
import ClipboardMergePasteDialog from '@/components/clipboard/ClipboardMergePasteDialog.vue';
import ClipboardPinnedSection from '@/components/clipboard/ClipboardPinnedSection.vue';
import ClipboardSearchBox from '@/components/clipboard/ClipboardSearchBox.vue';
import ClipboardStats from '@/components/clipboard/ClipboardStats.vue';
import ClipboardSettingsPanel from '@/components/clipboard/ClipboardSettingsPanel.vue';
import ClipboardToolbar from '@/components/clipboard/ClipboardToolbar.vue';
import { buildClipboardToolbarLayout } from '@/lib/clipboardSettingsUi';
import { clipboardApi } from '@/lib/tauri';
import type { ClipboardFilter, ClipboardGroup, ClipboardItem } from '@/lib/clipboardTypes';

defineOptions({ name: 'ClipboardManagerPage' });

const { t } = useI18n();
const store = useClipboardStore();
const selectedId = ref<number | null>(null);
const settingsOpen = ref(false);
const reloadCounter = ref(0);
const copyToast = ref<string | null>(null);
let copyToastTimer: number | null = null;

const filters: ClipboardFilter[] = ['all', 'text', 'image', 'file', 'favorite'];
const selectionCount = computed(() => store.selectedIds.value.size);
const hasVisibleItems = computed(() => store.visibleItems.value.length > 0);
const toolbarLayout = computed(() =>
  buildClipboardToolbarLayout(store.settings.value.toolbar, ['batch', 'settings']),
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
  } catch (error) {
    console.error('[clipboard] copy failed:', error);
    store.error.value = `${t('clipboard.errors.saveFailed')} - ${error}`;
  }
}

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

watch(
  () => store.visibleItems.value,
  (items) => {
    if (selectedId.value !== null && !items.some((item) => item.id === selectedId.value)) {
      selectedId.value = items[0]?.id ?? null;
    }
  },
  { deep: false },
);

function setFilter(filter: ClipboardFilter) {
  store.filter.value = filter;
  selectedId.value = null;
  store.clearSelection();
  void store.reload();
}

function setGroup(groupId: number | null) {
  store.selectGroup(groupId);
  selectedId.value = null;
  store.clearSelection();
  void store.reload();
}

function onSearchChange(value: string) {
  store.search.value = value;
  void store.reload();
}

function toggleSettingsPanel() {
  settingsOpen.value = !settingsOpen.value;
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

async function createGroup(name: string) {
  await store.createGroup(name);
}

async function renameGroup(payload: { id: number; name: string }) {
  await store.renameGroup(payload.id, payload.name);
}

async function deleteGroup(group: ClipboardGroup) {
  if (!window.confirm(t('clipboard.groups.deleteConfirm', { name: group.name }))) return;
  await store.deleteGroup(group.id);
}

async function batchDelete() {
  const ids = store.orderedSelectedIds.value;
  if (ids.length === 0) return;
  const message = t('clipboard.actions.batchDeleteConfirm', { n: ids.length });
  if (!window.confirm(message)) return;
  try {
    await clipboardApi.deleteBatch(ids);
    clearSelection();
    await store.reload();
    reloadCounter.value++;
  } catch (error) {
    console.error('[clipboard] batchDelete failed:', error);
    store.error.value = `${t('clipboard.errors.saveFailed')} - ${error}`;
  }
}

async function batchFavorite(nextFavorite: boolean) {
  const ids = store.orderedSelectedIds.value;
  for (const id of ids) {
    try {
      const item = await clipboardApi.get(id);
      if (nextFavorite && !item.is_favorite) await clipboardApi.toggleFavorite(id);
      if (!nextFavorite && item.is_favorite) await clipboardApi.toggleFavorite(id);
    } catch (error) {
      console.error('[clipboard] batchFavorite failed:', error);
      store.error.value = `${t('clipboard.errors.saveFailed')} - ${error}`;
      return;
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
  } catch (error) {
    console.error('[clipboard] reorder failed:', error);
    store.error.value = `${t('clipboard.errors.saveFailed')} - ${error}`;
  }
}

async function pasteFromContextMenu(id: number, plain: boolean) {
  try {
    if (plain) await clipboardApi.pastePlain(id);
    else await clipboardApi.paste(id);
  } catch (error) {
    setClipboardActionError(error, plain ? 'pastePlain' : 'paste');
  }
}

async function onToggleItemPin(id: number) {
  await store.togglePin(id);
}

async function onRemoveItem(id: number) {
  await store.remove(id);
  reloadCounter.value++;
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
  onPaste: pasteFromContextMenu,
  onCopy: copyToClipboard,
  onDelete: async (id: number) => {
    await onRemoveItem(id);
  },
  onToggleFavorite: async (id: number) => {
    await store.toggleFavorite(id);
    reloadCounter.value++;
  },
  onTogglePin: async (id: number) => {
    await onToggleItemPin(id);
  },
  onMoveToGroup: async (id: number, groupId: number | null) => {
    await store.moveToGroup(id, groupId);
  },
  onError: setClipboardActionError,
  onMergeSuccess: async () => {
    resetBatchSelection();
    await store.reload();
    reloadCounter.value++;
  },
});

function onListMenu(payload: { item: ClipboardItem; x: number; y: number }) {
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
    <div class="mx-auto flex h-full w-full max-w-7xl flex-col gap-4 px-6 py-6 pb-10">
      <header class="space-y-2">
        <h1 class="text-2xl font-bold tracking-tight text-slate-950">
          {{ t('clipboard.tool.title') }}
        </h1>
        <p class="text-sm text-slate-500">{{ t('clipboard.tool.description') }}</p>
      </header>

      <details
        class="rounded-2xl border border-slate-200 bg-white"
        :open="settingsOpen"
        @toggle="settingsOpen = ($event.target as HTMLDetailsElement).open"
      >
        <summary class="cursor-pointer px-5 py-3 text-sm font-medium text-slate-700">
          {{ t('clipboard.settings.title') }}
        </summary>
        <div class="border-t border-slate-100">
          <ClipboardSettingsPanel />
        </div>
      </details>

      <ClipboardStats :reload-signal="reloadCounter" />

      <div class="rounded-2xl border border-slate-200 bg-white p-3 shadow-sm">
        <div class="flex flex-wrap items-start gap-3">
          <div class="min-w-0 flex-1 space-y-3">
            <ClipboardSearchBox
              v-if="toolbarLayout.showSearch"
              :model-value="store.search.value"
              :placeholder="t('clipboard.search.placeholder')"
              @update:model-value="onSearchChange"
              @clear="onSearchChange('')"
            />

            <div v-if="toolbarLayout.showFilter" class="flex flex-wrap gap-2">
              <button
                v-for="filter in filters"
                :key="filter"
                type="button"
                class="rounded-full px-3 py-1 text-xs font-medium transition-colors"
                :class="store.filter.value === filter
                  ? 'bg-slate-900 text-white shadow-sm'
                  : 'bg-slate-100 text-slate-600 hover:bg-slate-200'"
                @click="setFilter(filter)"
              >
                {{ t(`clipboard.filter.${filter}`) }}
              </button>
            </div>
          </div>

          <div class="flex items-center gap-3">
            <span class="text-xs text-slate-400">
              {{ store.total.value }} {{ t('clipboard.totalSuffix') }}
            </span>
            <ClipboardToolbar
              :items="toolbarLayout.actionItems"
              :batch-mode="store.batchMode.value"
              @batch="toggleBatchMode"
              @settings="toggleSettingsPanel"
            />
          </div>
        </div>
      </div>

      <div
        v-if="store.batchMode.value"
        class="flex flex-wrap items-center gap-2 rounded-xl border border-slate-200 bg-white px-3 py-2 text-xs"
      >
        <span class="text-slate-500">{{ selectionCount }} / {{ store.visibleItems.value.length }}</span>
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

      <div class="grid min-h-[520px] flex-1 gap-4 lg:grid-cols-[260px_minmax(0,1fr)]">
        <ClipboardGroupSidebar
          :groups="store.groups.value"
          :selected-group-id="store.selectedGroupId.value"
          @select="setGroup"
          @create="createGroup"
          @rename="renameGroup"
          @delete="deleteGroup"
        />

        <section class="min-h-[520px] rounded-2xl border border-slate-200 bg-white shadow-sm">
          <div v-if="store.error.value" class="p-5 text-sm text-rose-500">
            {{ store.error.value }}
          </div>

          <div v-else-if="store.loading.value" class="p-6 text-sm text-slate-400">
            {{ t('clipboard.loading') }}
          </div>

          <div v-else-if="!hasVisibleItems" class="p-8 text-center text-sm text-slate-400">
            {{ store.search.value ? t('clipboard.panel.noMatch') : t('clipboard.panel.empty') }}
          </div>

          <div v-else-if="store.batchMode.value" class="max-h-[60vh] overflow-y-auto">
            <label
              v-for="item in store.visibleItems.value"
              :key="item.id"
              class="flex cursor-pointer items-center gap-3 border-b border-slate-100 px-4 py-2 hover:bg-slate-50"
              @click.prevent="toggleSelect({ id: item.id, shiftKey: $event.shiftKey })"
            >
              <input
                type="checkbox"
                class="h-4 w-4 shrink-0"
                :checked="store.selectedIds.value.has(item.id)"
              />
              <span class="inline-flex shrink-0 rounded bg-slate-200/60 px-1.5 py-0.5 text-[10px] uppercase text-slate-600">
                {{ item.kind }}
              </span>
              <Star
                v-if="item.is_favorite"
                class="h-3.5 w-3.5 shrink-0 text-amber-500"
                fill="currentColor"
              />
              <Pin
                v-if="item.is_pinned"
                class="h-3.5 w-3.5 shrink-0 text-amber-700"
              />
              <span class="flex-1 truncate text-sm text-slate-700">{{ item.content_preview }}</span>
            </label>
          </div>

          <div v-else class="flex h-full min-h-[520px] flex-col gap-3 p-3">
            <ClipboardPinnedSection
              :items="store.pinnedItems.value"
              :selected-id="selectedId"
              :display-settings="store.settings.value.display"
              :highlight-keywords="store.searchKeywords.value"
              :show-delete-button="true"
              :show-favorite-button="true"
              :show-pin-button="true"
              @select="(id: number) => (selectedId = id)"
              @activate="(id: number) => copyToClipboard(id)"
              @favorite="(id: number) => store.toggleFavorite(id)"
              @pin="onToggleItemPin"
              @remove="onRemoveItem"
              @menu="onListMenu"
            />

            <div class="min-h-0 flex-1 overflow-hidden">
              <ClipboardList
                v-if="store.items.value.length"
                :items="store.items.value"
                :selected-id="selectedId"
                :display-settings="store.settings.value.display"
                :highlight-keywords="store.searchKeywords.value"
                :draggable="store.filter.value === 'favorite'"
                :show-favorite-button="true"
                :show-pin-button="true"
                :show-delete-button="true"
                :index-offset="store.pinnedItems.value.length"
                @select="(id: number) => (selectedId = id)"
                @activate="(id: number) => copyToClipboard(id)"
                @favorite="(id: number) => store.toggleFavorite(id)"
                @pin="onToggleItemPin"
                @remove="onRemoveItem"
                @menu="onListMenu"
                @reorder="onReorder"
              />
            </div>
          </div>
        </section>
      </div>
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
