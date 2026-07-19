<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  AlertTriangle,
  CheckCircle2,
  Contrast,
  LoaderCircle,
  MonitorCog,
  RefreshCw,
  SunMedium,
} from 'lucide-vue-next';

import { pushToast } from '@/composables/useToast';
import {
  monitorControlApi,
  type DisplayControlMonitor,
  type MonitorControlFeature,
} from '@/lib/tauri';

defineOptions({ name: 'DisplayControlPage' });

const { t } = useI18n();

const monitors = ref<DisplayControlMonitor[]>([]);
const selectedMonitorId = ref<string | null>(null);
const loading = ref(false);
const settingFeature = ref<MonitorControlFeature | null>(null);
const loadError = ref<string | null>(null);
const featureError = ref<string | null>(null);
const lastUpdatedAt = ref<number | null>(null);
const brightnessDraft = ref(0);
const contrastDraft = ref(0);

const selectedMonitor = computed(() =>
  monitors.value.find((monitor) => monitor.id === selectedMonitorId.value) ?? null,
);

const hasSupportedControl = computed(() =>
  Boolean(selectedMonitor.value?.brightness_supported || selectedMonitor.value?.contrast_supported),
);

const isBusy = computed(() => loading.value || settingFeature.value !== null);

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'string' && error) return error;
  return t('displayControl.errors.generic');
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}

function monitorMeta(monitor: DisplayControlMonitor): string {
  const backend = monitor.backend || t('displayControl.monitor.unknownBackend');
  return `${backend} / ${monitor.is_internal ? t('displayControl.monitor.internal') : t('displayControl.monitor.external')}`;
}

function setSelectedMonitor(id: string) {
  selectedMonitorId.value = id;
  featureError.value = null;
}

function syncDrafts(monitor: DisplayControlMonitor | null) {
  brightnessDraft.value = monitor?.brightness ?? monitor?.brightness_min ?? 0;
  contrastDraft.value = monitor?.contrast ?? monitor?.contrast_min ?? 0;
}

watch(selectedMonitor, syncDrafts, { immediate: true });

async function loadMonitors() {
  loading.value = true;
  loadError.value = null;
  featureError.value = null;

  try {
    const nextMonitors = await monitorControlApi.listMonitors();
    monitors.value = nextMonitors;

    const selectedStillExists = nextMonitors.some((monitor) => monitor.id === selectedMonitorId.value);
    if (!selectedStillExists) {
      const primary = nextMonitors.find((monitor) => monitor.is_primary);
      selectedMonitorId.value = primary?.id ?? nextMonitors[0]?.id ?? null;
    }
    lastUpdatedAt.value = Date.now();
  } catch (error) {
    loadError.value = errorMessage(error);
    monitors.value = [];
    selectedMonitorId.value = null;
  } finally {
    loading.value = false;
  }
}

function featureRange(feature: MonitorControlFeature) {
  const monitor = selectedMonitor.value;
  if (!monitor) return { min: 0, max: 100 };
  return feature === 'brightness'
    ? { min: monitor.brightness_min, max: monitor.brightness_max }
    : { min: monitor.contrast_min, max: monitor.contrast_max };
}

function featureSupported(feature: MonitorControlFeature): boolean {
  const monitor = selectedMonitor.value;
  return feature === 'brightness'
    ? Boolean(monitor?.brightness_supported)
    : Boolean(monitor?.contrast_supported);
}

async function applyFeature(feature: MonitorControlFeature, rawValue: number) {
  const monitor = selectedMonitor.value;
  if (!monitor || !featureSupported(feature) || settingFeature.value !== null) return;

  const range = featureRange(feature);
  const value = clamp(Number(rawValue), range.min, range.max);
  if (feature === 'brightness') brightnessDraft.value = value;
  else contrastDraft.value = value;

  settingFeature.value = feature;
  featureError.value = null;

  try {
    await monitorControlApi.setFeature({
      monitor_id: monitor.id,
      feature,
      value,
    });

    const current = monitors.value.find((entry) => entry.id === monitor.id);
    if (current) {
      if (feature === 'brightness') current.brightness = value;
      else current.contrast = value;
    }
    pushToast(t('displayControl.toast.updated'), 'success');
  } catch (error) {
    featureError.value = errorMessage(error);
    pushToast(t('displayControl.toast.updateFailed'), 'error');
    syncDrafts(monitor);
  } finally {
    settingFeature.value = null;
  }
}

