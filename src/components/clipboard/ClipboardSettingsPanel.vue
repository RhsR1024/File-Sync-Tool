<script setup lang="ts">
import { emit } from '@tauri-apps/api/event';
import { computed, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import AppFilterTab from '@/components/clipboard-settings/AppFilterTab.vue';
import DataTab from '@/components/clipboard-settings/DataTab.vue';
import DisplayTab from '@/components/clipboard-settings/DisplayTab.vue';
import GeneralTab from '@/components/clipboard-settings/GeneralTab.vue';
import PreviewTab from '@/components/clipboard-settings/PreviewTab.vue';
import ShortcutsTab from '@/components/clipboard-settings/ShortcutsTab.vue';
import {
  CLIPBOARD_SETTINGS_TABS,
  type ClipboardSettingsTabId,
} from '@/lib/clipboardSettingsUi';
import { clipboardApi } from '@/lib/tauri';
import {
  createDefaultClipboardSettings,
  normalizeClipboardSettings,
  type ClipboardSettings,
  type DeepPartial,
} from '@/lib/clipboardTypes';

const { t } = useI18n();

const model = reactive<ClipboardSettings>(createDefaultClipboardSettings());
const loading = ref(true);
const saving = ref(false);
const error = ref<string | null>(null);
const activeTab = ref<ClipboardSettingsTabId>('general');
const savedNoticeVisible = ref(false);
const winVEnabled = ref(false);
const isElevated = ref(false);
const runAsAdminEnabled = ref(false);
const tablistRef = ref<HTMLElement | null>(null);
let savedNoticeTimer: ReturnType<typeof setTimeout> | null = null;

const tabComponents = {
  general: GeneralTab,
  display: DisplayTab,
  shortcuts: ShortcutsTab,
  data: DataTab,
  preview: PreviewTab,
  appFilter: AppFilterTab,
} as const;

const currentTabComponent = computed(
  () => tabComponents[activeTab.value],
);

function setActiveTab(id: ClipboardSettingsTabId) {
  activeTab.value = id;
}

function focusTab(index: number) {
  const button = tablistRef.value?.querySelector<HTMLButtonElement>(`[data-tab-index="${index}"]`);
  button?.focus();
}

function onTabKeydown(event: KeyboardEvent, index: number) {
  const lastIndex = CLIPBOARD_SETTINGS_TABS.length - 1;
  if (event.key === 'ArrowRight') {
    event.preventDefault();
    const nextIndex = index >= lastIndex ? 0 : index + 1;
    setActiveTab(CLIPBOARD_SETTINGS_TABS[nextIndex].id);
    focusTab(nextIndex);
  } else if (event.key === 'ArrowLeft') {
    event.preventDefault();
    const nextIndex = index <= 0 ? lastIndex : index - 1;
    setActiveTab(CLIPBOARD_SETTINGS_TABS[nextIndex].id);
    focusTab(nextIndex);
  } else if (event.key === 'Home') {
    event.preventDefault();
    setActiveTab(CLIPBOARD_SETTINGS_TABS[0].id);
    focusTab(0);
  } else if (event.key === 'End') {
    event.preventDefault();
    setActiveTab(CLIPBOARD_SETTINGS_TABS[lastIndex].id);
    focusTab(lastIndex);
  }
}

function showSavedNotice() {
  savedNoticeVisible.value = true;
  if (savedNoticeTimer !== null) {
    clearTimeout(savedNoticeTimer);
  }
  savedNoticeTimer = setTimeout(() => {
    savedNoticeVisible.value = false;
    savedNoticeTimer = null;
  }, 1800);
}

function buildNextSettings(patch: DeepPartial<ClipboardSettings>): ClipboardSettings {
  return normalizeClipboardSettings({
    enabled: patch.enabled ?? model.enabled,
    hotkey: patch.hotkey ?? model.hotkey,
    image_copy_hotkey_enabled:
      patch.image_copy_hotkey_enabled ?? model.image_copy_hotkey_enabled,
    image_copy_hotkey: patch.image_copy_hotkey ?? model.image_copy_hotkey,
    explorer_context_menu_enabled:
      patch.explorer_context_menu_enabled ?? model.explorer_context_menu_enabled,
    max_items: patch.max_items ?? model.max_items,
    retain_days: patch.retain_days ?? model.retain_days,
    max_item_bytes: patch.max_item_bytes ?? model.max_item_bytes,
    preview_delay_ms: patch.preview_delay_ms ?? model.preview_delay_ms,
    enable_text_preview: patch.enable_text_preview ?? model.enable_text_preview,
    use_win_v_replacement: patch.use_win_v_replacement ?? model.use_win_v_replacement,
    run_as_admin: patch.run_as_admin ?? model.run_as_admin,
    show_startup_notification: patch.show_startup_notification ?? model.show_startup_notification,
    dedup_strategy: patch.dedup_strategy ?? model.dedup_strategy,
    reinsert_on_self_copy: patch.reinsert_on_self_copy ?? model.reinsert_on_self_copy,
    display: {
      ...model.display,
      ...(patch.display ?? {}),
    },
    preview: {
      ...model.preview,
      ...(patch.preview ?? {}),
    },
    shortcuts: {
      ...model.shortcuts,
      ...(patch.shortcuts ?? {}),
      focus_search: patch.shortcuts?.focus_search
        ? [...patch.shortcuts.focus_search]
        : [...model.shortcuts.focus_search],
    },
    navigation: {
      ...model.navigation,
      ...(patch.navigation ?? {}),
    },
    data: {
      ...model.data,
      ...(patch.data ?? {}),
    },
    app_filter: {
      ...model.app_filter,
      ...(patch.app_filter ?? {}),
      patterns: patch.app_filter?.patterns
        ? [...patch.app_filter.patterns]
        : [...model.app_filter.patterns],
    },
  });
}

async function refreshSystemState() {
  try {
    const [winV, elevated, runAsAdmin] = await Promise.all([
      clipboardApi.isWinVEnabled(),
      clipboardApi.isElevated(),
      clipboardApi.isRunAsAdminEnabled(),
    ]);
    winVEnabled.value = winV;
    isElevated.value = elevated;
    runAsAdminEnabled.value = runAsAdmin;
  } catch {
    // Non-fatal. The panel still works with saved settings only.
  }
}

async function broadcastSettings() {
  await emit('clipboard-settings-updated', normalizeClipboardSettings(model));
}

async function load() {
  loading.value = true;
  try {
    const got = await clipboardApi.getSettings();
    Object.assign(model, normalizeClipboardSettings(got));
    await refreshSystemState();
    error.value = null;
  } catch (e) {
    console.error('[clipboard] load settings failed:', e);
    error.value = `${t('clipboard.errors.loadFailed')} - ${e}`;
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
    await refreshSystemState();
    await broadcastSettings();
    error.value = null;
    showSavedNotice();
  } catch (e) {
    console.error('[clipboard] save settings failed:', e);
    error.value = `${t('clipboard.errors.saveFailed')} - ${e}`;
  } finally {
    saving.value = false;
  }
}

function applyPatch(patch: DeepPartial<ClipboardSettings>) {
  Object.assign(model, buildNextSettings(patch));
  void save();
}

async function onToggleWinV(enabled: boolean) {
  try {
    if (enabled) {
      await clipboardApi.enableWinV();
    } else {
      await clipboardApi.disableWinV();
    }
    winVEnabled.value = enabled;
    model.use_win_v_replacement = enabled;
    await broadcastSettings();
    error.value = null;
  } catch (e) {
    console.error('[clipboard] toggle Win+V failed:', e);
    error.value = `${t('clipboard.errors.winVFailed')} - ${e}`;
    await refreshSystemState();
  }
}

async function onToggleRunAsAdmin(enabled: boolean) {
  try {
    await clipboardApi.setRunAsAdmin(enabled);
    runAsAdminEnabled.value = enabled;
    model.run_as_admin = enabled;
    await refreshSystemState();
    await broadcastSettings();
    error.value = null;
  } catch (e) {
    console.error('[clipboard] setRunAsAdmin failed:', e);
    error.value = `${t('clipboard.errors.saveFailed')} - ${e}`;
    await refreshSystemState();
  }
}

void load();
</script>

<template>
  <section class="rounded-2xl border border-slate-200 bg-white">
    <div v-if="error" class="mx-5 mt-4 rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-600">
      {{ error }}
    </div>

    <div v-if="loading" class="px-5 py-8 text-sm text-slate-400">
      {{ t('clipboard.loading') }}
    </div>

    <div v-else class="flex flex-col gap-4 p-4">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <nav
          ref="tablistRef"
          class="flex max-w-full items-center gap-1 overflow-x-auto rounded-full border border-slate-200/70 bg-slate-100/80 p-1 shadow-[inset_0_1px_0_rgba(255,255,255,0.7),inset_0_-1px_0_rgba(15,23,42,0.04)] backdrop-blur-sm"
          role="tablist"
          :aria-label="t('clipboard.settings.title')"
        >
          <button
            v-for="(tab, index) in CLIPBOARD_SETTINGS_TABS"
            :key="tab.id"
            type="button"
            :id="`clipboard-settings-tab-${tab.id}`"
            :data-tab-index="index"
            role="tab"
            :aria-controls="`clipboard-settings-panel-${tab.id}`"
            :aria-selected="activeTab === tab.id"
            :tabindex="activeTab === tab.id ? 0 : -1"
            class="group relative flex shrink-0 items-center gap-2 rounded-full px-4 py-2 text-base font-semibold transition-all duration-200 ease-out focus:outline-none focus-visible:ring-2 focus-visible:ring-slate-900/30"
            :class="activeTab === tab.id
              ? 'bg-white text-slate-900 shadow-[0_1px_2px_rgba(15,23,42,0.08),0_2px_8px_rgba(15,23,42,0.06)] ring-1 ring-slate-200/60'
              : 'text-slate-500 hover:text-slate-700'"
            @click="setActiveTab(tab.id)"
            @keydown="onTabKeydown($event, index)"
          >
            <component
              :is="tab.icon"
              class="h-4 w-4 transition-colors"
              :class="activeTab === tab.id
                ? 'text-slate-900'
                : 'text-slate-400 group-hover:text-slate-500'"
              :stroke-width="activeTab === tab.id ? 2.25 : 2"
            />
            <span class="whitespace-nowrap tracking-[0.01em]">{{ t(tab.labelKey) }}</span>
          </button>
        </nav>

        <span v-if="saving" class="shrink-0 text-xs text-slate-400">{{ t('clipboard.settings.saving') }}</span>
        <span v-else-if="savedNoticeVisible" class="shrink-0 text-xs text-emerald-600">{{ t('clipboard.settings.saved') }}</span>
      </div>

      <div
        :id="`clipboard-settings-panel-${activeTab}`"
        role="tabpanel"
        :aria-labelledby="`clipboard-settings-tab-${activeTab}`"
      >
        <component
          :is="currentTabComponent"
          :settings="model"
          :is-elevated="isElevated"
          :run-as-admin-enabled="runAsAdminEnabled"
          :win-v-enabled="winVEnabled"
          @patch="applyPatch"
          @toggle-run-as-admin="onToggleRunAsAdmin"
          @toggle-win-v="onToggleWinV"
        />
      </div>
    </div>
  </section>
</template>
