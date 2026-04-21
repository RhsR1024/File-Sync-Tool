<script setup lang="ts">
import { ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import type { DeepPartial, ClipboardSettings } from '@/lib/clipboardTypes';

const props = defineProps<{
  settings: ClipboardSettings;
}>();

const emit = defineEmits<{
  patch: [patch: DeepPartial<ClipboardSettings>];
}>();

const { t } = useI18n();
const modeOptions = ['blacklist', 'whitelist'] as const;
const patternsText = ref(props.settings.app_filter.patterns.join('\n'));

watch(
  () => props.settings.app_filter.patterns,
  (patterns) => {
    patternsText.value = patterns.join('\n');
  },
);

function patch(next: DeepPartial<ClipboardSettings>) {
  emit('patch', next);
}

function commitPatterns() {
  patch({
    app_filter: {
      patterns: patternsText.value
        .split(/\r?\n/u)
        .map((pattern) => pattern.trim())
        .filter(Boolean),
    },
  });
}
</script>

<template>
  <div class="space-y-4">
    <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.tabs.appFilter') }}</h4>
      <div class="mt-4 space-y-4">
        <label class="flex items-center justify-between gap-4">
          <span class="text-sm text-slate-700">{{ t('clipboard.settings.appFilter.enabled') }}</span>
          <input
            type="checkbox"
            :checked="props.settings.app_filter.enabled"
            @change="patch({ app_filter: { enabled: ($event.target as HTMLInputElement).checked } })"
          >
        </label>

        <div class="space-y-2">
          <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.appFilter.mode') }}</div>
          <div class="flex flex-wrap gap-2">
            <button
              v-for="option in modeOptions"
              :key="option"
              type="button"
              class="rounded-full border px-3 py-1.5 text-xs font-medium transition-colors"
              :class="props.settings.app_filter.mode === option
                ? 'border-slate-900 bg-slate-900 text-white'
                : 'border-slate-200 bg-white text-slate-600 hover:bg-slate-100'"
              @click="patch({ app_filter: { mode: option } })"
            >
              {{ t(`clipboard.settings.options.appFilterMode.${option}`) }}
            </button>
          </div>
        </div>

        <label class="block space-y-2">
          <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.appFilter.patterns') }}</div>
          <textarea
            v-model="patternsText"
            rows="8"
            class="w-full rounded-2xl border border-slate-200 px-3 py-2 text-sm outline-none focus:border-slate-400"
            placeholder="Code.exe&#10;SnippingTool.exe"
            @change="commitPatterns"
          />
        </label>
      </div>
    </div>
  </div>
</template>
