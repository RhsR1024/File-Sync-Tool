import { computed, ref, watch } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useI18n } from 'vue-i18n';

import {
  pruneClipboardSelection,
  toggleClipboardSelection,
} from '@/composables/clipboardInteractionHelpers';
import { clipboardApi } from '@/lib/tauri';
import { parseSearch } from '@/lib/clipboardSearchParser';
import type { ClipboardFilter, ClipboardItem } from '@/lib/clipboardTypes';

export function useClipboardStore() {
  const { t } = useI18n();
  const items = ref<ClipboardItem[]>([]);
  const total = ref(0);
  const filter = ref<ClipboardFilter>('all');
  const search = ref('');
  const loading = ref(false);
  const error = ref<string | null>(null);
  const batchMode = ref(false);
  const selectedIds = ref<Set<number>>(new Set());
  const selectionAnchorId = ref<number | null>(null);

  const orderedSelectedIds = computed(() =>
    items.value.filter((item) => selectedIds.value.has(item.id)).map((item) => item.id),
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
      visibleIds: items.value.map((item) => item.id),
      selectedIds: selectedIds.value,
      anchorId: selectionAnchorId.value,
      targetId: id,
      shiftKey,
    });
    selectedIds.value = next.selectedIds;
    selectionAnchorId.value = next.anchorId;
  }

  function selectAllVisible() {
    const visibleIds = items.value.map((item) => item.id);
    selectedIds.value = new Set(visibleIds);
    selectionAnchorId.value = visibleIds.at(-1) ?? null;
  }

  async function reload() {
    loading.value = true;
    error.value = null;
    try {
      const parsed = parseSearch(search.value);
      const fromMs = parsed.filters.from
        ? new Date(parsed.filters.from + 'T00:00:00').getTime()
        : null;
      const toMs = parsed.filters.to
        ? new Date(parsed.filters.to + 'T23:59:59').getTime()
        : null;

      const result = await clipboardApi.list({
        filter: filter.value,
        search: parsed.keywords.join(' '),
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
      items.value = result.items;
      total.value = result.total;
    } catch (e) {
      console.error('[clipboard] reload failed:', e);
      error.value = `${t('clipboard.errors.loadFailed')} — ${e}`;
    } finally {
      loading.value = false;
    }
  }

  async function toggleFavorite(id: number) {
    try {
      await clipboardApi.toggleFavorite(id);
      await reload();
    } catch (e) {
      console.error('[clipboard] toggleFavorite failed:', e);
      error.value = `${t('clipboard.errors.saveFailed')} — ${e}`;
    }
  }

  async function remove(id: number) {
    try {
      await clipboardApi.delete(id);
      await reload();
    } catch (e) {
      console.error('[clipboard] remove failed:', e);
      error.value = `${t('clipboard.errors.saveFailed')} — ${e}`;
    }
  }

  async function startListening(): Promise<UnlistenFn> {
    return listen('clipboard-item-added', () => {
      void reload();
    });
  }

  watch(
    items,
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
    total,
    filter,
    search,
    loading,
    error,
    batchMode,
    selectedIds,
    orderedSelectedIds,
    reload,
    toggleFavorite,
    remove,
    startListening,
    clearSelection,
    setBatchMode,
    toggleBatchMode,
    toggleSelection,
    selectAllVisible,
  };
}
