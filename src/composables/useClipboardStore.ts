import { ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useI18n } from 'vue-i18n';

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

  return {
    items,
    total,
    filter,
    search,
    loading,
    error,
    reload,
    toggleFavorite,
    remove,
    startListening,
  };
}
