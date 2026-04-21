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
  props.settings.preview.image_enabled ? 'image on' : 'image off',
  props.settings.preview.text_enabled ? 'text on' : 'text off',
  `${props.settings.preview.delay_ms}ms`,
].join(' / '));

const audioSummary = computed(() => [
  props.settings.audio.enabled ? 'enabled' : 'disabled',
  `${props.settings.audio.volume}%`,
].join(' / '));
</script>

<template>
  <div class="space-y-4">
    <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.tabs.about') }}</h4>
      <div class="mt-4 grid gap-3 md:grid-cols-2">
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
          <div class="text-xs text-slate-500">{{ t('clipboard.settings.tabs.audio') }}</div>
          <div class="mt-1 text-sm font-medium text-slate-800">{{ audioSummary }}</div>
        </div>
      </div>
    </div>
  </div>
</template>
