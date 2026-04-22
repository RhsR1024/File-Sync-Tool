<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';

import type { DeepPartial, ClipboardSettings } from '@/lib/clipboardTypes';

const props = defineProps<{
  settings: ClipboardSettings;
}>();

const emit = defineEmits<{
  patch: [patch: DeepPartial<ClipboardSettings>];
}>();

const { t } = useI18n();
const volumePresets = [25, 50, 100] as const;
const controlsDisabled = computed(() => !props.settings.audio.enabled);

function patch(next: DeepPartial<ClipboardSettings>) {
  emit('patch', next);
}

function applyVolumePreset(volume: number) {
  patch({ audio: { volume } });
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

        <p class="rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-xs leading-5 text-slate-500">
          {{ t('clipboard.settings.audio.disabledHint') }}
        </p>

        <div :class="controlsDisabled ? 'pointer-events-none opacity-50' : ''" class="space-y-4 transition-opacity">
          <label class="block space-y-2">
            <div class="flex items-center justify-between gap-3">
              <span class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.audio.volume') }}</span>
              <span class="text-xs text-slate-500">{{ props.settings.audio.volume }}%</span>
            </div>
            <div class="flex flex-wrap gap-2">
              <button
                v-for="preset in volumePresets"
                :key="preset"
                type="button"
                class="rounded-full border px-3 py-1 text-[11px] font-medium transition-colors"
                :class="props.settings.audio.volume === preset
                  ? 'border-slate-900 bg-slate-900 text-white'
                  : 'border-slate-200 bg-white text-slate-600 hover:bg-slate-50'"
                :disabled="controlsDisabled"
                @click="applyVolumePreset(preset)"
              >
                {{ preset }}%
              </button>
            </div>
            <input
              type="range"
              min="0"
              max="100"
              step="5"
              class="w-full accent-slate-900"
              :disabled="controlsDisabled"
              :value="props.settings.audio.volume"
              @change="patch({ audio: { volume: Number(($event.target as HTMLInputElement).value) } })"
            >
          </label>

          <label class="flex items-center justify-between gap-4">
            <span class="text-sm text-slate-700">{{ t('clipboard.settings.audio.onCopy') }}</span>
            <input
              type="checkbox"
              :disabled="controlsDisabled"
              :checked="props.settings.audio.on_copy"
              @change="patch({ audio: { on_copy: ($event.target as HTMLInputElement).checked } })"
            >
          </label>

          <label class="flex items-center justify-between gap-4">
            <span class="text-sm text-slate-700">{{ t('clipboard.settings.audio.onPaste') }}</span>
            <input
              type="checkbox"
              :disabled="controlsDisabled"
              :checked="props.settings.audio.on_paste"
              @change="patch({ audio: { on_paste: ($event.target as HTMLInputElement).checked } })"
            >
          </label>
        </div>
      </div>
    </div>
  </div>
</template>
