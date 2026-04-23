<script setup lang="ts">
import { emit } from '@tauri-apps/api/event';
import { computed, reactive, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import AboutTab from '@/components/clipboard-settings/AboutTab.vue';
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
const winVEnabled = ref(false);
const isElevated = ref(false);
const runAsAdminEnabled = ref(false);

const tabComponents = {
  general: GeneralTab,
  display: DisplayTab,
  shortcuts: ShortcutsTab,
  data: DataTab,
  preview: PreviewTab,
  appFilter: AppFilterTab,
  about: AboutTab,
} as const;

const currentTabComponent = computed(
  () => tabComponents[activeTab.value],
);

function buildNextSettings(patch: DeepPartial<ClipboardSettings>): ClipboardSettings {
  return normalizeClipboardSettings({
    ...model,
    ...patch,
    display: {
      ...model.display,
      ...(patch.display ?? {}),
    },
    preview: {
      ...model.preview,
      ...(patch.preview ?? {}),
    },
    panel: {
      ...model.panel,
      ...(patch.panel ?? {}),
    },
    shortcuts: {
      ...model.shortcuts,
      ...(patch.shortcuts ?? {}),
      quick_paste: patch.shortcuts?.quick_paste
        ? [...patch.shortcuts.quick_paste]
        : [...model.shortcuts.quick_paste],
      focus_search: patch.shortcuts?.focus_search
        ? [...patch.shortcuts.focus_search]
        : [...model.shortcuts.focus_search],
    },
    navigation: {
      ...model.navigation,
      ...(patch.navigation ?? {}),
    },
    toolbar: {
      ...model.toolbar,
      ...(patch.toolbar ?? {}),
      items: patch.toolbar?.items
        ? [...patch.toolbar.items]
        : [...model.toolbar.items],
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
    <div class="flex items-center justify-between border-b border-slate-100 px-5 py-4">
      <div>
        <h3 class="text-lg font-semibold text-slate-900">{{ t('clipboard.settings.title') }}</h3>
        <p class="mt-1 text-xs text-slate-500">{{ t(`clipboard.settings.tabs.${activeTab}`) }}</p>
      </div>
      <span v-if="saving" class="text-xs text-slate-400">{{ t('clipboard.settings.saving') }}</span>
    </div>

    <div v-if="error" class="mx-5 mt-4 rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-600">
      {{ error }}
    </div>

    <div v-if="loading" class="px-5 py-8 text-sm text-slate-400">
      {{ t('clipboard.loading') }}
    </div>

    <div v-else class="flex flex-col gap-4 p-4">
      <nav
        class="mx-auto flex w-fit max-w-full items-center gap-0.5 overflow-x-auto rounded-full border border-slate-200/70 bg-slate-100/80 p-1 shadow-[inset_0_1px_0_rgba(255,255,255,0.7),inset_0_-1px_0_rgba(15,23,42,0.04)] backdrop-blur-sm"
        role="tablist"
      >
        <button
          v-for="tab in CLIPBOARD_SETTINGS_TABS"
          :key="tab.id"
          type="button"
          role="tab"
          :aria-selected="activeTab === tab.id"
          class="group relative flex shrink-0 items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition-all duration-200 ease-out focus:outline-none focus-visible:ring-2 focus-visible:ring-slate-900/30"
          :class="activeTab === tab.id
            ? 'bg-white text-slate-900 shadow-[0_1px_2px_rgba(15,23,42,0.08),0_2px_8px_rgba(15,23,42,0.06)] ring-1 ring-slate-200/60'
            : 'text-slate-500 hover:text-slate-700'"
          @click="activeTab = tab.id"
        >
          <component
            :is="tab.icon"
            class="h-3.5 w-3.5 transition-colors"
            :class="activeTab === tab.id
              ? 'text-slate-900'
              : 'text-slate-400 group-hover:text-slate-500'"
            :stroke-width="activeTab === tab.id ? 2.25 : 2"
          />
          <span class="whitespace-nowrap tracking-[0.01em]">{{ t(tab.labelKey) }}</span>
        </button>
      </nav>

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
  </section>
</template>
