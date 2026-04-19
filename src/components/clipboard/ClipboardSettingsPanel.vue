<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import { clipboardApi } from '@/lib/tauri';
import type { ClipboardSettings } from '@/lib/clipboardTypes';
import ClipboardHotkeyInput from './ClipboardHotkeyInput.vue';

const { t } = useI18n();

const defaults: ClipboardSettings = {
  enabled: true,
  hotkey: 'Alt+C',
  max_items: 1000,
  retain_days: 30,
  max_item_bytes: 10 * 1024 * 1024,
  preview_delay_ms: 500,
  enable_text_preview: false,
  use_win_v_replacement: false,
  run_as_admin: false,
  show_startup_notification: true,
};

const model = reactive<ClipboardSettings>({ ...defaults });
const loading = ref(true);
const saving = ref(false);
const error = ref<string | null>(null);

async function load() {
  loading.value = true;
  try {
    const got = await clipboardApi.getSettings();
    Object.assign(model, got);
    error.value = null;
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function save() {
  saving.value = true;
  try {
    const updated = await clipboardApi.saveSettings({ ...model });
    Object.assign(model, updated);
    error.value = null;
  } catch (e) {
    error.value = String(e);
  } finally {
    saving.value = false;
  }
}

onMounted(load);
</script>

<template>
  <section class="space-y-5 rounded-2xl border border-slate-200 bg-white p-5">
    <div class="flex items-center justify-between">
      <h3 class="text-lg font-semibold text-slate-900">{{ t('clipboard.settings.title') }}</h3>
      <span v-if="saving" class="text-xs text-slate-400">{{ t('clipboard.settings.saving') }}</span>
    </div>

    <div v-if="error" class="rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs text-rose-600">
      {{ error }}
    </div>

    <div v-if="!loading" class="space-y-5">
      <!-- Basic -->
      <div class="space-y-3">
        <h4 class="text-xs font-semibold uppercase tracking-[0.12em] text-slate-500">
          {{ t('clipboard.settings.sectionBasic') }}
        </h4>

        <label class="flex items-center justify-between gap-4">
          <div>
            <div class="text-sm font-medium text-slate-800">{{ t('clipboard.settings.enableLabel') }}</div>
            <div class="text-xs text-slate-500">{{ t('clipboard.settings.enableHint') }}</div>
          </div>
          <input type="checkbox" v-model="model.enabled" @change="save" />
        </label>

        <label class="flex items-center justify-between gap-4">
          <div class="text-sm font-medium text-slate-800">
            {{ t('clipboard.settings.startupNotificationLabel') }}
          </div>
          <input type="checkbox" v-model="model.show_startup_notification" @change="save" />
        </label>
      </div>

      <!-- Hotkey -->
      <div class="space-y-3">
        <h4 class="text-xs font-semibold uppercase tracking-[0.12em] text-slate-500">
          {{ t('clipboard.settings.sectionHotkey') }}
        </h4>
        <label class="flex items-center justify-between gap-4">
          <div class="text-sm font-medium text-slate-800">{{ t('clipboard.settings.hotkeyLabel') }}</div>
          <ClipboardHotkeyInput v-model="model.hotkey" @change="save" />
        </label>
      </div>

      <!-- Data management -->
      <div class="space-y-3">
        <h4 class="text-xs font-semibold uppercase tracking-[0.12em] text-slate-500">
          {{ t('clipboard.settings.sectionData') }}
        </h4>
        <label class="flex items-center justify-between gap-4">
          <div class="text-sm font-medium text-slate-800">{{ t('clipboard.settings.maxItemsLabel') }}</div>
          <input
            type="number"
            min="0"
            class="w-28 rounded-lg border border-slate-300 px-2 py-1 text-sm"
            v-model.number="model.max_items"
            @change="save"
          />
        </label>
        <label class="flex items-center justify-between gap-4">
          <div class="text-sm font-medium text-slate-800">{{ t('clipboard.settings.retainDaysLabel') }}</div>
          <input
            type="number"
            min="0"
            class="w-28 rounded-lg border border-slate-300 px-2 py-1 text-sm"
            v-model.number="model.retain_days"
            @change="save"
          />
        </label>
        <label class="flex items-center justify-between gap-4">
          <div class="text-sm font-medium text-slate-800">{{ t('clipboard.settings.previewDelayLabel') }}</div>
          <input
            type="number"
            min="0"
            class="w-28 rounded-lg border border-slate-300 px-2 py-1 text-sm"
            v-model.number="model.preview_delay_ms"
            @change="save"
          />
        </label>
        <label class="flex items-center justify-between gap-4">
          <div class="text-sm font-medium text-slate-800">
            {{ t('clipboard.settings.enableTextPreviewLabel') }}
          </div>
          <input type="checkbox" v-model="model.enable_text_preview" @change="save" />
        </label>
      </div>
    </div>
  </section>
</template>
