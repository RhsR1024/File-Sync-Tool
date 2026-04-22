<script setup lang="ts">
import { onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import { clipboardApi } from '@/lib/tauri';
import type { ClipboardStats as Stats } from '@/lib/clipboardTypes';

const props = defineProps<{
  reloadSignal?: number;
}>();

const { t } = useI18n();
const stats = ref<Stats | null>(null);
const error = ref<string | null>(null);

async function reload() {
  try {
    stats.value = await clipboardApi.stats();
    error.value = null;
  } catch (e) {
    console.error('[clipboard] stats reload failed:', e);
    error.value = `${t('clipboard.errors.loadFailed')} — ${e}`;
  }
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

onMounted(reload);

watch(() => props.reloadSignal, () => {
  void reload();
});

defineExpose({ reload });
</script>

<template>
  <div v-if="error" class="rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs text-rose-600">
    {{ error }}
  </div>
  <div v-else-if="stats" class="grid grid-cols-3 gap-3">
    <div class="rounded-xl border border-slate-200 bg-white p-4">
      <div class="text-xs text-slate-500">{{ t('clipboard.stats.totalItems') }}</div>
      <div class="mt-1 text-xl font-bold text-slate-800">{{ stats.total }}</div>
    </div>
    <div class="rounded-xl border border-slate-200 bg-white p-4">
      <div class="text-xs text-slate-500">{{ t('clipboard.stats.dbSize') }}</div>
      <div class="mt-1 text-xl font-bold text-slate-800">{{ formatBytes(stats.db_bytes) }}</div>
    </div>
    <div class="rounded-xl border border-slate-200 bg-white p-4">
      <div class="text-xs text-slate-500">{{ t('clipboard.stats.imageCount') }}</div>
      <div class="mt-1 text-xl font-bold text-slate-800">
        {{ stats.image_count }} · {{ formatBytes(stats.images_bytes) }}
      </div>
    </div>
  </div>
</template>
