<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { AlertTriangle, Download, RefreshCw, ShieldCheck, X } from 'lucide-vue-next';

import { updaterApi } from '@/lib/tauri';
import { addLog } from '@/lib/store';
import { useUpdater } from '@/composables/useUpdater';
import { formatReleaseDate } from '@/pages/about/version';

defineOptions({ name: 'UpdateDialog' });

const { t } = useI18n();
const { state, progress, dialogOpen, dialogState, dialogError } = useUpdater();

const latestEntry = computed(() => {
  const manifest = state.value?.manifest;
  if (!manifest) {
    return null;
  }
  return manifest.versions.find((entry) => entry.version === manifest.latest) ?? manifest.versions[0] ?? null;
});

const pendingEntry = computed(() => state.value?.pending_update ?? null);

const percent = computed(() => {
  const payload = progress.value;
  if (!payload?.total || payload.total <= 0) {
    return null;
  }
  return Math.min(100, Math.round((payload.downloaded / payload.total) * 100));
});

function formatBytes(value: number) {
  if (value < 1024) {
    return `${value} B`;
  }
  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }
  if (value < 1024 * 1024 * 1024) {
    return `${(value / 1024 / 1024).toFixed(2)} MB`;
  }
  return `${(value / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

async function startDownload() {
  dialogState.value = 'downloading';
  dialogError.value = null;
  dialogOpen.value = true;
  try {
    await updaterApi.startDownload();
  } catch (error) {
    dialogState.value = 'network_error';
    dialogError.value = String(error);
  }
}

async function cancelDownload() {
  try {
    await updaterApi.cancelDownload();
    addLog(`[updater] ${t('updater.toast.cancelled')}`, 'info');
  } catch (error) {
    dialogState.value = 'network_error';
    dialogError.value = String(error);
    return;
  }

  dialogState.value = 'closed';
  dialogError.value = null;
  dialogOpen.value = false;
}

async function applyNow() {
  try {
    await updaterApi.applyNow();
  } catch (error) {
    dialogState.value = 'network_error';
    dialogError.value = String(error);
    addLog(`[updater] ${t('updater.toast.restartFailed', { detail: String(error) })}`, 'error');
  }
}

function closeDialog() {
  dialogOpen.value = false;
  if (dialogState.value !== 'ready' && dialogState.value !== 'resume') {
    dialogState.value = 'closed';
  }
}

function remindLater() {
  dialogOpen.value = false;
  dialogError.value = null;
  if (state.value?.pending_update) {
    dialogState.value = 'resume';
  } else {
    dialogState.value = 'closed';
  }
}
</script>

<template>
  <transition
    enter-active-class="transition duration-180 ease-out"
    enter-from-class="opacity-0"
    enter-to-class="opacity-100"
    leave-active-class="transition duration-140 ease-in"
    leave-from-class="opacity-100"
    leave-to-class="opacity-0"
  >
    <div
      v-if="dialogOpen"
      class="fixed inset-0 z-[90] flex items-center justify-center bg-slate-950/55 p-4 backdrop-blur-sm"
    >
      <div class="relative w-full max-w-xl overflow-hidden rounded-[28px] border border-slate-200 bg-white shadow-[0_28px_80px_rgba(15,23,42,0.36)]">
        <div class="absolute inset-x-0 top-0 h-1 bg-gradient-to-r from-cyan-400 via-sky-500 to-indigo-500"></div>
        <button
          type="button"
          class="absolute right-4 top-4 rounded-full p-2 text-slate-400 transition hover:bg-slate-100 hover:text-slate-700"
          @click="closeDialog"
        >
          <X class="h-4 w-4" />
        </button>

        <div class="space-y-6 px-6 py-7">
          <template v-if="dialogState === 'found' && latestEntry">
            <div class="space-y-2">
              <p class="text-xs font-semibold uppercase tracking-[0.24em] text-sky-500">
                {{ t('about.title') }}
              </p>
              <h2 class="text-2xl font-semibold text-slate-950">
                {{ t('updater.dialog.titleFound') }}
              </h2>
              <p class="text-sm leading-6 text-slate-600">
                {{
                  t('updater.dialog.bodyCurrentLatest', {
                    current: state?.current ?? '',
                    latest: latestEntry.version,
                    date: latestEntry.released_at,
                  })
                }}
              </p>
            </div>

            <div class="rounded-2xl border border-slate-200 bg-slate-50/80 p-4">
              <div class="flex items-center justify-between gap-3">
                <div>
                  <div class="text-lg font-semibold text-slate-900">{{ latestEntry.version }}</div>
                  <div class="text-sm text-slate-500">
                    {{ t('about.bannerReleasedOn', { date: formatReleaseDate(latestEntry.released_at) }) }}
                  </div>
                </div>
                <div class="rounded-full bg-sky-100 px-3 py-1 text-xs font-semibold text-sky-700">
                  {{ t('about.upgradeCta') }}
                </div>
              </div>

              <div class="mt-4 space-y-2">
                <p class="text-xs font-semibold uppercase tracking-[0.18em] text-slate-500">
                  {{ t('updater.dialog.changelogHeader') }}
                </p>
                <ul class="list-disc space-y-1 pl-5 text-sm leading-6 text-slate-700">
                  <li v-for="(line, index) in latestEntry.changelog" :key="index">{{ line }}</li>
                  <li v-if="latestEntry.changelog.length === 0" class="list-none pl-0 text-slate-400">
                    {{ t('about.changelogEmpty') }}
                  </li>
                </ul>
              </div>
            </div>

            <div class="flex justify-end gap-3">
              <button
                type="button"
                class="rounded-full border border-slate-200 px-4 py-2 text-sm font-medium text-slate-700 transition hover:bg-slate-50"
                @click="remindLater"
              >
                {{ t('updater.dialog.actionLater') }}
              </button>
              <button
                type="button"
                class="inline-flex items-center gap-2 rounded-full bg-sky-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-sky-700"
                @click="startDownload"
              >
                <Download class="h-4 w-4" />
                {{ t('updater.dialog.actionUpgrade') }}
              </button>
            </div>
          </template>

          <template v-else-if="dialogState === 'downloading'">
            <div class="space-y-2">
              <p class="text-xs font-semibold uppercase tracking-[0.24em] text-sky-500">
                {{ t('about.title') }}
              </p>
              <h2 class="text-2xl font-semibold text-slate-950">
                {{ t('updater.dialog.titleDownloading', { version: latestEntry?.version ?? '' }) }}
              </h2>
              <p class="text-sm leading-6 text-slate-600">
                {{ t('updater.dialog.changelogHeader') }}
              </p>
            </div>

            <div class="rounded-2xl border border-slate-200 bg-slate-50/80 p-4">
              <div class="h-3 overflow-hidden rounded-full bg-slate-200">
                <div
                  class="h-full rounded-full bg-gradient-to-r from-cyan-400 via-sky-500 to-indigo-500 transition-all duration-150"
                  :style="{ width: `${percent ?? 12}%` }"
                ></div>
              </div>
              <p class="mt-3 text-sm text-slate-600" v-if="progress && percent !== null">
                {{
                  t('updater.dialog.progress', {
                    percent,
                    downloaded: formatBytes(progress.downloaded),
                    total: formatBytes(progress.total ?? 0),
                    speed: formatBytes(progress.speed_bps),
                  })
                }}
              </p>
              <p class="mt-3 text-sm text-slate-600" v-else-if="progress">
                {{
                  t('updater.dialog.progressUnknownTotal', {
                    downloaded: formatBytes(progress.downloaded),
                    speed: formatBytes(progress.speed_bps),
                  })
                }}
              </p>
            </div>

            <div class="flex justify-end">
              <button
                type="button"
                class="rounded-full border border-slate-200 px-4 py-2 text-sm font-medium text-slate-700 transition hover:bg-slate-50"
                @click="cancelDownload"
              >
                {{ t('updater.dialog.actionCancel') }}
              </button>
            </div>
          </template>

          <template v-else-if="dialogState === 'ready' || dialogState === 'resume'">
            <div class="space-y-2">
              <p class="text-xs font-semibold uppercase tracking-[0.24em] text-emerald-500">
                {{ t('about.title') }}
              </p>
              <h2 class="flex items-center gap-2 text-2xl font-semibold text-slate-950">
                <ShieldCheck class="h-5 w-5 text-emerald-500" />
                {{ dialogState === 'resume' ? t('updater.dialog.titleResume') : t('updater.dialog.titleReady') }}
              </h2>
              <p class="text-sm leading-6 text-slate-600">
                {{
                  dialogState === 'resume'
                    ? t('updater.dialog.bodyResume', { version: pendingEntry?.target_version ?? '' })
                    : t('updater.dialog.bodyCurrentLatest', {
                        current: state?.current ?? '',
                        latest: pendingEntry?.target_version ?? latestEntry?.version ?? '',
                        date: latestEntry?.released_at ?? '',
                      })
                }}
              </p>
            </div>

            <div class="rounded-2xl border border-emerald-200 bg-emerald-50/80 p-4 text-sm text-emerald-800">
              <div class="font-medium">{{ pendingEntry?.target_version ?? latestEntry?.version }}</div>
              <div class="mt-1 break-all text-xs text-emerald-700/80">
                {{ pendingEntry?.temp_path }}
              </div>
            </div>

            <div class="flex justify-end gap-3">
              <button
                type="button"
                class="rounded-full border border-slate-200 px-4 py-2 text-sm font-medium text-slate-700 transition hover:bg-slate-50"
                @click="remindLater"
              >
                {{ t('updater.dialog.actionLaterRestart') }}
              </button>
              <button
                type="button"
                class="inline-flex items-center gap-2 rounded-full bg-emerald-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-emerald-700"
                @click="applyNow"
              >
                <RefreshCw class="h-4 w-4" />
                {{ t('updater.dialog.actionRestart') }}
              </button>
            </div>
          </template>

          <template v-else>
            <div class="space-y-2">
              <p class="text-xs font-semibold uppercase tracking-[0.24em] text-rose-500">
                {{ t('about.title') }}
              </p>
              <h2 class="flex items-center gap-2 text-2xl font-semibold text-slate-950">
                <AlertTriangle class="h-5 w-5 text-rose-500" />
                {{
                  dialogState === 'verify_failed'
                    ? t('updater.dialog.titleVerifyFail')
                    : t('updater.dialog.titleError')
                }}
              </h2>
              <p class="text-sm leading-6 text-slate-600">
                {{
                  dialogState === 'verify_failed'
                    ? t('updater.dialog.verifyHint')
                    : (dialogError || t('updater.toast.networkFail', { detail: 'unknown' }))
                }}
              </p>
            </div>

            <div v-if="dialogError" class="rounded-2xl border border-rose-200 bg-rose-50/80 p-4 text-sm text-rose-700">
              {{ dialogError }}
            </div>

            <div class="flex justify-end gap-3">
              <button
                type="button"
                class="rounded-full border border-slate-200 px-4 py-2 text-sm font-medium text-slate-700 transition hover:bg-slate-50"
                @click="closeDialog"
              >
                {{ t('updater.dialog.actionClose') }}
              </button>
              <button
                v-if="state?.has_update"
                type="button"
                class="inline-flex items-center gap-2 rounded-full bg-sky-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-sky-700"
                @click="startDownload"
              >
                <RefreshCw class="h-4 w-4" />
                {{ t('updater.dialog.actionRetry') }}
              </button>
            </div>
          </template>
        </div>
      </div>
    </div>
  </transition>
</template>
