<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import {
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  Globe,
  Minus,
  Plus,
  RefreshCw,
  Sparkles,
  ShieldCheck,
} from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';

import { updaterApi } from '@/lib/tauri';
import { addLog } from '@/lib/store';
import { useUpdater } from '@/composables/useUpdater';
import { useToast } from '@/composables/useToast';
import { compareVersionsAsc, formatReleaseDate, isCurrentVersion } from './about/version';

defineOptions({ name: 'AboutPage' });

const router = useRouter();
const { t } = useI18n();
const { state, dialogOpen, dialogState, dialogError } = useUpdater();
const { pushToast } = useToast();

const expandedVersion = ref<string | null>(null);
const showLatestChangelog = ref(false);
const isChecking = ref(false);
const isTesting = ref(false);

// Build-time injected release date (see `vite.config.ts` `define`).
// Acts as the structured fallback when the manifest does not yet list the
// currently running version. Falling back to an empty string allows
// `formatReleaseDate` to render an empty value cleanly.
const fallbackReleaseDate = typeof __APP_RELEASE_DATE__ === 'string' ? __APP_RELEASE_DATE__ : '';

const sortedVersions = computed(() => {
  const versions = [...(state.value?.manifest?.versions ?? [])];
  versions.sort((left, right) => compareVersionsAsc(right, left));
  return versions;
});

const latestEntry = computed(() => {
  const manifest = state.value?.manifest;
  if (!manifest) {
    return null;
  }
  return manifest.versions.find((entry) => entry.version === manifest.latest) ?? manifest.versions[0] ?? null;
});
const latestChangelogTotal = computed(() => latestEntry.value?.changelog.length ?? 0);
const latestPreviewChangelog = computed(() => latestEntry.value?.changelog.slice(0, 3) ?? []);
const latestHiddenChangelogCount = computed(() =>
  Math.max(0, latestChangelogTotal.value - latestPreviewChangelog.value.length),
);
const visibleLatestChangelog = computed(() =>
  showLatestChangelog.value ? (latestEntry.value?.changelog ?? []) : latestPreviewChangelog.value,
);

const currentEntry = computed(() => {
  const currentVersion = state.value?.current ?? '';
  return sortedVersions.value.find((entry) => isCurrentVersion(entry.version, currentVersion)) ?? null;
});

const currentReleaseDate = computed(() => currentEntry.value?.released_at ?? fallbackReleaseDate);

watch(
  () => state.value?.current,
  (current) => {
    if (!expandedVersion.value && current) {
      expandedVersion.value = current;
    }
  },
  { immediate: true },
);

watch(
  () => latestEntry.value?.version,
  () => {
    showLatestChangelog.value = false;
  },
);

async function checkNow() {
  isChecking.value = true;
  try {
    const result = await updaterApi.check();
    if (result.has_update) {
      // The persistent in-page banner (right column) acts as the passive
      // notice; the dialog is the active prompt. No toast — avoid duplicating
      // the same announcement across three surfaces.
      dialogState.value = 'found';
      dialogError.value = null;
      dialogOpen.value = true;
      return;
    }

    pushToast(t('updater.toast.upToDate'), 'info');
  } catch (error) {
    const message = String(error);
    pushToast(message, 'error', { ttlMs: 4800 });
    addLog(`[updater] ${message}`, 'error');
  } finally {
    isChecking.value = false;
  }
}

async function testConnection() {
  isTesting.value = true;
  try {
    const result = await updaterApi.testServer();
    if (result.ok) {
      pushToast(t('updater.toast.testOk'), 'success');
    } else {
      pushToast(
        t('updater.toast.testFail', { detail: result.error ?? result.status ?? 'unknown' }),
        'error',
        { ttlMs: 4800 },
      );
    }
  } catch (error) {
    pushToast(t('updater.toast.testFail', { detail: String(error) }), 'error', { ttlMs: 4800 });
  } finally {
    isTesting.value = false;
  }
}

function openUpgradeDialog() {
  dialogState.value = state.value?.pending_update ? 'resume' : 'found';
  dialogError.value = null;
  dialogOpen.value = true;
}

function toggleExpanded(version: string) {
  expandedVersion.value = expandedVersion.value === version ? null : version;
}

function toggleLatestChangelog() {
  showLatestChangelog.value = !showLatestChangelog.value;
}

