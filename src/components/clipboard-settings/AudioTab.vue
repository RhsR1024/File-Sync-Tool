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
      <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.tabs.audio') }}</h4>
      <div class="mt-4 space-y-4">
        <label class="flex items-center justify-between gap-4">
          <span class="text-sm text-slate-700">{{ t('clipboard.settings.audio.enabled') }}</span>
          <input
            type="checkbox"
            :checked="props.settings.audio.enabled"
            @change="patch({ audio: { enabled: ($event.target as HTMLInputElement).checked } })"
          >
        </label>

        <label class="block space-y-2">
          <div class="flex items-center justify-between gap-3">
            <span class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.audio.volume') }}</span>
            <span class="text-xs text-slate-500">{{ props.settings.audio.volume }}%</span>
          </div>
          <input
            type="range"
            min="0"
            max="100"
            step="5"
            class="w-full accent-slate-900"
            :value="props.settings.audio.volume"
            @change="patch({ audio: { volume: Number(($event.target as HTMLInputElement).value) } })"
          >
        </label>

        <label class="flex items-center justify-between gap-4">
          <span class="text-sm text-slate-700">{{ t('clipboard.settings.audio.onCopy') }}</span>
          <input
            type="checkbox"
            :checked="props.settings.audio.on_copy"
            @change="patch({ audio: { on_copy: ($event.target as HTMLInputElement).checked } })"
          >
        </label>

        <label class="flex items-center justify-between gap-4">
          <span class="text-sm text-slate-700">{{ t('clipboard.settings.audio.onPaste') }}</span>
          <input
            type="checkbox"
            :checked="props.settings.audio.on_paste"
            @change="patch({ audio: { on_paste: ($event.target as HTMLInputElement).checked } })"
          >
        </label>
      </div>
    </div>
  </div>
</template>
