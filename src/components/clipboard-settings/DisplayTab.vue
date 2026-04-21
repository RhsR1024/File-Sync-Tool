<script setup lang="ts">
import { ChevronLeft, ChevronRight } from 'lucide-vue-next';
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';

import {
  CLIPBOARD_TOOLBAR_ITEM_IDS,
  moveClipboardToolbarItem,
  normalizeClipboardToolbarItems,
  type ClipboardToolbarItemId,
} from '@/lib/clipboardSettingsUi';
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

const toolbarItems = computed(() =>
  normalizeClipboardToolbarItems(props.settings.toolbar.items),
);

function patch(next: DeepPartial<ClipboardSettings>) {
  emit('patch', next);
}

function patchDisplay(next: DeepPartial<ClipboardSettings['display']>) {
  patch({ display: next });
}

function patchToolbar(next: DeepPartial<ClipboardSettings['toolbar']>) {
  patch({ toolbar: next });
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

function toggleToolbarItem(item: ClipboardToolbarItemId) {
  const active = toolbarItems.value.includes(item);
  if (active) {
    patchToolbar({
      items: toolbarItems.value.filter((entry) => entry !== item),
    });
    return;
  }

  patchToolbar({
    items: normalizeClipboardToolbarItems([...toolbarItems.value, item]),
  });
}

function moveItem(item: ClipboardToolbarItemId, direction: -1 | 1) {
  patchToolbar({
    items: moveClipboardToolbarItem(toolbarItems.value, item, direction),
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

    <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <div class="flex items-center justify-between gap-4">
        <div>
          <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.display.toolbarOrder') }}</h4>
          <p class="mt-1 text-xs text-slate-500">{{ t('clipboard.settings.display.toolbarVisible') }}</p>
        </div>
        <input
          type="checkbox"
          :checked="props.settings.toolbar.visible"
          @change="patchToolbar({ visible: ($event.target as HTMLInputElement).checked })"
        >
      </div>

      <div class="mt-4 space-y-2">
        <div
          v-for="item in CLIPBOARD_TOOLBAR_ITEM_IDS"
          :key="item"
          class="flex items-center gap-3 rounded-xl border border-slate-100 px-3 py-2"
          :class="toolbarItems.includes(item) ? 'bg-slate-50' : 'bg-white'"
        >
          <button
            type="button"
            class="inline-flex h-6 w-6 items-center justify-center rounded border border-slate-200 text-[11px] text-slate-600"
            :disabled="!toolbarItems.includes(item)"
            @click="moveItem(item, -1)"
          >
            <ChevronLeft class="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            class="inline-flex h-6 w-6 items-center justify-center rounded border border-slate-200 text-[11px] text-slate-600"
            :disabled="!toolbarItems.includes(item)"
            @click="moveItem(item, 1)"
          >
            <ChevronRight class="h-3.5 w-3.5" />
          </button>

          <div class="min-w-0 flex-1">
            <div class="text-sm font-medium text-slate-700">
              {{ t(`clipboard.settings.toolbarItems.${item}`) }}
            </div>
          </div>

          <input
            type="checkbox"
            :checked="toolbarItems.includes(item)"
            @change="toggleToolbarItem(item)"
          >
        </div>
      </div>
    </div>
  </div>
</template>
