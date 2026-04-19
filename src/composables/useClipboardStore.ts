import { ref } from 'vue';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import { clipboardApi } from '@/lib/tauri';
import type { ClipboardFilter, ClipboardItem } from '@/lib/clipboardTypes';

export function useClipboardStore() {
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
      const result = await clipboardApi.list({
        filter: filter.value,
        search: search.value,
        offset: 0,
        limit: 200,
      });
      items.value = result.items;
      total.value = result.total;
    } catch (e) {
      error.value = String(e);
    } finally {
      loading.value = false;
    }
  }

  async function toggleFavorite(id: number) {
    try {
      await clipboardApi.toggleFavorite(id);
      await reload();
    } catch (e) {
      error.value = String(e);
    }
  }

  async function remove(id: number) {
    try {
      await clipboardApi.delete(id);
      await reload();
    } catch (e) {
      error.value = String(e);
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
