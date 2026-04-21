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
const previewPositionOptions = ['auto', 'left', 'right'] as const;

function patch(next: DeepPartial<ClipboardSettings>) {
  emit('patch', next);
}
</script>

<template>
  <div class="space-y-4">
    <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.tabs.preview') }}</h4>
      <div class="mt-4 space-y-4">
        <label class="flex items-center justify-between gap-4">
          <span class="text-sm text-slate-700">{{ t('clipboard.settings.preview.imageEnabled') }}</span>
          <input
            type="checkbox"
            :checked="props.settings.preview.image_enabled"
            @change="patch({ preview: { image_enabled: ($event.target as HTMLInputElement).checked } })"
          >
        </label>

        <label class="flex items-center justify-between gap-4">
          <span class="text-sm text-slate-700">{{ t('clipboard.settings.enableTextPreviewLabel') }}</span>
          <input
            type="checkbox"
            :checked="props.settings.preview.text_enabled"
            @change="patch({ preview: { text_enabled: ($event.target as HTMLInputElement).checked } })"
          >
        </label>

        <label class="block space-y-2">
          <div class="flex items-center justify-between gap-3">
            <span class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.previewDelayLabel') }}</span>
            <span class="text-xs text-slate-500">{{ props.settings.preview.delay_ms }}ms</span>
          </div>
          <input
            type="range"
            min="0"
            max="1500"
            step="50"
            class="w-full accent-slate-900"
            :value="props.settings.preview.delay_ms"
            @change="patch({ preview: { delay_ms: Number(($event.target as HTMLInputElement).value) } })"
          >
        </label>

        <label class="block space-y-2">
          <div class="flex items-center justify-between gap-3">
            <span class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.preview.zoomStep') }}</span>
            <span class="text-xs text-slate-500">{{ props.settings.preview.zoom_step }}%</span>
          </div>
          <input
            type="range"
            min="5"
            max="50"
            step="5"
            class="w-full accent-slate-900"
            :value="props.settings.preview.zoom_step"
            @change="patch({ preview: { zoom_step: Number(($event.target as HTMLInputElement).value) } })"
          >
        </label>

        <div class="space-y-2">
          <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.preview.position') }}</div>
          <div class="flex flex-wrap gap-2">
            <button
              v-for="option in previewPositionOptions"
              :key="option"
              type="button"
              class="rounded-full border px-3 py-1.5 text-xs font-medium transition-colors"
              :class="props.settings.preview.position === option
                ? 'border-slate-900 bg-slate-900 text-white'
                : 'border-slate-200 bg-white text-slate-600 hover:bg-slate-100'"
              @click="patch({ preview: { position: option } })"
            >
              {{ t(`clipboard.settings.options.previewPosition.${option}`) }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
