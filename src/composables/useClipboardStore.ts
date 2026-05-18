import { computed, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useI18n } from 'vue-i18n';

import {
  pruneClipboardSelection,
  toggleClipboardSelection,
} from '@/composables/clipboardInteractionHelpers';
import { partitionClipboardItemsForDisplay, resolveActiveClipboardGroupId } from '@/lib/clipboardGroupsView';
import { extractClipboardSearchKeywords } from '@/lib/clipboardListPresentation';
import { clipboardApi } from '@/lib/tauri';
import { parseSearch } from '@/lib/clipboardSearchParser';
import {
  createDefaultClipboardSettings,
  normalizeClipboardSettings,
  type ClipboardFilter,
  type ClipboardGroup,
  type ClipboardItem,
  type ClipboardSettings,
} from '@/lib/clipboardTypes';

export function useClipboardStore() {
  const { t } = useI18n();
  const items = ref<ClipboardItem[]>([]);
  const pinnedItems = ref<ClipboardItem[]>([]);
  const groups = ref<ClipboardGroup[]>([]);
  const selectedGroupId = ref<number | null>(null);
  const total = ref(0);
  const filter = ref<ClipboardFilter>('all');
  const search = ref('');
  const loading = ref(false);
  const error = ref<string | null>(null);
  const batchMode = ref(false);
  const selectedIds = ref<Set<number>>(new Set());
  const selectionAnchorId = ref<number | null>(null);
  const settings = ref<ClipboardSettings>(createDefaultClipboardSettings());
  const parsedSearch = computed(() => parseSearch(search.value));
  const searchKeywords = computed(() => extractClipboardSearchKeywords(search.value));
  const visibleItems = computed(() => [...pinnedItems.value, ...items.value]);

  const orderedSelectedIds = computed(() =>
    visibleItems.value
      .filter((item) => selectedIds.value.has(item.id))
      .map((item) => item.id),
  );

  function clearSelection() {
    selectedIds.value = new Set();
    selectionAnchorId.value = null;
  }

  function setBatchMode(next: boolean) {
    batchMode.value = next;
    if (!next) clearSelection();
  }

  function toggleBatchMode() {
    setBatchMode(!batchMode.value);
  }

  function toggleSelection(id: number, shiftKey = false) {
    const next = toggleClipboardSelection({
      visibleIds: visibleItems.value.map((item) => item.id),
      selectedIds: selectedIds.value,
      anchorId: selectionAnchorId.value,
      targetId: id,
      shiftKey,
    });
    selectedIds.value = next.selectedIds;
    selectionAnchorId.value = next.anchorId;
  }

  function selectAllVisible() {
    const visibleIds = visibleItems.value.map((item) => item.id);
    selectedIds.value = new Set(visibleIds);
    selectionAnchorId.value = visibleIds.at(-1) ?? null;
  }

  async function selectGroup(groupId: number | null) {
    selectedGroupId.value = groupId;
    try {
      await clipboardApi.setActiveGroup(groupId);
    } catch (e) {
      console.error('[clipboard] setActiveGroup failed:', e);
      error.value = `${t('clipboard.errors.saveFailed')} - ${e}`;
    }
  }

  async function reload() {
    loading.value = true;
    error.value = null;
    try {
      const parsed = parsedSearch.value;
      const fromMs = parsed.filters.from
        ? new Date(parsed.filters.from + 'T00:00:00').getTime()
        : null;
      const toMs = parsed.filters.to
        ? new Date(parsed.filters.to + 'T23:59:59').getTime()
        : null;

      const result = await clipboardApi.list({
        filter: filter.value,
        search: parsed.keywords.join(' '),
        search_payload: parsed,
        group_id: selectedGroupId.value,
        pinned_only: false,
        op_type: parsed.filters.type ?? null,
        op_from_ms: fromMs,
        op_to_ms: toMs,
        op_app: parsed.filters.app ?? null,
        op_fav_only: parsed.filters.fav === true,
        op_size_gt: parsed.filters.sizeGt ?? null,
        op_size_lt: parsed.filters.sizeLt ?? null,
        offset: 0,
        limit: 200,
      });
      const next = partitionClipboardItemsForDisplay(result.items);
      pinnedItems.value = next.pinnedItems;
      items.value = next.regularItems;
      total.value = result.total;
    } catch (e) {
      console.error('[clipboard] reload failed:', e);
      error.value = `${t('clipboard.errors.loadFailed')} - ${e}`;
    } finally {
      loading.value = false;
    }
  }

  async function reloadGroups() {
    try {
      const next = await clipboardApi.listGroups();
      groups.value = next;
      await selectGroup(resolveActiveClipboardGroupId(next, selectedGroupId.value));
    } catch (e) {
      console.error('[clipboard] reload groups failed:', e);
      error.value = `${t('clipboard.errors.loadFailed')} - ${e}`;
    }
  }

  async function reloadSettings() {
    try {
      const next = await clipboardApi.getSettings();
      settings.value = normalizeClipboardSettings(next);
    } catch (e) {
      console.error('[clipboard] reload settings failed:', e);
    }
  }

  async function toggleFavorite(id: number) {
    try {
      await clipboardApi.toggleFavorite(id);
      await reload();
    } catch (e) {
      console.error('[clipboard] toggleFavorite failed:', e);
      error.value = `${t('clipboard.errors.saveFailed')} - ${e}`;
    }
  }

  async function togglePin(id: number) {
    try {
      await clipboardApi.togglePin(id);
      await reload();
    } catch (e) {
      console.error('[clipboard] togglePin failed:', e);
      error.value = `${t('clipboard.errors.saveFailed')} - ${e}`;
    }
  }

  async function moveToGroup(id: number, groupId: number | null) {
    try {
      await clipboardApi.moveToGroup(id, groupId);
      await reload();
    } catch (e) {
      console.error('[clipboard] moveToGroup failed:', e);
      error.value = `${t('clipboard.errors.saveFailed')} - ${e}`;
    }
  }

  async function createGroup(name: string) {
    try {
      const group = await clipboardApi.createGroup(name);
      await reloadGroups();
      await selectGroup(group.id);
      await reload();
    } catch (e) {
      console.error('[clipboard] createGroup failed:', e);
      error.value = `${t('clipboard.errors.saveFailed')} - ${e}`;
    }
  }

  async function renameGroup(id: number, name: string) {
    try {
      await clipboardApi.renameGroup(id, name);
      await reloadGroups();
    } catch (e) {
      console.error('[clipboard] renameGroup failed:', e);
      error.value = `${t('clipboard.errors.saveFailed')} - ${e}`;
    }
  }

  async function deleteGroup(id: number) {
    try {
      await clipboardApi.deleteGroup(id);
      if (selectedGroupId.value === id) {
        await selectGroup(null);
      }
      await reloadGroups();
      await reload();
    } catch (e) {
      console.error('[clipboard] deleteGroup failed:', e);
      error.value = `${t('clipboard.errors.saveFailed')} - ${e}`;
    }
  }

  async function remove(id: number) {
    try {
      await clipboardApi.delete(id);
      await reload();
    } catch (e) {
      console.error('[clipboard] remove failed:', e);
      error.value = `${t('clipboard.errors.saveFailed')} - ${e}`;
    }
  }

  async function startListening(): Promise<UnlistenFn> {
    const unlistenItemAdded = await listen('clipboard-item-added', () => {
      void reload();
    });
    const unlistenGroups = await listen<ClipboardGroup[]>(
      'clipboard-groups-changed',
      (event) => {
        groups.value = event.payload;
        void selectGroup(resolveActiveClipboardGroupId(
          event.payload,
          selectedGroupId.value,
        ));
        void reload();
      },
    );
    const unlistenSettings = await listen<ClipboardSettings>(
      'clipboard-settings-updated',
      (event) => {
        settings.value = normalizeClipboardSettings(event.payload);
      },
    );
    await Promise.all([
      reloadGroups(),
      reloadSettings(),
    ]);
    return () => {
      unlistenItemAdded();
      unlistenGroups();
      unlistenSettings();
    };
  }

  watch(
    visibleItems,
    (list) => {
      const next = pruneClipboardSelection(list.map((item) => item.id), {
        selectedIds: selectedIds.value,
        anchorId: selectionAnchorId.value,
      });
      selectedIds.value = next.selectedIds;
      selectionAnchorId.value = next.anchorId;
    },
    { deep: false },
  );

  return {
    items,
    pinnedItems,
    groups,
    selectedGroupId,
    visibleItems,
    total,
    filter,
    search,
    loading,
    error,
    batchMode,
    selectedIds,
    orderedSelectedIds,
    settings,
    searchKeywords,
    reload,
    reloadGroups,
    reloadSettings,
    selectGroup,
    toggleFavorite,
    togglePin,
    moveToGroup,
    createGroup,
    renameGroup,
    deleteGroup,
    remove,
    startListening,
    clearSelection,
    setBatchMode,
    toggleBatchMode,
    toggleSelection,
    selectAllVisible,
  };
}
