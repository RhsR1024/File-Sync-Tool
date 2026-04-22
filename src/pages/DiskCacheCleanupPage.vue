<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import {
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  HardDrive,
  Loader,
  RefreshCw,
  Server,
  Trash2,
} from 'lucide-vue-next';

import {
  diskCleanupCheckRedis,
  diskCleanupDeleteCache,
  diskCleanupListDisks,
  diskCleanupListServers,
  getConfig,
  saveConfig,
  type AppConfig,
  type DiskInfoItem,
  type DiskServerItem,
} from '../lib/tauri';
import { mergeRecentItems, normalizeRecentItems } from '../lib/recentHistory';

defineOptions({
  name: 'DiskCacheCleanupPage',
});

const { t } = useI18n();

const RECENT_KEY = 'diskCacheCleanup.recentHosts';
const MAX_RECENT_HOSTS = 10;
const TIMEOUT_OPTIONS = [1, 2, 3, 5, 10, 15, 30] as const;

const STATUS_GREEN = new Set([1, 13]);
const STATUS_BLUE = new Set([4, 7, 8, 9, 10, 11, 12, 16, 20]);
const STATUS_AMBER = new Set([5, 14, 19, 21, 22]);
const STATUS_RED = new Set([2, 3, 6, 15, 17, 18, 23]);
const KNOWN_STATUSES = new Set([
  1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
  13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
]);
const KNOWN_USAGES = new Set([1, 2, 3, 4, 5, 255, -1]);

const config = ref<AppConfig | null>(null);
const timeoutSecs = ref(5);
const hostIp = ref('');
const recentHosts = ref<string[]>([]);
const serverList = ref<DiskServerItem[]>([]);
const pickedServerIp = ref('');
const disks = ref<DiskInfoItem[]>([]);
const expandedIds = ref<Set<string>>(new Set());
const presentCacheIds = ref<Set<string>>(new Set());
const cleaningIds = ref<Set<string>>(new Set());
const fetchingServers = ref(false);
const loadingDisks = ref(false);
const batchCleaning = ref(false);
const hasFetchedServers = ref(false);
const redisAvailable = ref(true);
const redisError = ref<string | null>(null);
const errorMessage = ref<string | null>(null);

let serversRequestSeq = 0;
let disksRequestSeq = 0;

const savedSshHosts = computed(() => {
  if (!config.value) return [];

  const unique = new Set<string>();
  for (const server of config.value.servers ?? []) {
    const host = server.host?.trim();
    if (!host) continue;
    if (recentHosts.value.includes(host)) continue;
    unique.add(host);
  }
  return Array.from(unique);
});

const selectedServer = computed(
  () => serverList.value.find((item) => item.serverIp === pickedServerIp.value) ?? null,
);

const cleanableIds = computed(() =>
  disks.value
    .filter((disk) => presentCacheIds.value.has(disk.storageId))
    .map((disk) => disk.storageId),
);

const canFetchServers = computed(
  () => hostIp.value.trim().length > 0 && !fetchingServers.value,
);

function formatError(error: unknown) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

async function loadRecentHosts() {
  try {
    const saved = await invoke<unknown>('load_kv', { key: RECENT_KEY });
    recentHosts.value = normalizeRecentItems(Array.isArray(saved) ? saved : [], MAX_RECENT_HOSTS);
  } catch {
    recentHosts.value = [];
  }
}

async function persistRecentHosts(nextHosts: string[]) {
  recentHosts.value = nextHosts;
  try {
    await invoke('save_kv', {
      key: RECENT_KEY,
      value: nextHosts,
    });
  } catch {
    /* Best-effort persistence only. */
  }
}

async function pushRecentHost(host: string) {
  const normalized = host.trim();
  if (!normalized) return;

  await persistRecentHosts(mergeRecentItems(recentHosts.value, normalized, MAX_RECENT_HOSTS));
}

function resetDiskState() {
  disks.value = [];
  expandedIds.value = new Set();
  presentCacheIds.value = new Set();
  cleaningIds.value = new Set();
  redisAvailable.value = true;
  redisError.value = null;
}

