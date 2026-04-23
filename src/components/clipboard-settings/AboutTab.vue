<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';

import { normalizeClipboardToolbarItems } from '@/lib/clipboardSettingsUi';
import type { ClipboardSettings } from '@/lib/clipboardTypes';

const props = defineProps<{
  settings: ClipboardSettings;
}>();

const { t } = useI18n();

const toolbarSummary = computed(() =>
  normalizeClipboardToolbarItems(props.settings.toolbar.items)
    .map((item) => t(`clipboard.settings.toolbarItems.${item}`))
    .join(' / '),
);

const previewSummary = computed(() => [
  props.settings.preview.image_enabled
    ? t('clipboard.settings.about.imagePreviewOn')
    : t('clipboard.settings.about.imagePreviewOff'),
  props.settings.preview.text_enabled
    ? t('clipboard.settings.about.textPreviewOn')
    : t('clipboard.settings.about.textPreviewOff'),
  `${props.settings.preview.delay_ms}ms`,
].join(' / '));

const appFilterSummary = computed(() => {
  if (!props.settings.app_filter.enabled) {
    return t('clipboard.settings.about.appFilterDisabled');
  }
  return t('clipboard.settings.about.appFilterSummary', {
    mode: t(`clipboard.settings.options.appFilterMode.${props.settings.app_filter.mode}`),
    count: props.settings.app_filter.patterns.length,
  });
});

const cleanupSummary = computed(() => {
  const maxItems = props.settings.data.max_items === 0
    ? t('clipboard.settings.about.unlimited')
    : props.settings.data.max_items.toLocaleString();
  const retainDays = props.settings.data.retain_days === 0
    ? t('clipboard.settings.about.unlimited')
    : t('clipboard.settings.about.days', { n: props.settings.data.retain_days });

  return t('clipboard.settings.about.cleanupSummary', { maxItems, retainDays });
});
</script>

<template>
  <div class="space-y-4">
    <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.tabs.about') }}</h4>
      <div class="mt-4 grid gap-3 md:grid-cols-2 xl:grid-cols-3">
        <div class="rounded-xl border border-slate-100 bg-slate-50 px-3 py-3">
          <div class="text-xs text-slate-500">{{ t('clipboard.settings.hotkeyLabel') }}</div>
          <div class="mt-1 text-sm font-medium text-slate-800">{{ props.settings.hotkey }}</div>
        </div>
        <div class="rounded-xl border border-slate-100 bg-slate-50 px-3 py-3">
          <div class="text-xs text-slate-500">{{ t('clipboard.settings.tabs.preview') }}</div>
          <div class="mt-1 text-sm font-medium text-slate-800">{{ previewSummary }}</div>
        </div>
        <div class="rounded-xl border border-slate-100 bg-slate-50 px-3 py-3">
          <div class="text-xs text-slate-500">{{ t('clipboard.settings.display.toolbarOrder') }}</div>
          <div class="mt-1 text-sm font-medium text-slate-800">{{ toolbarSummary }}</div>
        </div>
        <div class="rounded-xl border border-slate-100 bg-slate-50 px-3 py-3">
          <div class="text-xs text-slate-500">{{ t('clipboard.settings.tabs.appFilter') }}</div>
          <div class="mt-1 text-sm font-medium text-slate-800">{{ appFilterSummary }}</div>
        </div>
        <div class="rounded-xl border border-slate-100 bg-slate-50 px-3 py-3">
          <div class="text-xs text-slate-500">{{ t('clipboard.settings.about.cleanupTitle') }}</div>
          <div class="mt-1 text-sm font-medium text-slate-800">{{ cleanupSummary }}</div>
          <p class="mt-1 text-xs leading-5 text-slate-500">
            {{ t('clipboard.settings.about.protectedRecords') }}
          </p>
        </div>
      </div>
    </div>
  </div>
</template>
