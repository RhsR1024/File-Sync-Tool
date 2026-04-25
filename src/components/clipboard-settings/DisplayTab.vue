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

const densityOptions = ['compact', 'standard', 'spacious'] as const;
const timeFormatOptions = ['relative', 'absolute'] as const;
const sourceAppOptions = ['none', 'name', 'icon', 'both'] as const;

function patch(next: DeepPartial<ClipboardSettings>) {
  emit('patch', next);
}

function patchDisplay(next: DeepPartial<ClipboardSettings['display']>) {
  patch({ display: next });
}

function updatePreviewLines(event: Event) {
  patchDisplay({
    preview_lines: Number((event.target as HTMLInputElement).value),
  });
}

function updateImageMaxHeight(event: Event) {
  patchDisplay({
    image_max_height: Number((event.target as HTMLInputElement).value),
  });
}
</script>

<template>
  <div class="space-y-4">
    <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.tabs.display') }}</h4>
      <div class="mt-4 grid gap-4 lg:grid-cols-2">
        <div class="space-y-4">
          <div class="space-y-2">
            <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.display.density') }}</div>
            <div class="flex flex-wrap gap-2">
              <button
                v-for="option in densityOptions"
                :key="option"
                type="button"
                class="rounded-full border px-3 py-1.5 text-xs font-medium transition-colors"
                :class="props.settings.display.density === option
                  ? 'border-slate-900 bg-slate-900 text-white'
                  : 'border-slate-200 bg-white text-slate-600 hover:bg-slate-100'"
                @click="patchDisplay({ density: option })"
              >
                {{ t(`clipboard.settings.options.density.${option}`) }}
              </button>
            </div>
          </div>

          <label class="block space-y-2">
            <div class="flex items-center justify-between gap-3">
              <span class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.display.previewLines') }}</span>
              <span class="text-xs text-slate-500">{{ props.settings.display.preview_lines }}</span>
            </div>
            <input
              type="range"
              min="1"
              max="10"
              :value="props.settings.display.preview_lines"
              class="w-full accent-slate-900"
              @change="updatePreviewLines"
            >
          </label>

          <div class="space-y-2">
            <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.display.timeFormat') }}</div>
            <div class="flex flex-wrap gap-2">
              <button
                v-for="option in timeFormatOptions"
                :key="option"
                type="button"
                class="rounded-full border px-3 py-1.5 text-xs font-medium transition-colors"
                :class="props.settings.display.time_format === option
                  ? 'border-slate-900 bg-slate-900 text-white'
                  : 'border-slate-200 bg-white text-slate-600 hover:bg-slate-100'"
                @click="patchDisplay({ time_format: option })"
              >
                {{ t(`clipboard.settings.options.timeFormat.${option}`) }}
              </button>
            </div>
          </div>

          <div class="space-y-2">
            <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.display.showSourceApp') }}</div>
            <div class="flex flex-wrap gap-2">
              <button
                v-for="option in sourceAppOptions"
                :key="option"
                type="button"
                class="rounded-full border px-3 py-1.5 text-xs font-medium transition-colors"
                :class="props.settings.display.show_source_app === option
                  ? 'border-slate-900 bg-slate-900 text-white'
                  : 'border-slate-200 bg-white text-slate-600 hover:bg-slate-100'"
                @click="patchDisplay({ show_source_app: option })"
              >
                {{ t(`clipboard.settings.options.sourceApp.${option}`) }}
              </button>
            </div>
          </div>
        </div>

        <div class="space-y-3 rounded-2xl border border-slate-100 bg-slate-50/70 p-4">
          <label class="flex items-center justify-between gap-4">
            <span class="text-sm text-slate-700">{{ t('clipboard.settings.display.showCharCount') }}</span>
            <input
              type="checkbox"
              :checked="props.settings.display.show_char_count"
              @change="patchDisplay({ show_char_count: ($event.target as HTMLInputElement).checked })"
            >
          </label>

          <label class="flex items-center justify-between gap-4">
            <span class="text-sm text-slate-700">{{ t('clipboard.settings.display.showByteSize') }}</span>
            <input
              type="checkbox"
              :checked="props.settings.display.show_byte_size"
              @change="patchDisplay({ show_byte_size: ($event.target as HTMLInputElement).checked })"
            >
          </label>

          <label class="flex items-center justify-between gap-4">
            <span class="text-sm text-slate-700">{{ t('clipboard.settings.display.imageAutoHeight') }}</span>
            <input
              type="checkbox"
              :checked="props.settings.display.image_auto_height"
              @change="patchDisplay({ image_auto_height: ($event.target as HTMLInputElement).checked })"
            >
          </label>

          <label class="flex items-center justify-between gap-4">
            <span class="text-sm text-slate-700">{{ t('clipboard.settings.display.dragIndicator') }}</span>
            <input
              type="checkbox"
              :checked="props.settings.display.drag_indicator"
              @change="patchDisplay({ drag_indicator: ($event.target as HTMLInputElement).checked })"
            >
          </label>

          <label class="block space-y-2">
            <div class="flex items-center justify-between gap-3">
              <span class="text-sm text-slate-700">{{ t('clipboard.settings.display.imageMaxHeight') }}</span>
              <span class="text-xs text-slate-500">{{ props.settings.display.image_max_height }}px</span>
            </div>
            <input
              type="number"
              min="48"
              max="480"
              class="w-full rounded-xl border border-slate-200 bg-white px-3 py-2 text-sm"
              :value="props.settings.display.image_max_height"
              @change="updateImageMaxHeight"
            >
          </label>
        </div>
      </div>
    </div>
  </div>
</template>