async function fetchServers() {
  const host = hostIp.value.trim();
  if (!host) {
    errorMessage.value = t('diskCacheCleanup.errors.hostEmpty');
    return;
  }

  const currentSeq = ++serversRequestSeq;
  fetchingServers.value = true;
  hasFetchedServers.value = true;
  errorMessage.value = null;
  pickedServerIp.value = '';
  serverList.value = [];
  resetDiskState();

  try {
    const list = await diskCleanupListServers(host, timeoutSecs.value);
    if (currentSeq !== serversRequestSeq) return;

    serverList.value = list;
    await pushRecentHost(host);

    if (list.length > 0) {
      pickedServerIp.value = list[0].serverIp;
      await fetchDisksFor(list[0].serverIp);
    }
  } catch (error) {
    if (currentSeq !== serversRequestSeq) return;
    errorMessage.value = t('diskCacheCleanup.errors.http', {
      reason: formatError(error),
    });
  } finally {
    if (currentSeq === serversRequestSeq) {
      fetchingServers.value = false;
    }
  }
}

async function fetchDisksFor(serverIp: string) {
  const host = hostIp.value.trim();
  if (!host || !serverIp) return;

  const currentSeq = ++disksRequestSeq;
  loadingDisks.value = true;
  errorMessage.value = null;
  resetDiskState();

  try {
    const list = await diskCleanupListDisks(host, serverIp, timeoutSecs.value);
    if (currentSeq !== disksRequestSeq) return;

    disks.value = list;
    if (list.length === 0) return;

    try {
      const check = await diskCleanupCheckRedis(
        host,
        list.map((disk) => disk.storageId),
      );
      if (currentSeq !== disksRequestSeq) return;

      redisAvailable.value = check.redis_available;
      redisError.value = check.redis_available
        ? null
        : (check.error ?? t('diskCacheCleanup.cache.unavailable'));
      presentCacheIds.value = new Set(check.present_ids ?? []);
    } catch (error) {
      if (currentSeq !== disksRequestSeq) return;
      redisAvailable.value = false;
      redisError.value = formatError(error);
      presentCacheIds.value = new Set();
    }
  } catch (error) {
    if (currentSeq !== disksRequestSeq) return;
    errorMessage.value = t('diskCacheCleanup.errors.http', {
      reason: formatError(error),
    });
  } finally {
    if (currentSeq === disksRequestSeq) {
      loadingDisks.value = false;
    }
  }
}

async function refreshCurrentServer() {
  if (pickedServerIp.value) {
    await fetchDisksFor(pickedServerIp.value);
    return;
  }

  await fetchServers();
}

async function handleServerChange(serverIp: string) {
  pickedServerIp.value = serverIp;
  await fetchDisksFor(serverIp);
}

function toggleExpanded(storageId: string) {
  const next = new Set(expandedIds.value);
  if (next.has(storageId)) {
    next.delete(storageId);
  } else {
    next.add(storageId);
  }
  expandedIds.value = next;
}

function statusLabelKey(status: number) {
  if (!KNOWN_STATUSES.has(status)) {
    return 'diskCacheCleanup.status.unknown';
  }
  return `diskCacheCleanup.status.${status}`;
}

function usageLabelKey(usage: number) {
  if (!KNOWN_USAGES.has(usage)) {
    return 'diskCacheCleanup.usage.unknown';
  }
  return `diskCacheCleanup.usage.${usage}`;
}

function statusBadgeClass(status: number) {
  if (STATUS_GREEN.has(status)) {
    return 'border-emerald-200 bg-emerald-50 text-emerald-700';
  }
  if (STATUS_BLUE.has(status)) {
    return 'border-sky-200 bg-sky-50 text-sky-700';
  }
  if (STATUS_AMBER.has(status)) {
    return 'border-amber-200 bg-amber-50 text-amber-700';
  }
  if (STATUS_RED.has(status)) {
    return 'border-rose-200 bg-rose-50 text-rose-700';
  }
  return 'border-slate-200 bg-slate-50 text-slate-600';
}

