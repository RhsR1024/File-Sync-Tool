<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import ClipboardImportExportDialog from '@/components/clipboard/ClipboardImportExportDialog.vue';
import type { ClipboardStats, DeepPartial, ClipboardSettings } from '@/lib/clipboardTypes';
import {
  clipboardApi,
  confirmQuit,
  getAppPaths,
  getCustomDataDir,
  openDirectory,
  setCustomDataDir,
  type ClipboardImportMode,
} from '@/lib/tauri';

const props = defineProps<{
  settings: ClipboardSettings;
}>();

const emit = defineEmits<{
  patch: [patch: DeepPartial<ClipboardSettings>];
}>();

const { t } = useI18n();

const stats = ref<ClipboardStats | null>(null);
const currentDataDir = ref('');
const pending = ref<string | null>(null);
const error = ref<string | null>(null);
const feedback = ref<string | null>(null);

const dialogMode = ref<'export' | 'import' | null>(null);
const transferPath = ref('');
const includeImages = ref(true);
const importMode = ref<ClipboardImportMode>('replace');

const dialogOpen = computed(() => dialogMode.value !== null);

function patch(next: DeepPartial<ClipboardSettings>) {
  emit('patch', next);
}

function formatBytes(bytes: number | null | undefined): string {
  if (!bytes) return '0 B';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

function deriveDirectoryPath(path: string): string {
  return path.replace(/[\\/][^\\/]+$/, '');
}

async function refreshStatsAndPaths() {
  pending.value = 'refresh';
  try {
    const [statsResult, customDataDir, appPaths] = await Promise.all([
      clipboardApi.stats(),
      getCustomDataDir(),
      getAppPaths(),
    ]);
    stats.value = statsResult;
    currentDataDir.value = customDataDir || deriveDirectoryPath(appPaths[1]);
    error.value = null;
  } catch (reason) {
    console.error('[clipboard] refreshStatsAndPaths failed:', reason);
    error.value = `${t('clipboard.errors.loadFailed')} - ${reason}`;
  } finally {
    pending.value = null;
  }
}

function openTransferDialog(mode: 'export' | 'import') {
  dialogMode.value = mode;
  if (mode === 'export' && currentDataDir.value) {
    transferPath.value = `${currentDataDir.value}\\clipboard-export.zip`;
  } else if (mode === 'import') {
    transferPath.value = '';
  }
}

function closeTransferDialog() {
  dialogMode.value = null;
}

async function onConfirmTransfer() {
  if (!dialogMode.value || !transferPath.value.trim()) return;

  pending.value = dialogMode.value;
  try {
    if (dialogMode.value === 'export') {
      await clipboardApi.exportData(transferPath.value.trim(), includeImages.value);
      feedback.value = t('clipboard.transfer.exportSuccess');
    } else {
      const report = await clipboardApi.importData(transferPath.value.trim(), importMode.value);
      feedback.value = t('clipboard.transfer.importSuccess', { count: report.imported_items });
      if (report.backup_path) {
        feedback.value += ` ${t('clipboard.transfer.importBackup', { path: report.backup_path })}`;
      }
      await refreshStatsAndPaths();
    }
    error.value = null;
    closeTransferDialog();
  } catch (reason) {
    console.error('[clipboard] transfer failed:', reason);
    error.value = `${t('clipboard.transfer.failed')} - ${reason}`;
  } finally {
    pending.value = null;
  }
}

async function runMaintenance(action: 'optimize' | 'vacuum') {
  pending.value = action;
  try {
    if (action === 'optimize') {
      await clipboardApi.dbOptimize();
      feedback.value = t('clipboard.transfer.optimizeSuccess');
    } else {
      await clipboardApi.dbVacuum();
      feedback.value = t('clipboard.transfer.vacuumSuccess');
    }
    error.value = null;
    await refreshStatsAndPaths();
  } catch (reason) {
    console.error(`[clipboard] ${action} failed:`, reason);
    error.value = `${t('clipboard.transfer.failed')} - ${reason}`;
  } finally {
    pending.value = null;
  }
}

async function clearClipboardHistory() {
  if (!window.confirm(t('clipboard.transfer.clearHistoryConfirm'))) return;

  pending.value = 'clear';
  try {
    await clipboardApi.clearAll(false);
    feedback.value = t('clipboard.transfer.clearHistorySuccess');
    error.value = null;
    await refreshStatsAndPaths();
  } catch (reason) {
    console.error('[clipboard] clear history failed:', reason);
    error.value = `${t('clipboard.transfer.failed')} - ${reason}`;
  } finally {
    pending.value = null;
  }
}

async function resetClipboardConfig() {
  if (!window.confirm(t('clipboard.transfer.resetConfigConfirm'))) return;

  pending.value = 'reset-config';
  try {
    await clipboardApi.resetConfig();
    feedback.value = t('clipboard.transfer.resetConfigSuccess');
    error.value = null;
    window.location.reload();
  } catch (reason) {
    console.error('[clipboard] reset config failed:', reason);
    error.value = `${t('clipboard.transfer.failed')} - ${reason}`;
  } finally {
    pending.value = null;
  }
}

async function resetClipboardAll() {
  if (!window.confirm(t('clipboard.transfer.resetAllConfirm'))) return;

  pending.value = 'reset-all';
  try {
    await clipboardApi.resetAll();
    feedback.value = t('clipboard.transfer.resetAllSuccess');
    error.value = null;
    window.location.reload();
  } catch (reason) {
    console.error('[clipboard] reset all failed:', reason);
    error.value = `${t('clipboard.transfer.failed')} - ${reason}`;
  } finally {
    pending.value = null;
  }
}

async function changeDataDirectory(resetToDefault = false) {
  pending.value = 'data-dir';
  try {
    const nextPath = resetToDefault ? '' : await openDirectory();
    if (!resetToDefault && !nextPath) {
      pending.value = null;
      return;
    }

    await setCustomDataDir(nextPath ?? '');
    await refreshStatsAndPaths();
    feedback.value = resetToDefault
      ? t('clipboard.transfer.dataDirResetSuccess')
      : t('clipboard.transfer.dataDirChangeSuccess');
    error.value = null;

    if (window.confirm(t('clipboard.transfer.restartPrompt'))) {
      await confirmQuit();
    }
  } catch (reason) {
    console.error('[clipboard] change data directory failed:', reason);
    error.value = `${t('clipboard.transfer.failed')} - ${reason}`;
  } finally {
    pending.value = null;
  }
}

onMounted(() => {
  void refreshStatsAndPaths();
});
</script>

<template>
  <div class="space-y-4">
    <div
      v-if="error"
      class="rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-600"
    >
      {{ error }}
    </div>

    <div
      v-if="feedback"
      class="rounded-2xl border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm text-emerald-700"
    >
      {{ feedback }}
    </div>

    <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
      <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.settings.tabs.data') }}</h4>
      <div class="mt-4 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <label class="space-y-2">
          <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.maxItemsLabel') }}</div>
          <input
            type="number"
            min="0"
            class="w-full rounded-xl border border-slate-200 px-3 py-2 text-sm"
            :value="props.settings.data.max_items"
            @change="patch({ data: { max_items: Number(($event.target as HTMLInputElement).value) } })"
          >
        </label>

        <label class="space-y-2">
          <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.retainDaysLabel') }}</div>
          <input
            type="number"
            min="0"
            class="w-full rounded-xl border border-slate-200 px-3 py-2 text-sm"
            :value="props.settings.data.retain_days"
            @change="patch({ data: { retain_days: Number(($event.target as HTMLInputElement).value) } })"
          >
        </label>

        <label class="space-y-2">
          <div class="text-sm font-medium text-slate-700">{{ t('clipboard.settings.data.maxItemBytes') }}</div>
          <input
            type="number"
            min="0"
            step="1024"
            class="w-full rounded-xl border border-slate-200 px-3 py-2 text-sm"
            :value="props.settings.data.max_item_bytes"
            @change="patch({ data: { max_item_bytes: Number(($event.target as HTMLInputElement).value) } })"
          >
        </label>

        <label class="space-y-2">
          <div class="text-sm font-medium text-slate-700">{{ t('clipboard.transfer.dedupLabel') }}</div>
          <select
            class="w-full rounded-xl border border-slate-200 px-3 py-2 text-sm"
            :value="props.settings.dedup_strategy"
            @change="patch({ dedup_strategy: ($event.target as HTMLSelectElement).value as ClipboardSettings['dedup_strategy'] })"
          >
            <option value="move_to_top">{{ t('clipboard.transfer.dedup.moveToTop') }}</option>
            <option value="ignore">{{ t('clipboard.transfer.dedup.ignore') }}</option>
            <option value="always_new">{{ t('clipboard.transfer.dedup.alwaysNew') }}</option>
          </select>
        </label>
      </div>
    </div>

    <div class="grid gap-4 xl:grid-cols-2">
      <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <div class="flex items-center justify-between gap-3">
          <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.transfer.statsTitle') }}</h4>
          <button
            type="button"
            class="rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-700 transition hover:bg-slate-50"
            :disabled="pending === 'refresh'"
            @click="refreshStatsAndPaths"
          >
            {{ t('clipboard.transfer.refreshStats') }}
          </button>
        </div>

        <div class="mt-4 grid gap-3 sm:grid-cols-2">
          <div class="rounded-xl border border-slate-100 bg-slate-50 px-4 py-3">
            <div class="text-xs uppercase tracking-[0.14em] text-slate-500">{{ t('clipboard.transfer.totalItems') }}</div>
            <div class="mt-2 text-2xl font-semibold text-slate-900">{{ stats?.total ?? 0 }}</div>
          </div>
          <div class="rounded-xl border border-slate-100 bg-slate-50 px-4 py-3">
            <div class="text-xs uppercase tracking-[0.14em] text-slate-500">{{ t('clipboard.transfer.dbSize') }}</div>
            <div class="mt-2 text-2xl font-semibold text-slate-900">{{ formatBytes(stats?.db_bytes) }}</div>
          </div>
          <div class="rounded-xl border border-slate-100 bg-slate-50 px-4 py-3">
            <div class="text-xs uppercase tracking-[0.14em] text-slate-500">{{ t('clipboard.transfer.imageCount') }}</div>
            <div class="mt-2 text-2xl font-semibold text-slate-900">{{ stats?.image_count ?? 0 }}</div>
          </div>
          <div class="rounded-xl border border-slate-100 bg-slate-50 px-4 py-3">
            <div class="text-xs uppercase tracking-[0.14em] text-slate-500">{{ t('clipboard.transfer.imageSize') }}</div>
            <div class="mt-2 text-2xl font-semibold text-slate-900">{{ formatBytes(stats?.images_bytes) }}</div>
          </div>
        </div>
      </div>

      <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.transfer.locationTitle') }}</h4>
        <div class="mt-4 rounded-xl border border-slate-100 bg-slate-50 px-4 py-3">
          <div class="text-xs uppercase tracking-[0.14em] text-slate-500">{{ t('clipboard.transfer.currentDataDir') }}</div>
          <div class="mt-2 break-all text-sm text-slate-800">{{ currentDataDir || '-' }}</div>
        </div>
        <div class="mt-4 flex flex-wrap gap-2">
          <button
            type="button"
            class="rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-700 transition hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="pending === 'data-dir'"
            @click="changeDataDirectory(false)"
          >
            {{ t('clipboard.transfer.changeDataDir') }}
          </button>
          <button
            type="button"
            class="rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-700 transition hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="pending === 'data-dir'"
            @click="changeDataDirectory(true)"
          >
            {{ t('clipboard.transfer.resetDataDir') }}
          </button>
        </div>
      </div>
    </div>

    <div class="grid gap-4 xl:grid-cols-2">
      <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.transfer.transferTitle') }}</h4>
        <div class="mt-4 flex flex-wrap gap-2">
          <button
            type="button"
            class="rounded-lg bg-slate-900 px-3 py-1.5 text-xs font-medium text-white transition hover:bg-slate-700"
            @click="openTransferDialog('export')"
          >
            {{ t('clipboard.transfer.openExport') }}
          </button>
          <button
            type="button"
            class="rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-700 transition hover:bg-slate-50"
            @click="openTransferDialog('import')"
          >
            {{ t('clipboard.transfer.openImport') }}
          </button>
        </div>
      </div>

      <div class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm">
        <h4 class="text-sm font-semibold text-slate-900">{{ t('clipboard.transfer.maintenanceTitle') }}</h4>
        <div class="mt-4 flex flex-wrap gap-2">
          <button
            type="button"
            class="rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-700 transition hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="pending !== null"
            @click="runMaintenance('optimize')"
          >
            {{ t('clipboard.transfer.optimize') }}
          </button>
          <button
            type="button"
            class="rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-700 transition hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="pending !== null"
            @click="runMaintenance('vacuum')"
          >
            {{ t('clipboard.transfer.vacuum') }}
          </button>
          <button
            type="button"
            class="rounded-lg border border-amber-200 px-3 py-1.5 text-xs font-medium text-amber-700 transition hover:bg-amber-50 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="pending !== null"
            @click="clearClipboardHistory"
          >
            {{ t('clipboard.transfer.clearHistory') }}
          </button>
          <button
            type="button"
            class="rounded-lg border border-rose-200 px-3 py-1.5 text-xs font-medium text-rose-700 transition hover:bg-rose-50 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="pending !== null"
            @click="resetClipboardConfig"
          >
            {{ t('clipboard.transfer.resetConfig') }}
          </button>
          <button
            type="button"
            class="rounded-lg bg-rose-600 px-3 py-1.5 text-xs font-medium text-white transition hover:bg-rose-500 disabled:cursor-not-allowed disabled:bg-rose-300"
            :disabled="pending !== null"
            @click="resetClipboardAll"
          >
            {{ t('clipboard.transfer.resetAll') }}
          </button>
        </div>
      </div>
    </div>

    <ClipboardImportExportDialog
      :open="dialogOpen"
      :mode="dialogMode ?? 'export'"
      :path="transferPath"
      :include-images="includeImages"
      :import-mode="importMode"
      :pending="pending === 'export' || pending === 'import'"
      @close="closeTransferDialog"
      @confirm="onConfirmTransfer"
      @update:path="transferPath = $event"
      @update:include-images="includeImages = $event"
      @update:import-mode="importMode = $event"
    />
  </div>
</template>