// Vue transition hooks for the changelog accordion. We measure scrollHeight
// at runtime so each entry expands to its own intrinsic height. The
// `prefers-reduced-motion` rule in <style> drops the height tween and falls
// back to opacity-only for users who request reduced motion.
function onExpandEnter(el: Element) {
  const target = el as HTMLElement;
  target.style.height = '0px';
  // Force a reflow so the transition picks up the change from 0.
  void target.offsetHeight;
  target.style.height = `${target.scrollHeight}px`;
}

function onExpandAfterEnter(el: Element) {
  (el as HTMLElement).style.height = '';
}

function onExpandLeave(el: Element) {
  const target = el as HTMLElement;
  target.style.height = `${target.scrollHeight}px`;
  void target.offsetHeight;
  target.style.height = '0px';
}
</script>

<template>
  <div class="h-full overflow-y-auto bg-[radial-gradient(circle_at_top,_rgba(14,165,233,0.12),_transparent_45%),linear-gradient(180deg,_#f8fbff_0%,_#eef4fb_100%)]">
    <div class="mx-auto max-w-5xl space-y-6 px-6 py-6">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <button
          type="button"
          class="inline-flex items-center gap-2 rounded-full border border-slate-200 bg-white/85 px-4 py-2 text-sm font-medium text-slate-700 shadow-sm transition hover:bg-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-white"
          @click="router.back()"
        >
          <ArrowLeft class="h-4 w-4" />
          {{ t('about.back') }}
        </button>
        <div
          v-if="state?.debug_build"
          class="rounded-full border border-amber-200 bg-amber-50 px-4 py-2 text-sm font-medium text-amber-700"
        >
          {{ t('about.devModeBadge') }}
        </div>
      </div>

      <section class="overflow-hidden rounded-[30px] border border-slate-200 bg-white shadow-[0_24px_80px_rgba(15,23,42,0.12)]">
        <div class="space-y-6 px-6 py-7">
          <div class="flex flex-wrap items-start justify-between gap-5">
            <div class="flex items-start gap-4">
              <div class="flex h-14 w-14 shrink-0 items-center justify-center rounded-2xl bg-gradient-to-br from-sky-500 to-indigo-600 text-white shadow-lg shadow-sky-200">
                <ShieldCheck class="h-7 w-7" />
              </div>
              <div class="space-y-1.5">
                <p class="text-xs font-semibold uppercase tracking-[0.24em] text-sky-500">
                  File Sync Tool
                </p>
                <h1 class="text-3xl font-semibold tracking-tight text-slate-950">
                  {{ t('about.title') }}
                </h1>
              </div>
            </div>

            <div class="flex flex-wrap items-center gap-3">
              <button
                type="button"
                class="inline-flex items-center gap-2 rounded-full border border-slate-200 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-white"
                :disabled="isTesting || !state?.server_url"
                @click="testConnection"
              >
                <RefreshCw class="h-4 w-4" :class="isTesting ? 'animate-spin' : ''" aria-hidden="true" />
                {{ isTesting ? t('about.testing') : t('about.testConnection') }}
              </button>
              <button
                type="button"
                class="inline-flex items-center gap-2 rounded-full bg-sky-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-sky-700 disabled:cursor-not-allowed disabled:opacity-60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-white"
                :disabled="isChecking || state?.debug_build || !state?.server_url"
                @click="checkNow"
              >
                <RefreshCw class="h-4 w-4" :class="isChecking ? 'animate-spin' : ''" aria-hidden="true" />
                {{ isChecking ? t('about.checking') : t('about.checkNow') }}
              </button>
            </div>
          </div>

          <div class="rounded-2xl border border-slate-200 bg-slate-50/80 px-4 py-3">
            <div class="flex flex-wrap items-center gap-3">
              <div class="flex items-center gap-2 text-sm font-medium text-slate-700">
                <Globe class="h-4 w-4 text-slate-400" aria-hidden="true" />
                <span>{{ t('about.serverLabel') }}</span>
              </div>
              <div class="min-w-0 flex-1 break-all text-sm text-slate-500">
                {{ state?.server_url || t('about.serverEmpty') }}
              </div>
            </div>
          </div>

          <div class="grid grid-cols-1 gap-3 lg:grid-cols-2">
            <div class="rounded-2xl border border-slate-200 bg-white px-5 py-4">
              <div class="flex items-center justify-between gap-3">
                <p class="text-xs font-semibold uppercase tracking-[0.18em] text-slate-400">
                  {{ t('about.currentVersionLabel') }}
                </p>
                <span class="rounded-full bg-slate-100 px-2.5 py-0.5 text-[11px] font-semibold uppercase tracking-wider text-slate-500">
                  {{ t('about.currentTag') }}
                </span>
              </div>
              <p class="mt-3 text-3xl font-semibold tracking-tight text-slate-950">
                {{ state?.current ?? '1.1.2' }}
              </p>
              <p class="mt-1 text-sm text-slate-500">
                {{ formatReleaseDate(currentReleaseDate) || '—' }}
              </p>
            </div>

            <div
              v-if="latestEntry && state?.has_update"
              class="relative overflow-hidden rounded-2xl bg-slate-950 px-5 py-4 text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.06)]"
            >
              <div class="flex items-center justify-between gap-3">
                <p class="inline-flex items-center gap-1.5 text-xs font-semibold uppercase tracking-[0.18em] text-sky-300/85">
                  <Sparkles class="h-3.5 w-3.5" aria-hidden="true" />
                  {{ t('about.newVersionLabel') }}
                </p>
                <span class="rounded-full border border-sky-300/20 bg-sky-300/10 px-2.5 py-0.5 text-[11px] font-semibold text-sky-100">
                  {{ latestChangelogTotal }} {{ t('about.changelogCount') }}
                </span>
              </div>
              <p class="mt-3 text-3xl font-semibold tracking-tight text-white">
                {{ latestEntry.version }}
              </p>
              <div class="mt-1 flex flex-wrap items-center justify-between gap-3">
                <p class="text-sm text-sky-100/75">
                  {{ formatReleaseDate(latestEntry.released_at) }}
                </p>
                <button
                  type="button"
                  class="inline-flex items-center gap-2 rounded-full bg-white px-4 py-1.5 text-sm font-semibold text-slate-900 transition hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-300 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-950"
                  @click="openUpgradeDialog"
                >
                  {{ t('about.upgradeCta') }}
                </button>
              </div>
            </div>

            <div
              v-else
              class="flex items-center rounded-2xl border border-slate-200 bg-slate-50/80 px-5 py-4 text-sm text-slate-500"
            >
              {{
                !state?.server_url
                  ? t('about.serverNotConfigured')
                  : t('about.noUpdateAvailable')
              }}
            </div>
          </div>
        </div>
      </section>

      <section
        v-if="latestEntry && state?.has_update"
        class="rounded-[30px] border border-slate-200 bg-white px-6 py-6 shadow-[0_18px_60px_rgba(15,23,42,0.08)]"
      >
        <div class="flex flex-wrap items-end justify-between gap-3">
          <div>
            <p class="text-xs font-semibold uppercase tracking-[0.24em] text-slate-400">
              {{ t('about.changelogSummary') }}
            </p>
            <h2 class="mt-2 text-2xl font-semibold tracking-tight text-slate-950">
              {{ t('about.changelogFor', { version: latestEntry.version }) }}
            </h2>
          </div>
          <p class="text-sm text-slate-500">
            {{ latestChangelogTotal }} {{ t('about.changelogCount') }} · {{ formatReleaseDate(latestEntry.released_at) }}
          </p>
        </div>

        <div id="changelog-latest" class="relative mt-5">
          <ul
            v-if="visibleLatestChangelog.length > 0"
            class="list-disc space-y-2.5 pl-6 text-[15px] leading-7 text-slate-700 marker:text-sky-500"
          >
            <li v-for="(line, index) in visibleLatestChangelog" :key="index">{{ line }}</li>
          </ul>
          <div v-else class="text-sm leading-6 text-slate-400">
            {{ t('about.changelogEmpty') }}
          </div>

          <div
            v-if="!showLatestChangelog && latestHiddenChangelogCount > 0"
            class="pointer-events-none absolute inset-x-0 bottom-0 h-16 bg-gradient-to-t from-white via-white/85 to-transparent"
            aria-hidden="true"
          />
        </div>

        <div v-if="latestHiddenChangelogCount > 0" class="mt-4 flex justify-center">
          <button
            type="button"
            class="inline-flex items-center gap-2 rounded-full border border-slate-200 bg-white px-5 py-2 text-sm font-semibold text-slate-700 shadow-sm transition hover:border-sky-300 hover:bg-sky-50 hover:text-sky-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-white"
            :aria-expanded="showLatestChangelog"
            aria-controls="changelog-latest"
            @click="toggleLatestChangelog"
          >
            <span>
              {{
                showLatestChangelog
                  ? t('about.hideChangelog')
                  : t('about.showAllChangelog', { count: latestHiddenChangelogCount })
              }}
            </span>
            <ChevronUp v-if="showLatestChangelog" class="h-4 w-4" aria-hidden="true" />
            <ChevronDown v-else class="h-4 w-4" aria-hidden="true" />
          </button>
        </div>
      </section>

      <section class="rounded-[30px] border border-slate-200 bg-white px-6 py-6 shadow-[0_18px_60px_rgba(15,23,42,0.08)]">
        <div class="flex items-center justify-between gap-3">
          <div>
            <p class="text-xs font-semibold uppercase tracking-[0.24em] text-slate-400">
              {{ t('about.history') }}
            </p>
            <h2 class="mt-2 text-2xl font-semibold tracking-tight text-slate-950">
              {{ t('about.history') }}
            </h2>
          </div>
        </div>

        <div v-if="sortedVersions.length > 0" class="mt-6 space-y-3">
          <article
            v-for="entry in sortedVersions"
            :key="entry.version"
            class="overflow-hidden rounded-3xl border border-slate-200 bg-slate-50/75 transition-colors duration-150 hover:bg-slate-50"
          >
            <button
              type="button"
              class="flex w-full items-center justify-between gap-4 px-5 py-4 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-white"
              :aria-expanded="expandedVersion === entry.version"
              :aria-controls="`changelog-${entry.version}`"
              @click="toggleExpanded(entry.version)"
            >
              <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                  <span class="text-lg font-semibold text-slate-900">{{ entry.version }}</span>
                  <span
                    v-if="isCurrentVersion(entry.version, state?.current ?? '')"
                    aria-current="true"
                    class="rounded-full bg-sky-100 px-2.5 py-0.5 text-xs font-semibold text-sky-700"
                  >
                    {{ t('about.currentTag') }}
                  </span>
                </div>
                <div class="mt-1 text-sm text-slate-500">
                  {{ formatReleaseDate(entry.released_at) }}
                </div>
              </div>
              <span class="text-slate-400" aria-hidden="true">
                <Minus v-if="expandedVersion === entry.version" class="h-4 w-4" />
                <Plus v-else class="h-4 w-4" />
              </span>
            </button>

            <transition
              name="changelog-expand"
              @enter="onExpandEnter"
              @after-enter="onExpandAfterEnter"
              @leave="onExpandLeave"
            >
              <div
                v-if="expandedVersion === entry.version"
                :id="`changelog-${entry.version}`"
                class="changelog-panel border-t border-slate-200 bg-white"
              >
                <ul class="list-disc space-y-2 px-5 py-4 pl-10 text-sm leading-6 text-slate-700">
                  <li v-for="(line, index) in entry.changelog" :key="index">{{ line }}</li>
                  <li v-if="entry.changelog.length === 0" class="list-none pl-0 text-slate-400">
                    {{ t('about.changelogEmpty') }}
                  </li>
                </ul>
              </div>
            </transition>
          </article>
        </div>
        <p v-else class="mt-6 text-sm text-slate-500">
          {{ t('about.serverEmpty') }}
        </p>
      </section>
    </div>
  </div>
</template>

<style scoped>
.changelog-panel {
  overflow: hidden;
}
.changelog-expand-enter-active,
.changelog-expand-leave-active {
  transition: height 200ms ease-out, opacity 160ms ease-out;
}
.changelog-expand-enter-from,
.changelog-expand-leave-to {
  opacity: 0;
}
.changelog-expand-enter-to,
.changelog-expand-leave-from {
  opacity: 1;
}
@media (prefers-reduced-motion: reduce) {
  .changelog-expand-enter-active,
  .changelog-expand-leave-active {
    transition: opacity 120ms linear;
  }
  /* Ensure no height tween runs under reduced motion. The hooks still set
     heights, but the transition is opacity-only so the size jumps instantly. */
  .changelog-panel {
    height: auto !important;
  }
}
</style>
