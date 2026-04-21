<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import type { DeepPartial, ClipboardSettings } from '@/lib/clipboardTypes';

const props = defineProps<{
  settings: ClipboardSettings;
  isElevated: boolean;
  runAsAdminEnabled: boolean;
}>();

const emit = defineEmits<{
  patch: [patch: DeepPartial<ClipboardSettings>];
  toggleRunAsAdmin: [enabled: boolean];
}>();

const { t } = useI18n();

function patch(next: DeepPartial<ClipboardSettings>) {
  emit('patch', next);
}

function onRunAsAdminChange(event: Event) {
  emit('toggleRunAsAdmin', (event.target as HTMLInputElement).checked);
}
</script>

<template>
  <div class="space-y-4">
    <div class="rounded-2xl border border-slate-200 bg-slate-50/60 p-4">
      <div class="grid gap-4 md:grid-cols-2">
        <label class="flex items-center justify-between gap-4 rounded-xl bg-white p-3 shadow-sm">
          <div>
            <div class="text-sm font-medium text-slate-900">{{ t('clipboard.settings.enableLabel') }}</div>
            <div class="text-xs text-slate-500">{{ t('clipboard.settings.enableHint') }}</div>
          </div>
          <input
            type="checkbox"
            :checked="props.settings.enabled"
            @change="patch({ enabled: ($event.target as HTMLInputElement).checked })"
          >
        </label>

        <label class="flex items-center justify-between gap-4 rounded-xl bg-white p-3 shadow-sm">
          <div class="text-sm font-medium text-slate-900">
            {{ t('clipboard.settings.startupNotificationLabel') }}
          </div>
          <input
            type="checkbox"
            :checked="props.settings.show_startup_notification"
            @change="patch({ show_startup_notification: ($event.target as HTMLInputElement).checked })"
          >
        </label>
      </div>
    </div>

    <div class="grid gap-4 lg:grid-cols-2">
      <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.tabs.general') }}</h4>
        <div class="mt-4 space-y-3">
          <label class="flex items-center justify-between gap-4">
            <span class="text-sm text-slate-700">{{ t('clipboard.settings.general.followCursor') }}</span>
            <input
              type="checkbox"
              :checked="props.settings.panel.follow_cursor"
              @change="patch({ panel: { follow_cursor: ($event.target as HTMLInputElement).checked } })"
            >
          </label>

          <label class="flex items-center justify-between gap-4">
            <span class="text-sm text-slate-700">{{ t('clipboard.settings.general.rememberPosition') }}</span>
            <input
              type="checkbox"
              :checked="props.settings.panel.remember_position"
              @change="patch({ panel: { remember_position: ($event.target as HTMLInputElement).checked } })"
            >
          </label>

          <label class="flex items-center justify-between gap-4">
            <span class="text-sm text-slate-700">{{ t('clipboard.settings.general.animate') }}</span>
            <input
              type="checkbox"
              :checked="props.settings.panel.animate"
              @change="patch({ panel: { animate: ($event.target as HTMLInputElement).checked } })"
            >
          </label>

          <label class="flex items-center justify-between gap-4">
            <span class="text-sm text-slate-700">{{ t('clipboard.settings.general.useMica') }}</span>
            <input
              type="checkbox"
              :checked="props.settings.panel.use_mica"
              @change="patch({ panel: { use_mica: ($event.target as HTMLInputElement).checked } })"
            >
          </label>
        </div>
      </div>

      <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.sectionSystem') }}</h4>
        <div class="mt-4 flex items-start justify-between gap-4 rounded-xl border border-slate-100 bg-slate-50 px-3 py-3">
          <div>
            <div class="text-sm font-medium text-slate-800">{{ t('clipboard.settings.adminLabel') }}</div>
            <div class="mt-2">
              <span
                class="rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em]"
                :class="props.isElevated ? 'bg-emerald-100 text-emerald-700' : 'bg-slate-200 text-slate-600'"
              >
                {{
                  props.isElevated
                    ? t('clipboard.settings.adminCurrentStatusElevated')
                    : t('clipboard.settings.adminCurrentStatusNormal')
                }}
              </span>
            </div>
          </div>
          <input
            type="checkbox"
            :checked="props.runAsAdminEnabled"
            @change="onRunAsAdminChange"
          >
        </div>
      </div>
    </div>
  </div>
</template>