function rangeMin(feature: MonitorControlFeature): number {
  return featureRange(feature).min;
}

function rangeMax(feature: MonitorControlFeature): number {
  return featureRange(feature).max;
}

onMounted(() => {
  void loadMonitors();
});
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-gradient-to-br from-slate-50 via-slate-50 to-sky-50/70">
    <div class="mx-auto flex w-full max-w-6xl flex-col gap-5 px-6 py-6 pb-10">
      <header class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div class="flex items-start gap-3">
          <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl bg-gradient-to-br from-sky-500 to-blue-600 text-white shadow-lg shadow-sky-500/20">
            <MonitorCog class="h-5 w-5" aria-hidden="true" />
          </div>
          <div>
            <h1 class="text-2xl font-bold tracking-tight text-slate-950">{{ t('displayControl.title') }}</h1>
            <p class="mt-1 max-w-2xl text-sm leading-6 text-slate-500">{{ t('displayControl.description') }}</p>
          </div>
        </div>

        <button
          type="button"
          class="inline-flex min-h-11 items-center justify-center gap-2 rounded-xl border border-slate-200 bg-white px-4 py-2 text-sm font-semibold text-slate-700 shadow-sm transition-colors hover:border-sky-300 hover:bg-sky-50 hover:text-sky-700 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/40"
          :disabled="isBusy"
          :aria-label="t('displayControl.actions.refresh')"
          @click="loadMonitors"
        >
          <LoaderCircle v-if="loading" class="h-4 w-4 animate-spin" aria-hidden="true" />
          <RefreshCw v-else class="h-4 w-4" aria-hidden="true" />
          <span>{{ t('displayControl.actions.refresh') }}</span>
        </button>
      </header>

      <div
        v-if="loadError"
        class="flex items-start gap-3 rounded-2xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800"
        role="alert"
      >
        <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
        <div class="min-w-0">
          <p class="font-semibold">{{ t('displayControl.errors.loadTitle') }}</p>
          <p class="mt-1 break-words text-rose-700">{{ loadError }}</p>
        </div>
      </div>

      <div
        v-if="loading && monitors.length === 0"
        class="flex min-h-[260px] items-center justify-center rounded-2xl border border-slate-200 bg-white px-6 py-12 shadow-sm"
        role="status"
        aria-live="polite"
      >
        <div class="flex items-center gap-3 text-sm font-medium text-slate-600">
          <LoaderCircle class="h-5 w-5 animate-spin text-sky-500" aria-hidden="true" />
          <span>{{ t('displayControl.loading') }}</span>
        </div>
      </div>

      <div
        v-else-if="monitors.length === 0"
        class="flex min-h-[260px] flex-col items-center justify-center rounded-2xl border border-dashed border-slate-300 bg-white px-6 py-12 text-center shadow-sm"
      >
        <MonitorCog class="h-9 w-9 text-slate-300" aria-hidden="true" />
        <h2 class="mt-4 text-base font-semibold text-slate-800">{{ t('displayControl.empty.title') }}</h2>
        <p class="mt-2 max-w-md text-sm leading-6 text-slate-500">{{ t('displayControl.empty.description') }}</p>
        <button
          type="button"
          class="mt-5 inline-flex min-h-11 items-center gap-2 rounded-xl bg-sky-600 px-4 py-2 text-sm font-semibold text-white transition-colors hover:bg-sky-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50 focus-visible:ring-offset-2"
          @click="loadMonitors"
        >
          <RefreshCw class="h-4 w-4" aria-hidden="true" />
          <span>{{ t('displayControl.actions.tryAgain') }}</span>
        </button>
      </div>

      <div v-else class="grid gap-5 lg:grid-cols-[minmax(250px,0.8fr)_minmax(0,1.5fr)]">
        <section class="rounded-2xl border border-slate-200 bg-white p-4 shadow-sm" aria-labelledby="display-control-list-title">
          <div class="flex items-center justify-between gap-3 px-1 pb-3">
            <div>
              <h2 id="display-control-list-title" class="text-sm font-bold text-slate-900">{{ t('displayControl.monitors.title') }}</h2>
              <p class="mt-1 text-xs text-slate-500">{{ t('displayControl.monitors.count', { count: monitors.length }) }}</p>
            </div>
            <span class="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-xs font-semibold text-slate-500">
              {{ lastUpdatedAt ? t('displayControl.monitors.detected') : t('displayControl.monitors.detecting') }}
            </span>
          </div>

          <div class="space-y-2" role="listbox" :aria-label="t('displayControl.monitors.title')">
            <button
              v-for="monitor in monitors"
              :key="monitor.id"
              type="button"
              role="option"
              :aria-selected="selectedMonitorId === monitor.id"
              class="w-full rounded-xl border px-3 py-3 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/40"
              :class="selectedMonitorId === monitor.id
                ? 'border-sky-300 bg-sky-50 shadow-sm'
                : 'border-slate-200 bg-white hover:border-sky-200 hover:bg-slate-50'"
              @click="setSelectedMonitor(monitor.id)"
            >
              <div class="flex items-start gap-3">
                <span
                  class="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg"
                  :class="selectedMonitorId === monitor.id ? 'bg-sky-600 text-white' : 'bg-slate-100 text-slate-500'"
                >
                  <MonitorCog class="h-4 w-4" aria-hidden="true" />
                </span>
                <span class="min-w-0 flex-1">
                  <span class="flex items-center gap-2">
                    <span class="truncate text-sm font-semibold text-slate-900">{{ monitor.name }}</span>
                    <span v-if="monitor.is_primary" class="shrink-0 rounded-full border border-sky-200 bg-sky-100 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-sky-700">
                      {{ t('displayControl.monitor.primary') }}
                    </span>
                  </span>
                  <span class="mt-1 block truncate text-xs text-slate-500">{{ monitorMeta(monitor) }}</span>
                  <span class="mt-2 flex items-center gap-2 text-[11px] font-medium text-slate-500">
                    <span :class="monitor.brightness_supported ? 'text-amber-600' : 'text-slate-400'">{{ t('displayControl.controls.brightnessShort') }}</span>
                    <span aria-hidden="true">/</span>
                    <span :class="monitor.contrast_supported ? 'text-indigo-600' : 'text-slate-400'">{{ t('displayControl.controls.contrastShort') }}</span>
                  </span>
                </span>
              </div>
            </button>
          </div>
        </section>

        <section
          v-if="selectedMonitor"
          class="rounded-2xl border border-slate-200 bg-white p-5 shadow-sm sm:p-6"
          aria-labelledby="display-control-settings-title"
        >
          <div class="flex flex-col gap-3 border-b border-slate-100 pb-5 sm:flex-row sm:items-start sm:justify-between">
            <div class="min-w-0">
              <div class="flex items-center gap-2">
                <h2 id="display-control-settings-title" class="truncate text-lg font-bold text-slate-950">{{ selectedMonitor.name }}</h2>
                <CheckCircle2 v-if="hasSupportedControl" class="h-4 w-4 shrink-0 text-emerald-500" :title="t('displayControl.monitor.available')" aria-hidden="true" />
              </div>
              <p class="mt-1 break-all text-xs text-slate-500">{{ monitorMeta(selectedMonitor) }}</p>
              <p v-if="selectedMonitor.id" class="mt-1 break-all font-mono text-[11px] text-slate-400">{{ selectedMonitor.id }}</p>
            </div>
            <span class="inline-flex w-fit shrink-0 rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-xs font-semibold text-slate-600">
              {{ selectedMonitor.is_primary ? t('displayControl.monitor.primary') : t('displayControl.monitor.secondary') }}
            </span>
          </div>

          <div
            v-if="!hasSupportedControl"
            class="mt-5 flex items-start gap-3 rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800"
            role="status"
          >
            <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
            <span>{{ t('displayControl.unsupportedMonitor') }}</span>
          </div>

          <div v-if="featureError" class="mt-5 flex items-start gap-3 rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800" role="alert">
            <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
            <span class="break-words">{{ featureError }}</span>
          </div>

          <div class="mt-6 space-y-7">
            <div class="space-y-3">
              <div class="flex items-start justify-between gap-4">
                <div class="flex items-start gap-3">
                  <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-amber-100 text-amber-700">
                    <SunMedium class="h-4 w-4" aria-hidden="true" />
                  </span>
                  <div>
                    <label for="display-control-brightness" class="text-sm font-bold text-slate-900">{{ t('displayControl.controls.brightness') }}</label>
                    <p class="mt-1 text-xs leading-5 text-slate-500">{{ t('displayControl.controls.brightnessHint') }}</p>
                  </div>
                </div>
                <output for="display-control-brightness" class="shrink-0 rounded-lg bg-amber-50 px-2.5 py-1 text-sm font-bold tabular-nums text-amber-700">
                  {{ selectedMonitor.brightness_supported ? brightnessDraft : t('displayControl.controls.unsupported') }}
                </output>
              </div>

              <input
                id="display-control-brightness"
                v-model.number="brightnessDraft"
                type="range"
                class="h-2 w-full cursor-pointer accent-amber-500 disabled:cursor-not-allowed disabled:opacity-40"
                :min="rangeMin('brightness')"
                :max="rangeMax('brightness')"
                step="1"
                :disabled="!selectedMonitor.brightness_supported || isBusy"
                :aria-valuetext="selectedMonitor.brightness_supported ? `${brightnessDraft}` : t('displayControl.controls.unsupported')"
                @change="applyFeature('brightness', brightnessDraft)"
              />
              <div class="flex justify-between text-[11px] font-medium tabular-nums text-slate-400">
                <span>{{ rangeMin('brightness') }}</span>
                <span>{{ t('displayControl.controls.rangeLabel') }}</span>
                <span>{{ rangeMax('brightness') }}</span>
              </div>
              <p v-if="settingFeature === 'brightness'" class="flex items-center gap-1.5 text-xs font-medium text-sky-600" role="status" aria-live="polite">
                <LoaderCircle class="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                {{ t('displayControl.controls.applying') }}
              </p>
            </div>

            <div class="space-y-3">
              <div class="flex items-start justify-between gap-4">
                <div class="flex items-start gap-3">
                  <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-indigo-100 text-indigo-700">
                    <Contrast class="h-4 w-4" aria-hidden="true" />
                  </span>
                  <div>
                    <label for="display-control-contrast" class="text-sm font-bold text-slate-900">{{ t('displayControl.controls.contrast') }}</label>
                    <p class="mt-1 text-xs leading-5 text-slate-500">{{ t('displayControl.controls.contrastHint') }}</p>
                  </div>
                </div>
                <output for="display-control-contrast" class="shrink-0 rounded-lg bg-indigo-50 px-2.5 py-1 text-sm font-bold tabular-nums text-indigo-700">
                  {{ selectedMonitor.contrast_supported ? contrastDraft : t('displayControl.controls.unsupported') }}
                </output>
              </div>

              <input
                id="display-control-contrast"
                v-model.number="contrastDraft"
                type="range"
                class="h-2 w-full cursor-pointer accent-indigo-500 disabled:cursor-not-allowed disabled:opacity-40"
                :min="rangeMin('contrast')"
                :max="rangeMax('contrast')"
                step="1"
                :disabled="!selectedMonitor.contrast_supported || isBusy"
                :aria-valuetext="selectedMonitor.contrast_supported ? `${contrastDraft}` : t('displayControl.controls.unsupported')"
                @change="applyFeature('contrast', contrastDraft)"
              />
              <div class="flex justify-between text-[11px] font-medium tabular-nums text-slate-400">
                <span>{{ rangeMin('contrast') }}</span>
                <span>{{ t('displayControl.controls.rangeLabel') }}</span>
                <span>{{ rangeMax('contrast') }}</span>
              </div>
              <p v-if="settingFeature === 'contrast'" class="flex items-center gap-1.5 text-xs font-medium text-sky-600" role="status" aria-live="polite">
                <LoaderCircle class="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
                {{ t('displayControl.controls.applying') }}
              </p>
            </div>
          </div>

          <p class="mt-7 border-t border-slate-100 pt-4 text-xs leading-5 text-slate-500">
            {{ t('displayControl.footerHint') }}
          </p>
        </section>
      </div>
    </div>
  </div>
</template>
