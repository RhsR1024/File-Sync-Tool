<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import type { DeepPartial, ClipboardSettings } from '@/lib/clipboardTypes';

const props = defineProps<{
  settings: ClipboardSettings;
}>();

const emit = defineEmits<{
  patch: [patch: DeepPartial<ClipboardSettings>];
}>();

const { t } = useI18n();

function patch(next: DeepPartial<ClipboardSettings>) {
  emit('patch', next);
}
</script>

<template>
  <div class="space-y-4">
    <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.tabs.data') }}</h4>
      <div class="mt-4 grid gap-4 md:grid-cols-3">
        <label class="space-y-2">
          <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.maxItemsLabel') }}</div>
          <input
            type="number"
            min="0"
            class="w-full rounded-xl border border-slate-200 px-3 py-2 text-sm"
            :value="props.settings.data.max_items"
            @change="patch({ data: { max_items: Number(($event.target as HTMLInputElement).value) } })"
          >
        </label>

        <label class="space-y-2">
          <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.retainDaysLabel') }}</div>
          <input
            type="number"
            min="0"
            class="w-full rounded-xl border border-slate-200 px-3 py-2 text-sm"
            :value="props.settings.data.retain_days"
            @change="patch({ data: { retain_days: Number(($event.target as HTMLInputElement).value) } })"
          >
        </label>

        <label class="space-y-2">
          <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.data.maxItemBytes') }}</div>
          <input
            type="number"
            min="0"
            step="1024"
            class="w-full rounded-xl border border-slate-200 px-3 py-2 text-sm"
            :value="props.settings.data.max_item_bytes"
            @change="patch({ data: { max_item_bytes: Number(($event.target as HTMLInputElement).value) } })"
          >
        </label>
      </div>
    </div>
  </div>
</template>
