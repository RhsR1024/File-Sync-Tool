<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import ClipboardHotkeyInput from '@/components/clipboard/ClipboardHotkeyInput.vue';
import type { DeepPartial, ClipboardSettings } from '@/lib/clipboardTypes';

const props = defineProps<{
  settings: ClipboardSettings;
  winVEnabled: boolean;
}>();

const emit = defineEmits<{
  patch: [patch: DeepPartial<ClipboardSettings>];
  toggleWinV: [enabled: boolean];
}>();

const { t } = useI18n();
const hotkeyModel = ref(props.settings.hotkey);

watch(
  () => props.settings.hotkey,
  (value) => {
    hotkeyModel.value = value;
  },
);

const shortcutRows = computed(() => [
  {
    label: t('clipboard.actions.paste'),
    value: props.settings.shortcuts.paste,
  },
  {
    label: t('clipboard.actions.pastePlain'),
    value: props.settings.shortcuts.plain_paste,
  },
  {
    label: t('clipboard.actions.delete'),
    value: props.settings.shortcuts.delete,
  },
  {
    label: t('clipboard.actions.favorite'),
    value: props.settings.shortcuts.favorite,
  },
  {
    label: t('clipboard.actions.close'),
    value: props.settings.shortcuts.close,
  },
  {
    label: t('clipboard.settings.shortcuts.focusSearch'),
    value: props.settings.shortcuts.focus_search.join(' / '),
  },
  {
    label: t('clipboard.settings.shortcuts.quickPaste'),
    value: 'Alt+1 - Alt+9',
  },
]);

function patch(next: DeepPartial<ClipboardSettings>) {
  emit('patch', next);
}

function onHotkeyChange() {
  patch({ hotkey: hotkeyModel.value });
}

function onWinVChange(event: Event) {
  emit('toggleWinV', (event.target as HTMLInputElement).checked);
}
</script>

<template>
  <div class="space-y-4">
    <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.sectionHotkey') }}</h4>
      <div class="mt-4 space-y-4">
        <label class="flex items-center justify-between gap-4">
          <span class="text-sm text-slate-700">{{ t('clipboard.settings.hotkeyLabel') }}</span>
          <ClipboardHotkeyInput v-model="hotkeyModel" @change="onHotkeyChange" />
        </label>

        <label class="flex items-center justify-between gap-4">
          <span class="text-sm text-slate-700">{{ t('clipboard.settings.shortcuts.navigation') }}</span>
          <input
            type="checkbox"
            :checked="props.settings.navigation.enabled"
            @change="patch({ navigation: { enabled: ($event.target as HTMLInputElement).checked } })"
          >
        </label>

        <div class="rounded-xl border border-amber-200 bg-amber-50 px-3 py-3 text-xs text-amber-700">
          {{ t('clipboard.settings.winVWarning') }}
        </div>

        <label class="flex items-center justify-between gap-4">
          <span class="text-sm text-slate-700">{{ t('clipboard.settings.winVLabel') }}</span>
          <input
            type="checkbox"
            :checked="props.winVEnabled"
            @change="onWinVChange"
          >
        </label>
      </div>
    </div>

    <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.shortcuts.defaults') }}</h4>
      <div class="mt-4 grid gap-3 md:grid-cols-2">
        <div
          v-for="row in shortcutRows"
          :key="row.label"
          class="rounded-xl border border-slate-100 bg-slate-50 px-3 py-2"
        >
          <div class="text-xs text-slate-500">{{ row.label }}</div>
          <code class="mt-1 block text-sm font-medium text-slate-700">{{ row.value }}</code>
        </div>
      </div>
    </div>
  </div>
</template>