function usageBadgeClass(usage: number) {
  if (usage === 1) return 'border-sky-200 bg-sky-50 text-sky-700';
  if (usage === 2) return 'border-indigo-200 bg-indigo-50 text-indigo-700';
  if (usage === 3) return 'border-violet-200 bg-violet-50 text-violet-700';
  if (usage === 4) return 'border-emerald-200 bg-emerald-50 text-emerald-700';
  if (usage === 5) return 'border-amber-200 bg-amber-50 text-amber-700';
  if (usage === 255) return 'border-slate-200 bg-slate-50 text-slate-600';
  return 'border-slate-200 bg-white text-slate-500';
}

function statusIsBusy(status: number) {
  return STATUS_BLUE.has(status);
}

function formatCapacity(totalCapacity: number) {
  if (!Number.isFinite(totalCapacity) || totalCapacity <= 0) {
    return '--';
  }
  if (totalCapacity >= 1024) {
    const value = totalCapacity / 1024;
    return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} TB`;
  }
  return `${totalCapacity} GB`;
}

async function cleanOne(storageId: string) {
  if (!redisAvailable.value || cleaningIds.value.has(storageId)) return;

  const next = new Set(cleaningIds.value);
  next.add(storageId);
  cleaningIds.value = next;
  errorMessage.value = null;

  try {
    const result = await diskCleanupDeleteCache(hostIp.value.trim(), [storageId]);
    if (!result.redis_available || result.error) {
      redisAvailable.value = result.redis_available;
      redisError.value = result.error ?? t('diskCacheCleanup.cache.unavailable');
      errorMessage.value = t('diskCacheCleanup.errors.deleteSingle', {
        reason: result.error ?? t('diskCacheCleanup.disabled.redisDown'),
      });
      return;
    }

    if (pickedServerIp.value) {
      await fetchDisksFor(pickedServerIp.value);
    }
  } catch (error) {
    errorMessage.value = t('diskCacheCleanup.errors.deleteSingle', {
      reason: formatError(error),
    });
  } finally {
    const done = new Set(cleaningIds.value);
    done.delete(storageId);
    cleaningIds.value = done;
  }
}

async function cleanAll() {
  if (!redisAvailable.value || batchCleaning.value || cleanableIds.value.length === 0) {
    return;
  }

  const confirmed = window.confirm(
    t('diskCacheCleanup.actions.cleanAllConfirm', {
      count: cleanableIds.value.length,
    }),
  );
  if (!confirmed) return;

  batchCleaning.value = true;
  errorMessage.value = null;

  try {
    const result = await diskCleanupDeleteCache(hostIp.value.trim(), cleanableIds.value);
    if (!result.redis_available || result.error) {
      redisAvailable.value = result.redis_available;
      redisError.value = result.error ?? t('diskCacheCleanup.cache.unavailable');
      errorMessage.value = t('diskCacheCleanup.errors.deleteBatch', {
        reason: result.error ?? t('diskCacheCleanup.disabled.redisDown'),
      });
      return;
    }

    if (pickedServerIp.value) {
      await fetchDisksFor(pickedServerIp.value);
    }
  } catch (error) {
    errorMessage.value = t('diskCacheCleanup.errors.deleteBatch', {
      reason: formatError(error),
    });
  } finally {
    batchCleaning.value = false;
  }
}

async function saveTimeout() {
  if (!config.value) return;

  config.value.disk_cleanup_http_timeout_secs = timeoutSecs.value;
  try {
    await saveConfig(config.value);
  } catch (error) {
    errorMessage.value = t('diskCacheCleanup.errors.http', {
      reason: formatError(error),
    });
  }
}

onMounted(async () => {
  try {
    const loaded = await getConfig();
    config.value = loaded;
    timeoutSecs.value = Number(config.value.disk_cleanup_http_timeout_secs ?? 5);
  } catch (error) {
    errorMessage.value = t('diskCacheCleanup.errors.http', {
      reason: formatError(error),
    });
  }

  await loadRecentHosts();
});
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-[radial-gradient(circle_at_top_left,_rgba(14,165,233,0.14),_transparent_28%),linear-gradient(180deg,_#f8fbff_0%,_#eef4fb_40%,_#f8fafc_100%)]">
    <div class="mx-auto flex w-full max-w-6xl flex-col gap-5 px-6 py-6 pb-10">
      <section class="relative overflow-hidden rounded-[28px] border border-white/70 bg-white/80 px-6 py-6 shadow-[0_18px_60px_rgba(15,23,42,0.08)] backdrop-blur">
        <div class="absolute -left-8 top-0 h-32 w-32 rounded-full bg-sky-100/70 blur-3xl"></div>
        <div class="absolute right-0 top-0 h-40 w-40 rounded-full bg-indigo-100/80 blur-3xl"></div>
        <div class="relative flex flex-col gap-5 lg:flex-row lg:items-end lg:justify-between">
          <div class="space-y-2">
            <div class="flex items-center gap-2">
              <span class="h-1.5 w-1.5 rounded-full bg-sky-500"></span>
              <span class="text-[11px] font-bold uppercase tracking-[0.12em] text-slate-500">Redis Cache Ops</span>
            </div>
            <div class="flex items-start gap-3">
              <div class="flex h-12 w-12 items-center justify-center rounded-2xl bg-gradient-to-br from-sky-500 via-blue-500 to-indigo-600 text-white shadow-lg shadow-sky-500/20">
                <HardDrive class="h-6 w-6" />
              </div>
              <div>
                <h1 class="text-2xl font-bold tracking-tight text-[#0F172A]">
                  {{ t('diskCacheCleanup.title') }}
                </h1>
                <p class="mt-1 max-w-2xl text-sm leading-6 text-[#334155]">
                  {{ t('diskCacheCleanup.description') }}
                </p>
              </div>
            </div>
          </div>

          <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <div class="rounded-2xl border border-slate-200/80 bg-slate-50/80 px-4 py-3">
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.summary.recent') }}
              </div>
              <div class="mt-2 font-mono text-2xl font-bold text-slate-900">{{ recentHosts.length }}</div>
            </div>
            <div class="rounded-2xl border border-slate-200/80 bg-slate-50/80 px-4 py-3">
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.summary.saved') }}
              </div>
              <div class="mt-2 font-mono text-2xl font-bold text-slate-900">{{ savedSshHosts.length }}</div>
            </div>
            <div class="rounded-2xl border border-slate-200/80 bg-slate-50/80 px-4 py-3">
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.summary.servers') }}
              </div>
              <div class="mt-2 font-mono text-2xl font-bold text-slate-900">{{ serverList.length }}</div>
            </div>
            <div class="rounded-2xl border border-slate-200/80 bg-slate-50/80 px-4 py-3">
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.summary.cached') }}
              </div>
              <div class="mt-2 font-mono text-2xl font-bold text-slate-900">{{ cleanableIds.length }}</div>
            </div>
          </div>
        </div>
      </section>

      <section class="rounded-[24px] border border-slate-200/80 bg-white/90 p-5 shadow-[0_14px_40px_rgba(15,23,42,0.06)]">
        <div class="flex flex-col gap-5 xl:flex-row xl:items-start xl:justify-between">
          <div class="flex-1">
            <label class="text-sm font-semibold text-slate-800">{{ t('diskCacheCleanup.hostIp.label') }}</label>
            <div class="mt-2 flex flex-col gap-3 sm:flex-row">
              <input
                v-model="hostIp"
                type="text"
                :placeholder="t('diskCacheCleanup.hostIp.placeholder')"
                class="w-full rounded-2xl border border-slate-300 bg-white px-4 py-3 text-sm font-mono text-slate-900 outline-none transition focus:border-sky-500 focus:ring-4 focus:ring-sky-500/10"
                @keyup.enter="fetchServers"
              >
              <button
                type="button"
                class="inline-flex shrink-0 items-center justify-center gap-2 rounded-2xl bg-[#0369A1] px-5 py-3 text-sm font-semibold text-white shadow-sm transition hover:bg-sky-700 disabled:cursor-not-allowed disabled:bg-slate-300"
                :disabled="!canFetchServers"
                @click="fetchServers"
              >
                <Loader v-if="fetchingServers" class="h-4 w-4 animate-spin" />
                <HardDrive v-else class="h-4 w-4" />
                <span>{{ fetchingServers ? t('diskCacheCleanup.actions.fetching') : t('diskCacheCleanup.actions.fetch') }}</span>
              </button>
            </div>
            <p class="mt-2 text-xs leading-5 text-slate-500">{{ t('diskCacheCleanup.hostIp.hint') }}</p>

            <div v-if="recentHosts.length || savedSshHosts.length" class="mt-4 space-y-3">
              <div v-if="recentHosts.length">
                <p class="mb-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                  {{ t('diskCacheCleanup.hostIp.recentGroup') }}
                </p>
                <div class="flex flex-wrap gap-2">
                  <button
                    v-for="item in recentHosts"
                    :key="`recent-${item}`"
                    type="button"
                    class="rounded-full border border-sky-200 bg-sky-50 px-3 py-1.5 font-mono text-xs text-sky-700 transition hover:border-sky-300 hover:bg-sky-100"
                    @click="hostIp = item"
                  >
                    {{ item }}
                  </button>
                </div>
              </div>

              <div v-if="savedSshHosts.length">
                <p class="mb-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                  {{ t('diskCacheCleanup.hostIp.serversGroup') }}
                </p>
                <div class="flex flex-wrap gap-2">
                  <button
                    v-for="item in savedSshHosts"
                    :key="`saved-${item}`"
                    type="button"
                    class="rounded-full border border-slate-200 bg-slate-50 px-3 py-1.5 font-mono text-xs text-slate-700 transition hover:border-slate-300 hover:bg-slate-100"
                    @click="hostIp = item"
                  >
                    {{ item }}
                  </button>
                </div>
              </div>
            </div>
          </div>

          <div class="grid min-w-[220px] grid-cols-1 gap-3 rounded-2xl border border-slate-200 bg-slate-50/70 p-4">
            <div>
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.timeout.label') }}
              </div>
              <select
                v-model.number="timeoutSecs"
                class="mt-2 w-full rounded-xl border border-slate-300 bg-white px-3 py-2 text-sm text-slate-700 outline-none transition focus:border-sky-500 focus:ring-4 focus:ring-sky-500/10"
                @change="saveTimeout"
              >
                <option
                  v-for="option in TIMEOUT_OPTIONS"
                  :key="option"
                  :value="option"
                >
                  {{ option }} {{ t('settings.seconds') }}
                </option>
              </select>
            </div>

            <div class="rounded-2xl border border-dashed border-slate-200 bg-white/80 px-3 py-3">
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.meta.redisTarget') }}
              </div>
              <div class="mt-2 break-all font-mono text-sm text-slate-700">
                {{ hostIp.trim() ? `${hostIp.trim()}:6379` : '--' }}
              </div>
            </div>
          </div>
        </div>
      </section>

      <section
        v-if="errorMessage"
        class="flex items-start gap-3 rounded-[20px] border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700 shadow-sm"
      >
        <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
        <span>{{ errorMessage }}</span>
      </section>

      <section
        v-if="redisError && !redisAvailable"
        class="flex items-start gap-3 rounded-[20px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800 shadow-sm"
      >
        <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
        <span>{{ t('diskCacheCleanup.errors.redis', { reason: redisError }) }}</span>
      </section>

      <section
        v-if="serverList.length > 0"
        class="rounded-[24px] border border-slate-200/80 bg-white/90 p-5 shadow-[0_14px_40px_rgba(15,23,42,0.06)]"
      >
        <div class="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
          <div class="flex-1">
            <div class="mb-2 flex items-center gap-2">
              <Server class="h-4 w-4 text-slate-400" />
              <label class="text-sm font-semibold text-slate-800">{{ t('diskCacheCleanup.server.pick') }}</label>
            </div>
            <select
              :value="pickedServerIp"
              class="w-full rounded-2xl border border-slate-300 bg-white px-4 py-3 text-sm text-slate-800 outline-none transition focus:border-sky-500 focus:ring-4 focus:ring-sky-500/10"
              @change="handleServerChange(($event.target as HTMLSelectElement).value)"
            >
              <option
                v-for="server in serverList"
                :key="server.serverIp"
                :value="server.serverIp"
              >
                {{ `${server.serverName || server.serverIp} · ${server.serverIp}${server.role ? ` · ${server.role}` : ''}` }}
              </option>
            </select>
          </div>

          <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <div class="rounded-2xl border border-slate-200 bg-slate-50/80 px-4 py-3">
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.server.detailsIp') }}
              </div>
              <div class="mt-2 font-mono text-sm font-semibold text-slate-900">
                {{ selectedServer?.serverIp ?? '--' }}
              </div>
            </div>
            <div class="rounded-2xl border border-slate-200 bg-slate-50/80 px-4 py-3">
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.server.detailsRole') }}
              </div>
              <div class="mt-2 text-sm font-semibold text-slate-900">
                {{ selectedServer?.role || '--' }}
              </div>
            </div>
            <div class="rounded-2xl border border-slate-200 bg-slate-50/80 px-4 py-3">
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.server.detailsSerial') }}
              </div>
              <div class="mt-2 font-mono text-sm font-semibold text-slate-900">
                {{ selectedServer?.serial || '--' }}
              </div>
            </div>
          </div>
        </div>
      </section>

      <section class="rounded-[24px] border border-slate-200/80 bg-white/90 shadow-[0_14px_40px_rgba(15,23,42,0.06)]">
        <div class="flex flex-col gap-4 border-b border-slate-200/80 px-5 py-5 md:flex-row md:items-center md:justify-between">
          <div>
            <h2 class="text-lg font-bold text-slate-900">{{ t('diskCacheCleanup.disks.title') }}</h2>
            <p class="mt-1 text-sm text-slate-500">
              {{
                selectedServer
                  ? t('diskCacheCleanup.disks.summary', {
                      total: disks.length,
                      cached: cleanableIds.length,
                    })
                  : t('diskCacheCleanup.disks.summaryIdle')
              }}
            </p>
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <span
              v-if="pickedServerIp"
              class="rounded-full border border-slate-200 bg-slate-50 px-3 py-1.5 font-mono text-xs text-slate-600"
            >
              {{ pickedServerIp }}
            </span>
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 transition hover:border-slate-300 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50"
              :disabled="fetchingServers || loadingDisks"
              @click="refreshCurrentServer"
            >
              <RefreshCw class="h-4 w-4" :class="{ 'animate-spin': fetchingServers || loadingDisks }" />
              {{ t('diskCacheCleanup.actions.refresh') }}
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-2xl px-4 py-2.5 text-sm font-semibold text-white transition disabled:cursor-not-allowed disabled:bg-slate-300"
              :class="redisAvailable && cleanableIds.length > 0 ? 'bg-rose-500 hover:bg-rose-600' : 'bg-slate-300'"
              :disabled="!redisAvailable || cleanableIds.length === 0 || batchCleaning"
              :title="!redisAvailable ? t('diskCacheCleanup.disabled.redisDown') : undefined"
              @click="cleanAll"
            >
              <Loader v-if="batchCleaning" class="h-4 w-4 animate-spin" />
              <Trash2 v-else class="h-4 w-4" />
              {{ batchCleaning ? t('diskCacheCleanup.actions.cleaningAll') : t('diskCacheCleanup.actions.cleanAll', { count: cleanableIds.length }) }}
            </button>
          </div>
        </div>

        <div class="p-5">
          <div
            v-if="!hasFetchedServers && !fetchingServers"
            class="flex min-h-[260px] flex-col items-center justify-center rounded-[20px] border border-dashed border-slate-200 bg-slate-50/60 px-6 py-8 text-center"
          >
            <div class="flex h-14 w-14 items-center justify-center rounded-2xl bg-sky-50 text-sky-600">
              <HardDrive class="h-7 w-7" />
            </div>
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.disks.emptyIdle') }}</p>
            <p class="mt-2 max-w-md text-sm leading-6 text-slate-500">{{ t('diskCacheCleanup.disks.emptyIdleHint') }}</p>
          </div>

          <div
            v-else-if="fetchingServers || loadingDisks"
            class="flex min-h-[260px] flex-col items-center justify-center rounded-[20px] border border-dashed border-slate-200 bg-slate-50/60 px-6 py-8 text-center"
          >
            <Loader class="h-7 w-7 animate-spin text-sky-600" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.disks.loading') }}</p>
            <p class="mt-2 text-sm text-slate-500">{{ t('diskCacheCleanup.disks.loadingHint') }}</p>
          </div>

          <div
            v-else-if="hasFetchedServers && serverList.length === 0"
            class="flex min-h-[260px] flex-col items-center justify-center rounded-[20px] border border-dashed border-slate-200 bg-slate-50/60 px-6 py-8 text-center"
          >
            <Server class="h-8 w-8 text-slate-400" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.server.empty') }}</p>
            <p class="mt-2 text-sm leading-6 text-slate-500">{{ t('diskCacheCleanup.server.emptyHint') }}</p>
          </div>

          <div
            v-else-if="disks.length === 0"
            class="flex min-h-[260px] flex-col items-center justify-center rounded-[20px] border border-dashed border-slate-200 bg-slate-50/60 px-6 py-8 text-center"
          >
            <HardDrive class="h-8 w-8 text-slate-400" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.disks.empty') }}</p>
            <p class="mt-2 text-sm leading-6 text-slate-500">{{ t('diskCacheCleanup.disks.emptyHint') }}</p>
          </div>

          <div v-else class="overflow-x-auto">
            <table class="min-w-[920px] w-full text-sm">
              <thead>
                <tr class="border-b border-slate-200 bg-slate-50 text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-500">
                  <th class="w-12 px-3 py-3 text-left"></th>
                  <th class="w-20 px-3 py-3 text-left">{{ t('diskCacheCleanup.disks.columns.slot') }}</th>
                  <th class="px-3 py-3 text-left">{{ t('diskCacheCleanup.disks.columns.device') }}</th>
                  <th class="w-28 px-3 py-3 text-right">{{ t('diskCacheCleanup.disks.columns.capacity') }}</th>
                  <th class="w-40 px-3 py-3 text-left">{{ t('diskCacheCleanup.disks.columns.usage') }}</th>
                  <th class="w-44 px-3 py-3 text-left">{{ t('diskCacheCleanup.disks.columns.status') }}</th>
                  <th class="w-36 px-3 py-3 text-left">{{ t('diskCacheCleanup.disks.columns.cache') }}</th>
                  <th class="w-40 px-3 py-3 text-right">{{ t('diskCacheCleanup.disks.columns.actions') }}</th>
                </tr>
              </thead>
              <tbody class="divide-y divide-slate-100">
                <template
                  v-for="disk in disks"
                  :key="disk.storageId"
                >
                  <tr class="hover:bg-slate-50/70 transition-colors">
                    <td class="px-3 py-3">
                      <button
                        type="button"
                        class="inline-flex h-8 w-8 items-center justify-center rounded-lg border border-slate-200 bg-white text-slate-500 transition hover:border-slate-300 hover:bg-slate-50"
                        :aria-expanded="expandedIds.has(disk.storageId)"
                        @click="toggleExpanded(disk.storageId)"
                      >
                        <ChevronDown
                          v-if="expandedIds.has(disk.storageId)"
                          class="h-4 w-4"
                        />
                        <ChevronRight v-else class="h-4 w-4" />
                      </button>
                    </td>
                    <td class="px-3 py-3 font-mono text-slate-700">
                      {{ disk.slot > 0 ? disk.slot : '--' }}
                    </td>
                    <td class="px-3 py-3">
                      <div class="font-medium text-slate-900">{{ disk.deviceName || '--' }}</div>
                      <div class="mt-1 font-mono text-xs text-slate-400">{{ disk.storageId }}</div>
                    </td>
                    <td class="px-3 py-3 text-right font-mono text-slate-700">
                      {{ formatCapacity(disk.totalCapacity) }}
                    </td>
                    <td class="px-3 py-3">
                      <span
                        class="inline-flex items-center rounded-full border px-2.5 py-1 text-xs font-semibold"
                        :class="usageBadgeClass(disk.usage)"
                      >
                        {{ t(usageLabelKey(disk.usage)) }}
                      </span>
                    </td>
                    <td class="px-3 py-3">
                      <span
                        class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold"
                        :class="statusBadgeClass(disk.storageStatus)"
                      >
                        <span
                          class="h-1.5 w-1.5 rounded-full bg-current"
                          :class="{ 'animate-pulse': statusIsBusy(disk.storageStatus) }"
                        ></span>
                        {{ t(statusLabelKey(disk.storageStatus)) }}
                      </span>
                    </td>
                    <td class="px-3 py-3">
                      <span
                        v-if="!redisAvailable"
                        class="inline-flex items-center rounded-full border border-amber-200 bg-amber-50 px-2.5 py-1 text-xs font-semibold text-amber-700"
                      >
                        {{ t('diskCacheCleanup.cache.unavailable') }}
                      </span>
                      <span
                        v-else-if="presentCacheIds.has(disk.storageId)"
                        class="inline-flex items-center rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 text-xs font-semibold text-indigo-700"
                      >
                        {{ t('diskCacheCleanup.cache.present') }}
                      </span>
                      <span v-else class="text-sm text-slate-400">
                        {{ t('diskCacheCleanup.cache.absent') }}
                      </span>
                    </td>
                    <td class="px-3 py-3 text-right">
                      <button
                        type="button"
                        class="inline-flex items-center justify-center gap-2 rounded-xl px-3 py-2 text-xs font-semibold text-white transition disabled:cursor-not-allowed disabled:bg-slate-300"
                        :class="redisAvailable && presentCacheIds.has(disk.storageId) ? 'bg-rose-500 hover:bg-rose-600' : 'bg-slate-300'"
                        :disabled="!redisAvailable || !presentCacheIds.has(disk.storageId) || cleaningIds.has(disk.storageId)"
                        :title="!redisAvailable ? t('diskCacheCleanup.disabled.redisDown') : undefined"
                        @click="cleanOne(disk.storageId)"
                      >
                        <Loader
                          v-if="cleaningIds.has(disk.storageId)"
                          class="h-3.5 w-3.5 animate-spin"
                        />
                        <Trash2 v-else class="h-3.5 w-3.5" />
                        <span>{{ cleaningIds.has(disk.storageId) ? t('diskCacheCleanup.actions.cleaningOne') : t('diskCacheCleanup.actions.cleanOne') }}</span>
                      </button>
                    </td>
                  </tr>

                  <tr v-if="expandedIds.has(disk.storageId)" class="bg-slate-50/70">
                    <td colspan="8" class="px-5 py-4">
                      <div class="grid grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
                        <div class="space-y-3 rounded-2xl border border-slate-200 bg-white px-4 py-4">
                          <div>
                            <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                              {{ t('diskCacheCleanup.disks.storageId') }}
                            </div>
                            <div class="mt-1 break-all font-mono text-sm text-slate-800">{{ disk.storageId }}</div>
                          </div>
                          <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
                            <div>
                              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                                {{ t('diskCacheCleanup.disks.cacheKey') }}
                              </div>
                              <div class="mt-1 break-all font-mono text-sm text-slate-800">{{ `Storage:${disk.storageId}` }}</div>
                            </div>
                            <div>
                              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                                {{ t('diskCacheCleanup.disks.enclosure') }}
                              </div>
                              <div class="mt-1 text-sm font-semibold text-slate-800">{{ disk.enclosureIndex }}</div>
                            </div>
                            <div>
                              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                                {{ t('diskCacheCleanup.disks.storageType') }}
                              </div>
                              <div class="mt-1 text-sm font-semibold text-slate-800">{{ disk.storageType }}</div>
                            </div>
                          </div>
                        </div>

                        <div class="rounded-2xl border border-slate-200 bg-white px-4 py-4">
                          <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                            {{ t('diskCacheCleanup.disks.wwn') }}
                          </div>
                          <div
                            v-if="disk.worldWideNameList?.length"
                            class="mt-3 space-y-2"
                          >
                            <div
                              v-for="item in disk.worldWideNameList"
                              :key="item.wwn"
                              class="rounded-xl border border-slate-200 bg-slate-50 px-3 py-2"
                            >
                              <div class="break-all font-mono text-xs text-slate-800">{{ item.wwn }}</div>
                              <div class="mt-1 text-[11px] text-slate-500">blockSize: {{ item.blockSize }}</div>
                            </div>
                          </div>
                          <p v-else class="mt-3 text-sm text-slate-400">{{ t('diskCacheCleanup.disks.noWwn') }}</p>
                        </div>
                      </div>
                    </td>
                  </tr>
                </template>
              </tbody>
            </table>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>
