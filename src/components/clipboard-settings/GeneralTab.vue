<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import type { DeepPartial, ClipboardSettings } from '@/lib/clipboardTypes';
import { clipboardApi, type AdminTaskStatus } from '@/lib/tauri';

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
const taskLoading = ref(false);
const taskStatus = ref<AdminTaskStatus | null>(null);

const taskStatusLabelKey = computed(() => {
  if (taskLoading.value && !taskStatus.value) {
    return 'clipboard.settings.adminSchedulerStatusLoading';
  }
  if (!taskStatus.value?.installed) {
    return props.runAsAdminEnabled
      ? 'clipboard.settings.adminSchedulerStatusMissing'
      : 'clipboard.settings.adminSchedulerStatusDisabled';
  }
  if (!taskStatus.value.path_valid) {
    return 'clipboard.settings.adminSchedulerStatusRepair';
  }
  return 'clipboard.settings.adminSchedulerStatusReady';
});

const taskStatusBadgeClass = computed(() => {
  if (taskLoading.value && !taskStatus.value) {
    return 'bg-slate-200 text-slate-600';
  }
  if (!taskStatus.value?.installed) {
    return props.runAsAdminEnabled
      ? 'bg-amber-100 text-amber-700'
      : 'bg-slate-200 text-slate-600';
  }
  if (!taskStatus.value.path_valid) {
    return 'bg-orange-100 text-orange-700';
  }
  return 'bg-emerald-100 text-emerald-700';
});

const showFallbackHint = computed(
  () => props.runAsAdminEnabled && !taskLoading.value && !taskStatus.value?.installed,
);

function formatError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function patch(next: DeepPartial<ClipboardSettings>) {
  emit('patch', next);
}

function onRunAsAdminChange(event: Event) {
  emit('toggleRunAsAdmin', (event.target as HTMLInputElement).checked);
}

async function refreshTaskStatus() {
  taskLoading.value = true;
  try {
    taskStatus.value = await clipboardApi.adminTaskStatus();
  } catch (error) {
    taskStatus.value = {
      installed: false,
      path_valid: false,
      last_error: formatError(error),
    };
  } finally {
    taskLoading.value = false;
  }
}

async function repairAdminTask() {
  taskLoading.value = true;
  try {
    taskStatus.value = await clipboardApi.adminTaskCreate();
  } catch (error) {
    taskStatus.value = {
      installed: false,
      path_valid: false,
      last_error: formatError(error),
    };
  } finally {
    taskLoading.value = false;
  }
}

async function removeAdminTask() {
  taskLoading.value = true;
  try {
    taskStatus.value = await clipboardApi.adminTaskRemove();
  } catch (error) {
    taskStatus.value = {
      installed: taskStatus.value?.installed ?? false,
      path_valid: false,
      last_error: formatError(error),
    };
  } finally {
    taskLoading.value = false;
  }
}

watch(
  () => [props.runAsAdminEnabled, props.isElevated],
  () => {
    void refreshTaskStatus();
  },
  { immediate: true },
);
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

        <div class="mt-4 space-y-3">
          <div class="flex items-start justify-between gap-4 rounded-xl border border-slate-100 bg-slate-50 px-3 py-3">
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

          <div class="rounded-xl border border-slate-100 bg-slate-50 px-3 py-3">
            <div class="flex items-start justify-between gap-4">
              <div>
                <div class="text-sm font-medium text-slate-800">
                  {{ t('clipboard.settings.adminSchedulerLabel') }}
                </div>
                <div class="mt-1 text-xs text-slate-500">
                  {{ t('clipboard.settings.adminSchedulerHint') }}
                </div>
              </div>
              <span
                class="rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em]"
                :class="taskStatusBadgeClass"
              >
                {{ t(taskStatusLabelKey) }}
              </span>
            </div>

            <p v-if="showFallbackHint" class="mt-3 text-xs text-amber-700">
              {{ t('clipboard.settings.adminSchedulerFallbackHint') }}
            </p>
            <p v-else-if="taskStatus?.last_error" class="mt-3 text-xs text-rose-600">
              {{ taskStatus.last_error }}
            </p>

            <div class="mt-3 flex flex-wrap gap-2">
              <button
                type="button"
                class="rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-700 transition hover:border-slate-300 hover:bg-white disabled:cursor-not-allowed disabled:opacity-50"
                :disabled="taskLoading"
                @click="repairAdminTask"
              >
                {{
                  taskLoading
                    ? t('clipboard.loading')
                    : t('clipboard.settings.adminSchedulerRepair')
                }}
              </button>

              <button
                type="button"
                class="rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-700 transition hover:border-slate-300 hover:bg-white disabled:cursor-not-allowed disabled:opacity-50"
                :disabled="taskLoading || !taskStatus?.installed"
                @click="removeAdminTask"
              >
                {{ t('clipboard.settings.adminSchedulerRemove') }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
