<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import { clipboardApi } from '@/lib/tauri';
import {
  createDefaultClipboardSettings,
  normalizeClipboardSettings,
  type ClipboardSettings,
} from '@/lib/clipboardTypes';
import ClipboardHotkeyInput from './ClipboardHotkeyInput.vue';
import ClipboardWinVConfirmDialog from './ClipboardWinVConfirmDialog.vue';

const { t } = useI18n();
const model = reactive<ClipboardSettings>(createDefaultClipboardSettings());
const loading = ref(true);
const saving = ref(false);
const error = ref<string | null>(null);
const winVEnabled = ref(false);
const winVDialogOpen = ref(false);
const isElevated = ref(false);
const runAsAdminEnabled = ref(false);

async function refreshWinV() {
  try {
    winVEnabled.value = await clipboardApi.isWinVEnabled();
  } catch {
    // Leave as-is on error; surfaced via the global error banner already.
  }
}

async function refreshAdmin() {
  try {
    isElevated.value = await clipboardApi.isElevated();
    runAsAdminEnabled.value = await clipboardApi.isRunAsAdminEnabled();
  } catch {
    // non-fatal
  }
}

async function load() {
  loading.value = true;
  try {
    const got = await clipboardApi.getSettings();
    Object.assign(model, normalizeClipboardSettings(got));
    await refreshWinV();
    await refreshAdmin();
    error.value = null;
  } catch (e) {
    console.error('[clipboard] load settings failed:', e);
    error.value = `${t('clipboard.errors.loadFailed')} — ${e}`;
  } finally {
    loading.value = false;
  }
}

async function save() {
  saving.value = true;
  try {
    const payload = normalizeClipboardSettings(model);
    const updated = await clipboardApi.saveSettings(payload);
    Object.assign(model, normalizeClipboardSettings(updated));
    error.value = null;
  } catch (e) {
    console.error('[clipboard] save settings failed:', e);
    error.value = `${t('clipboard.errors.saveFailed')} — ${e}`;
  } finally {
    saving.value = false;
  }
}

function onWinVToggle(e: Event) {
  const target = e.target as HTMLInputElement;
  if (target.checked) {
    // Revert the UI until the user confirms the destructive change.
    target.checked = false;
    winVDialogOpen.value = true;
  } else {
    // Disabling is non-destructive (restores Windows default); no double-confirm needed.
    void disableWinV();
  }
}

async function disableWinV() {
  try {
    await clipboardApi.disableWinV();
    winVEnabled.value = false;
    model.use_win_v_replacement = false;
  } catch (e) {
    console.error('[clipboard] disableWinV failed:', e);
    error.value = `${t('clipboard.errors.winVFailed')} — ${e}`;
    await refreshWinV();
  }
}

async function onWinVConfirm() {
  winVDialogOpen.value = false;
  try {
    await clipboardApi.enableWinV();
    winVEnabled.value = true;
    model.use_win_v_replacement = true;
  } catch (e) {
    console.error('[clipboard] enableWinV failed:', e);
    error.value = `${t('clipboard.errors.winVFailed')} — ${e}`;
    await refreshWinV();
  }
}

function onWinVCancel() {
  winVDialogOpen.value = false;
}

async function onRunAsAdminToggle(e: Event) {
  const target = e.target as HTMLInputElement;
  const next = target.checked;
  try {
    await clipboardApi.setRunAsAdmin(next);
    runAsAdminEnabled.value = next;
    model.run_as_admin = next;
  } catch (err) {
    console.error('[clipboard] setRunAsAdmin failed:', err);
    error.value = `${t('clipboard.errors.saveFailed')} — ${err}`;
    target.checked = runAsAdminEnabled.value;
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

      <!-- System integration -->
      <div class="space-y-3">
        <h4 class="text-xs font-semibold uppercase tracking-[0.12em] text-slate-500">
          {{ t('clipboard.settings.sectionSystem') }}
        </h4>

        <div class="space-y-2">
          <div class="flex items-start gap-2 text-xs text-orange-600">
            <span>⚠️</span>
            <span>{{ t('clipboard.settings.winVWarning') }}</span>
          </div>
          <label class="flex items-center justify-between gap-4">
            <div class="text-sm font-medium text-slate-800">
              {{ t('clipboard.settings.winVLabel') }}
            </div>
            <input
              type="checkbox"
              :checked="winVEnabled"
              @change="onWinVToggle($event)"
            />
          </label>
        </div>

        <div class="flex items-center justify-between gap-4">
          <div>
            <div class="text-sm font-medium text-slate-800">{{ t('clipboard.settings.adminLabel') }}</div>
            <div class="mt-1">
              <span
                class="rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.1em]"
                :class="isElevated
                  ? 'bg-emerald-100 text-emerald-700'
                  : 'bg-slate-100 text-slate-600'"
              >
                {{ isElevated ? t('clipboard.settings.adminCurrentStatusElevated') : t('clipboard.settings.adminCurrentStatusNormal') }}
              </span>
            </div>
          </div>
          <input type="checkbox" :checked="runAsAdminEnabled" @change="onRunAsAdminToggle($event)" />
        </div>
      </div>
    </div>

    <ClipboardWinVConfirmDialog
      :open="winVDialogOpen"
      @confirm="onWinVConfirm"
      @cancel="onWinVCancel"
    />
  </section>
</template>
