<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
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
  diskCleanupCheckCacheKeys,
  diskCleanupDeleteCacheKeys,
  diskCleanupGetCacheKeyContents,
  diskCleanupListIpsans,
  diskCleanupListLinuxDisks,
  diskCleanupListLinuxServers,
  diskCleanupListWindowsDisks,
  getConfig,
  saveConfig,
  type AppConfig,
  type CacheKeyContentEntry,
  type DiskInfoItem,
  type DiskServerItem,
  type IpsanItem,
  type WindowsDiskItem,
} from '../lib/tauri';
import { getSuggestedDiskCleanupHosts } from '../lib/diskCacheCleanupPresentation';
import { mergeRecentItems, normalizeRecentItems } from '../lib/recentHistory';

defineOptions({
  name: 'DiskCacheCleanupPage',
});

type LocalDiskTab = 'windows' | 'linux';

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
const localDiskTab = ref<LocalDiskTab>('linux');

const linuxServerList = ref<DiskServerItem[]>([]);
const selectedLinuxServerIp = ref('');
const linuxDisks = ref<DiskInfoItem[]>([]);
const windowsDisks = ref<WindowsDiskItem[]>([]);
const localExpandedIds = ref<Set<string>>(new Set());
const localPresentCacheKeys = ref<Set<string>>(new Set());
const localCacheContentEntries = ref<Map<string, CacheKeyContentEntry>>(new Map());
const localCleaningKeys = ref<Set<string>>(new Set());
const localLoading = ref(false);
const localBatchCleaning = ref(false);
const localError = ref<string | null>(null);
const localRedisAvailable = ref(true);
const localRedisError = ref<string | null>(null);
const hasFetchedLocal = ref(false);

const ipsans = ref<IpsanItem[]>([]);
const ipsanPresentCacheKeys = ref<Set<string>>(new Set());
const ipsanCacheContentEntries = ref<Map<string, CacheKeyContentEntry>>(new Map());
const ipsanCleaningKeys = ref<Set<string>>(new Set());
const ipsanLoading = ref(false);
const ipsanBatchCleaning = ref(false);
const ipsanError = ref<string | null>(null);
const ipsanRedisAvailable = ref(true);
const ipsanRedisError = ref<string | null>(null);
const hasFetchedIpsan = ref(false);

let localRequestSeq = 0;
let ipsanRequestSeq = 0;

const savedSshHosts = computed(() => {
  return getSuggestedDiskCleanupHosts(config.value?.servers, recentHosts.value);
});

const selectedLinuxServer = computed(
  () => linuxServerList.value.find((item) => item.serverIp === selectedLinuxServerIp.value) ?? null,
);

const localTabTitle = computed(() =>
  t(
    localDiskTab.value === 'windows'
      ? 'diskCacheCleanup.localDisk.tabs.windows'
      : 'diskCacheCleanup.localDisk.tabs.linux',
  ),
);

const fetchAllLoading = computed(() => localLoading.value || ipsanLoading.value);

const canFetchAll = computed(
  () => hostIp.value.trim().length > 0 && !fetchAllLoading.value,
);

const localRowCount = computed(() => {
  if (localDiskTab.value === 'windows') {
    return windowsDisks.value.reduce((sum, disk) => sum + disk.partitionList.length, 0);
  }
  return linuxDisks.value.length;
});

const localCachedCount = computed(() => localCleanableKeys.value.length);
const ipsanCachedCount = computed(() => ipsanCleanableKeys.value.length);

const localSummaryText = computed(() => {
  if (!hasFetchedLocal.value) {
    return t('diskCacheCleanup.disks.summaryIdle');
  }
  return t('diskCacheCleanup.disks.summary', {
    total: localRowCount.value,
    cached: localCachedCount.value,
  });
});

const ipsanSummaryText = computed(() => {
  if (!hasFetchedIpsan.value) {
    return t('diskCacheCleanup.ipsan.description');
  }
  return t('diskCacheCleanup.disks.summary', {
    total: ipsans.value.length,
    cached: ipsanCachedCount.value,
  });
});

const localCleanableKeys = computed(() => {
  if (localDiskTab.value === 'windows') {
    return windowsPartitionCacheKeys(windowsDisks.value)
      .filter((key) => localPresentCacheKeys.value.has(key));
  }

  return linuxDiskCacheKeys(linuxDisks.value)
    .filter((key) => localPresentCacheKeys.value.has(key));
});

const ipsanCleanableKeys = computed(() =>
  ipsanCacheKeys(ipsans.value).filter((key) => ipsanPresentCacheKeys.value.has(key)),
);

function formatError(error: unknown) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function linuxDiskCacheKey(storageId: string) {
  return `Storage:${storageId}`;
}

function windowsPartitionCacheKey(partitionGuid: string) {
  return `Storage:${partitionGuid}`;
}

function ipsanCacheKey(ipsanId: string) {
  return `Storage:${ipsanId}`;
}

function linuxDiskCacheKeys(items: DiskInfoItem[]) {
  return items.map((item) => linuxDiskCacheKey(item.storageId));
}

function windowsPartitionCacheKeys(items: WindowsDiskItem[]) {
  return items.flatMap((disk) =>
    disk.partitionList.map((partition) => windowsPartitionCacheKey(partition.partitionGUID)),
  );
}

function ipsanCacheKeys(items: IpsanItem[]) {
  return items.map((item) => ipsanCacheKey(item.IPSANId));
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

function buildCacheContentEntryMap(entries: CacheKeyContentEntry[]) {
  const next = new Map<string, CacheKeyContentEntry>();
  for (const entry of entries) {
    next.set(entry.key, entry);
  }
  return next;
}

function cachePreviewText(entry: CacheKeyContentEntry | null | undefined) {
  if (!entry) {
    return t('diskCacheCleanup.cache.emptyContent');
  }
  return entry.preview || t('diskCacheCleanup.cache.emptyContent');
}

function cacheValueType(entry: CacheKeyContentEntry | null | undefined) {
  return entry?.value_type || '--';
}

function localCacheContentEntry(key: string) {
  return localCacheContentEntries.value.get(key) ?? null;
}

function ipsanCacheContentEntry(key: string) {
  return ipsanCacheContentEntries.value.get(key) ?? null;
}

function resetLocalCacheState() {
  localExpandedIds.value = new Set();
  localPresentCacheKeys.value = new Set();
  localCacheContentEntries.value = new Map();
  localCleaningKeys.value = new Set();
  localRedisAvailable.value = true;
  localRedisError.value = null;
}

function resetIpsanCacheState() {
  ipsanPresentCacheKeys.value = new Set();
  ipsanCacheContentEntries.value = new Map();
  ipsanCleaningKeys.value = new Set();
  ipsanRedisAvailable.value = true;
  ipsanRedisError.value = null;
}

async function loadLocalCacheContent(host: string, keys: string[], requestSeq: number) {
  if (keys.length === 0) {
    localCacheContentEntries.value = new Map();
    return;
  }

  const result = await diskCleanupGetCacheKeyContents(host, keys);
  if (requestSeq !== localRequestSeq) return;

  localRedisAvailable.value = result.redis_available;
  localRedisError.value = result.error;
  localCacheContentEntries.value = buildCacheContentEntryMap(result.entries ?? []);
}

async function loadIpsanCacheContent(host: string, keys: string[], requestSeq: number) {
  if (keys.length === 0) {
    ipsanCacheContentEntries.value = new Map();
    return;
  }

  const result = await diskCleanupGetCacheKeyContents(host, keys);
  if (requestSeq !== ipsanRequestSeq) return;

  ipsanRedisAvailable.value = result.redis_available;
  ipsanRedisError.value = result.error;
  ipsanCacheContentEntries.value = buildCacheContentEntryMap(result.entries ?? []);
}

async function loadLinuxDisksFor(host: string, serverIp: string, requestSeq: number) {
  try {
    const disks = await diskCleanupListLinuxDisks(host, serverIp, timeoutSecs.value);
    if (requestSeq !== localRequestSeq) return;

    linuxDisks.value = disks;
    localExpandedIds.value = new Set();

    const result = await diskCleanupCheckCacheKeys(host, linuxDiskCacheKeys(disks));
    if (requestSeq !== localRequestSeq) return;

    localRedisAvailable.value = result.redis_available;
    localRedisError.value = result.error;
    const presentKeys = result.present_keys ?? [];
    localPresentCacheKeys.value = new Set(presentKeys);
    await loadLocalCacheContent(host, presentKeys, requestSeq);
  } catch (error) {
    if (requestSeq !== localRequestSeq) return;
    linuxDisks.value = [];
    localPresentCacheKeys.value = new Set();
    localCacheContentEntries.value = new Map();
    localError.value = t('diskCacheCleanup.errors.localHttp', {
      reason: formatError(error),
    });
  }
}

async function fetchLinuxLocalRegion() {
  const host = hostIp.value.trim();
  if (!host) return;

  const requestSeq = ++localRequestSeq;
  localLoading.value = true;
  hasFetchedLocal.value = true;
  localError.value = null;
  resetLocalCacheState();
  windowsDisks.value = [];

  try {
    const servers = await diskCleanupListLinuxServers(host, timeoutSecs.value);
    if (requestSeq !== localRequestSeq) return;

    linuxServerList.value = servers;
    const nextServerIp = servers.find((item) => item.serverIp === selectedLinuxServerIp.value)?.serverIp
      ?? servers[0]?.serverIp
      ?? '';
    selectedLinuxServerIp.value = nextServerIp;

    if (!nextServerIp) {
      linuxDisks.value = [];
      localPresentCacheKeys.value = new Set();
      localCacheContentEntries.value = new Map();
      return;
    }

    await loadLinuxDisksFor(host, nextServerIp, requestSeq);
  } catch (error) {
    if (requestSeq !== localRequestSeq) return;
    linuxServerList.value = [];
    selectedLinuxServerIp.value = '';
    linuxDisks.value = [];
    localPresentCacheKeys.value = new Set();
    localError.value = t('diskCacheCleanup.errors.localHttp', {
      reason: formatError(error),
    });
  } finally {
    if (requestSeq === localRequestSeq) {
      localLoading.value = false;
    }
  }
}

async function fetchWindowsLocalRegion() {
  const host = hostIp.value.trim();
  if (!host) return;

  const requestSeq = ++localRequestSeq;
  localLoading.value = true;
  hasFetchedLocal.value = true;
  localError.value = null;
  resetLocalCacheState();
  linuxServerList.value = [];
  selectedLinuxServerIp.value = '';
  linuxDisks.value = [];

  try {
    const disks = await diskCleanupListWindowsDisks(host, timeoutSecs.value);
    if (requestSeq !== localRequestSeq) return;

    windowsDisks.value = disks;
    const result = await diskCleanupCheckCacheKeys(host, windowsPartitionCacheKeys(disks));
    if (requestSeq !== localRequestSeq) return;

    localRedisAvailable.value = result.redis_available;
    localRedisError.value = result.error;
    const presentKeys = result.present_keys ?? [];
    localPresentCacheKeys.value = new Set(presentKeys);
    await loadLocalCacheContent(host, presentKeys, requestSeq);
  } catch (error) {
    if (requestSeq !== localRequestSeq) return;
    windowsDisks.value = [];
    localPresentCacheKeys.value = new Set();
    localCacheContentEntries.value = new Map();
    localError.value = t('diskCacheCleanup.errors.localHttp', {
      reason: formatError(error),
    });
  } finally {
    if (requestSeq === localRequestSeq) {
      localLoading.value = false;
    }
  }
}

async function fetchLocalRegion() {
  if (localDiskTab.value === 'windows') {
    await fetchWindowsLocalRegion();
    return;
  }
  await fetchLinuxLocalRegion();
}

async function fetchIpsanRegion() {
  const host = hostIp.value.trim();
  if (!host) return;

  const requestSeq = ++ipsanRequestSeq;
  ipsanLoading.value = true;
  hasFetchedIpsan.value = true;
  ipsanError.value = null;
  resetIpsanCacheState();

  try {
    const items = await diskCleanupListIpsans(host, timeoutSecs.value);
    if (requestSeq !== ipsanRequestSeq) return;

    ipsans.value = items;
    const result = await diskCleanupCheckCacheKeys(host, ipsanCacheKeys(items));
    if (requestSeq !== ipsanRequestSeq) return;

    ipsanRedisAvailable.value = result.redis_available;
    ipsanRedisError.value = result.error;
    const presentKeys = result.present_keys ?? [];
    ipsanPresentCacheKeys.value = new Set(presentKeys);
    await loadIpsanCacheContent(host, presentKeys, requestSeq);
  } catch (error) {
    if (requestSeq !== ipsanRequestSeq) return;
    ipsans.value = [];
    ipsanPresentCacheKeys.value = new Set();
    ipsanCacheContentEntries.value = new Map();
    ipsanError.value = t('diskCacheCleanup.errors.ipsanHttp', {
      reason: formatError(error),
    });
  } finally {
    if (requestSeq === ipsanRequestSeq) {
      ipsanLoading.value = false;
    }
  }
}

async function handleFetchAll() {
  const host = hostIp.value.trim();
  if (!host) return;

  await pushRecentHost(host);
  await Promise.all([
    fetchLocalRegion(),
    fetchIpsanRegion(),
  ]);
}

async function handleRefreshLocal() {
  const host = hostIp.value.trim();
  if (!host) return;

  await pushRecentHost(host);
  await fetchLocalRegion();
}

async function handleRefreshIpsan() {
  const host = hostIp.value.trim();
  if (!host) return;

  await pushRecentHost(host);
  await fetchIpsanRegion();
}

async function handleLinuxServerChange(serverIp: string) {
  const host = hostIp.value.trim();
  if (!host) return;

  selectedLinuxServerIp.value = serverIp;
  if (!serverIp) {
    linuxDisks.value = [];
    localPresentCacheKeys.value = new Set();
    localCacheContentEntries.value = new Map();
    return;
  }

  const requestSeq = ++localRequestSeq;
  localLoading.value = true;
  localError.value = null;
  resetLocalCacheState();

  try {
    await loadLinuxDisksFor(host, serverIp, requestSeq);
  } finally {
    if (requestSeq === localRequestSeq) {
      localLoading.value = false;
    }
  }
}

function toggleExpanded(storageId: string) {
  const next = new Set(localExpandedIds.value);
  if (next.has(storageId)) {
    next.delete(storageId);
  } else {
    next.add(storageId);
  }
  localExpandedIds.value = next;
}

async function cleanLocalKeys(keys: string[], singleKey?: string) {
  const host = hostIp.value.trim();
  if (!host || keys.length === 0) return;

  if (singleKey) {
    const next = new Set(localCleaningKeys.value);
    next.add(singleKey);
    localCleaningKeys.value = next;
  } else {
    const confirmed = window.confirm(
      t('diskCacheCleanup.actions.cleanAllConfirm', {
        count: keys.length,
      }),
    );
    if (!confirmed) return;
    localBatchCleaning.value = true;
  }

  localError.value = null;

  try {
    const result = await diskCleanupDeleteCacheKeys(host, keys);
    if (!result.redis_available || result.error) {
      localRedisAvailable.value = result.redis_available;
      localRedisError.value = result.error;
      localError.value = t('diskCacheCleanup.errors.localDelete', {
        reason: result.error ?? t('diskCacheCleanup.cache.unavailable'),
      });
      return;
    }

    await fetchLocalRegion();
  } catch (error) {
    localError.value = t('diskCacheCleanup.errors.localDelete', {
      reason: formatError(error),
    });
  } finally {
    if (singleKey) {
      const next = new Set(localCleaningKeys.value);
      next.delete(singleKey);
      localCleaningKeys.value = next;
    } else {
      localBatchCleaning.value = false;
    }
  }
}

async function cleanIpsanKeys(keys: string[], singleKey?: string) {
  const host = hostIp.value.trim();
  if (!host || keys.length === 0) return;

  if (singleKey) {
    const next = new Set(ipsanCleaningKeys.value);
    next.add(singleKey);
    ipsanCleaningKeys.value = next;
  } else {
    const confirmed = window.confirm(
      t('diskCacheCleanup.actions.cleanAllConfirm', {
        count: keys.length,
      }),
    );
    if (!confirmed) return;
    ipsanBatchCleaning.value = true;
  }

  ipsanError.value = null;

  try {
    const result = await diskCleanupDeleteCacheKeys(host, keys);
    if (!result.redis_available || result.error) {
      ipsanRedisAvailable.value = result.redis_available;
      ipsanRedisError.value = result.error;
      ipsanError.value = t('diskCacheCleanup.errors.ipsanDelete', {
        reason: result.error ?? t('diskCacheCleanup.cache.unavailable'),
      });
      return;
    }

    await fetchIpsanRegion();
  } catch (error) {
    ipsanError.value = t('diskCacheCleanup.errors.ipsanDelete', {
      reason: formatError(error),
    });
  } finally {
    if (singleKey) {
      const next = new Set(ipsanCleaningKeys.value);
      next.delete(singleKey);
      ipsanCleaningKeys.value = next;
    } else {
      ipsanBatchCleaning.value = false;
    }
  }
}

async function saveTimeout() {
  if (!config.value) return;

  config.value.disk_cleanup_http_timeout_secs = timeoutSecs.value;
  try {
    await saveConfig(config.value);
  } catch (error) {
    localError.value = t('diskCacheCleanup.errors.localHttp', {
      reason: formatError(error),
    });
  }
}

watch(localDiskTab, async () => {
  if (!hasFetchedLocal.value || !hostIp.value.trim()) return;
  await fetchLocalRegion();
});

onMounted(async () => {
  try {
    const loaded = await getConfig();
    config.value = loaded;
    timeoutSecs.value = Number(config.value.disk_cleanup_http_timeout_secs ?? 5);
  } catch (error) {
    localError.value = t('diskCacheCleanup.errors.localHttp', {
      reason: formatError(error),
    });
  }

  await loadRecentHosts();
});
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-[radial-gradient(circle_at_top_left,_rgba(14,165,233,0.14),_transparent_28%),linear-gradient(180deg,_#f8fbff_0%,_#eef4fb_40%,_#f8fafc_100%)]">
    <div class="mx-auto flex w-full max-w-7xl flex-col gap-5 px-6 py-6 pb-10">
      <section class="relative overflow-hidden rounded-[28px] border border-white/70 bg-white/85 px-6 py-6 shadow-[0_18px_60px_rgba(15,23,42,0.08)] backdrop-blur">
        <div class="absolute -left-8 top-0 h-32 w-32 rounded-full bg-slate-100/80 blur-3xl"></div>
        <div class="absolute right-0 top-0 h-40 w-40 rounded-full bg-amber-100/70 blur-3xl"></div>
        <div class="relative flex flex-col gap-5 lg:flex-row lg:items-end lg:justify-between">
          <div class="space-y-2">
            <div class="flex items-center gap-2">
              <span class="h-1.5 w-1.5 rounded-full bg-amber-500"></span>
              <span class="text-[11px] font-bold uppercase tracking-[0.12em] text-slate-500">Redis Cache Ops</span>
            </div>
            <div class="flex items-start gap-3">
              <div class="flex h-12 w-12 items-center justify-center rounded-2xl bg-gradient-to-br from-[#1E40AF] via-[#3B82F6] to-[#6366F1] text-white shadow-lg shadow-blue-500/20">
                <HardDrive class="h-6 w-6" />
              </div>
              <div>
                <h1 class="text-2xl font-bold tracking-tight text-[#0F172A]">
                  {{ t('diskCacheCleanup.title') }}
                </h1>
                <p class="mt-1 max-w-3xl text-sm leading-6 text-[#334155]">
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
                {{ t('diskCacheCleanup.localDisk.title') }}
              </div>
              <div class="mt-2 font-mono text-2xl font-bold text-slate-900">{{ localCachedCount }}</div>
            </div>
            <div class="rounded-2xl border border-slate-200/80 bg-slate-50/80 px-4 py-3">
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.ipsan.title') }}
              </div>
              <div class="mt-2 font-mono text-2xl font-bold text-slate-900">{{ ipsanCachedCount }}</div>
            </div>
          </div>
        </div>
      </section>

      <section class="rounded-[24px] border border-slate-200/80 bg-white/90 p-5 shadow-[0_14px_40px_rgba(15,23,42,0.06)]">
        <div class="grid gap-5 xl:grid-cols-[minmax(0,1.6fr)_minmax(280px,0.9fr)]">
          <div class="space-y-4">
            <div>
              <label class="text-sm font-semibold text-slate-800">{{ t('diskCacheCleanup.hostIp.label') }}</label>
              <div class="mt-2 flex flex-col gap-3 sm:flex-row">
                <input
                  v-model="hostIp"
                  type="text"
                  :placeholder="t('diskCacheCleanup.hostIp.placeholder')"
                  class="w-full rounded-2xl border border-slate-300 bg-white px-4 py-3 text-sm font-mono text-slate-900 outline-none transition focus:border-sky-500 focus:ring-4 focus:ring-sky-500/10"
                  @keyup.enter="handleFetchAll"
                >
                <button
                  type="button"
                  class="inline-flex shrink-0 items-center justify-center gap-2 rounded-2xl bg-[#0369A1] px-5 py-3 text-sm font-semibold text-white shadow-sm transition hover:bg-sky-700 disabled:cursor-not-allowed disabled:bg-slate-300"
                  :disabled="!canFetchAll"
                  @click="handleFetchAll"
                >
                  <Loader v-if="fetchAllLoading" class="h-4 w-4 animate-spin" />
                  <RefreshCw v-else class="h-4 w-4" />
                  <span>{{ fetchAllLoading ? t('diskCacheCleanup.actions.fetching') : t('diskCacheCleanup.actions.fetch') }}</span>
                </button>
              </div>
              <p class="mt-2 text-xs leading-5 text-slate-500">{{ t('diskCacheCleanup.hostIp.hint') }}</p>
            </div>

            <div v-if="recentHosts.length || savedSshHosts.length" class="space-y-3">
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

          <div class="grid grid-cols-1 gap-3 rounded-2xl border border-slate-200 bg-slate-50/70 p-4">
            <div>
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.localDisk.title') }}
              </div>
              <div class="mt-2 inline-flex gap-1 rounded-full border border-slate-200 bg-slate-100 p-1">
                <button
                  type="button"
                  class="rounded-full px-4 py-2 text-sm font-semibold transition"
                  :class="localDiskTab === 'windows' ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-500 hover:text-slate-700'"
                  @click="localDiskTab = 'windows'"
                >
                  {{ t('diskCacheCleanup.localDisk.tabs.windows') }}
                </button>
                <button
                  type="button"
                  class="rounded-full px-4 py-2 text-sm font-semibold transition"
                  :class="localDiskTab === 'linux' ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-500 hover:text-slate-700'"
                  @click="localDiskTab = 'linux'"
                >
                  {{ t('diskCacheCleanup.localDisk.tabs.linux') }}
                </button>
              </div>
            </div>

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

      <section class="rounded-[24px] border border-slate-200/80 bg-white/90 shadow-[0_14px_40px_rgba(15,23,42,0.06)]">
        <div class="flex flex-col gap-4 border-b border-slate-200/80 px-5 py-5 md:flex-row md:items-center md:justify-between">
          <div>
            <h2 class="text-lg font-bold text-slate-900">{{ t('diskCacheCleanup.localDisk.title') }}</h2>
            <p class="mt-1 text-sm text-slate-500">
              {{ localTabTitle }} · {{ localSummaryText }}
            </p>
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 transition hover:border-slate-300 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50"
              :disabled="!hostIp.trim() || localLoading"
              @click="handleRefreshLocal"
            >
              <RefreshCw class="h-4 w-4" :class="{ 'animate-spin': localLoading }" />
              {{ t('diskCacheCleanup.localDisk.actions.refresh') }}
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-2xl px-4 py-2.5 text-sm font-semibold text-white transition disabled:cursor-not-allowed disabled:bg-slate-300"
              :class="localRedisAvailable && localCleanableKeys.length > 0 ? 'bg-rose-500 hover:bg-rose-600' : 'bg-slate-300'"
              :disabled="!localRedisAvailable || localCleanableKeys.length === 0 || localBatchCleaning"
              :title="!localRedisAvailable ? t('diskCacheCleanup.disabled.redisDown') : undefined"
              @click="cleanLocalKeys(localCleanableKeys)"
            >
              <Loader v-if="localBatchCleaning" class="h-4 w-4 animate-spin" />
              <Trash2 v-else class="h-4 w-4" />
              {{ localBatchCleaning ? t('diskCacheCleanup.actions.cleaningAll') : t('diskCacheCleanup.localDisk.actions.cleanAll', { count: localCleanableKeys.length }) }}
            </button>
          </div>
        </div>

        <div class="space-y-4 p-5">
          <section
            v-if="localError"
            class="flex items-start gap-3 rounded-[20px] border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700 shadow-sm"
          >
            <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
            <span>{{ localError }}</span>
          </section>

          <section
            v-if="localRedisError && !localRedisAvailable"
            class="flex items-start gap-3 rounded-[20px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800 shadow-sm"
          >
            <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
            <span>{{ localRedisError }}</span>
          </section>

          <section
            v-if="localDiskTab === 'linux' && (hasFetchedLocal || linuxServerList.length > 0)"
            class="rounded-[20px] border border-slate-200 bg-slate-50/70 p-4"
          >
            <div class="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
              <div class="flex-1">
                <div class="mb-2 flex items-center gap-2">
                  <Server class="h-4 w-4 text-slate-400" />
                  <label class="text-sm font-semibold text-slate-800">{{ t('diskCacheCleanup.server.pick') }}</label>
                </div>
                <select
                  :value="selectedLinuxServerIp"
                  class="w-full rounded-2xl border border-slate-300 bg-white px-4 py-3 text-sm text-slate-800 outline-none transition focus:border-sky-500 focus:ring-4 focus:ring-sky-500/10"
                  :disabled="linuxServerList.length === 0 || localLoading"
                  @change="handleLinuxServerChange(($event.target as HTMLSelectElement).value)"
                >
                  <option
                    v-for="server in linuxServerList"
                    :key="server.serverIp"
                    :value="server.serverIp"
                  >
                    {{ `${server.serverName || server.serverIp} · ${server.serverIp}${server.role ? ` · ${server.role}` : ''}` }}
                  </option>
                </select>
              </div>

              <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
                <div class="rounded-2xl border border-slate-200 bg-white px-4 py-3">
                  <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                    {{ t('diskCacheCleanup.server.detailsIp') }}
                  </div>
                  <div class="mt-2 font-mono text-sm font-semibold text-slate-900">
                    {{ selectedLinuxServer?.serverIp ?? '--' }}
                  </div>
                </div>
                <div class="rounded-2xl border border-slate-200 bg-white px-4 py-3">
                  <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                    {{ t('diskCacheCleanup.server.detailsRole') }}
                  </div>
                  <div class="mt-2 text-sm font-semibold text-slate-900">
                    {{ selectedLinuxServer?.role || '--' }}
                  </div>
                </div>
                <div class="rounded-2xl border border-slate-200 bg-white px-4 py-3">
                  <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                    {{ t('diskCacheCleanup.server.detailsSerial') }}
                  </div>
                  <div class="mt-2 font-mono text-sm font-semibold text-slate-900">
                    {{ selectedLinuxServer?.serial || '--' }}
                  </div>
                </div>
              </div>
            </div>
          </section>

          <div
            v-if="!hasFetchedLocal && !localLoading"
            class="flex min-h-[240px] flex-col items-center justify-center rounded-[20px] border border-dashed border-slate-200 bg-slate-50/60 px-6 py-8 text-center"
          >
            <div class="flex h-14 w-14 items-center justify-center rounded-2xl bg-sky-50 text-sky-600">
              <HardDrive class="h-7 w-7" />
            </div>
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.disks.emptyIdle') }}</p>
            <p class="mt-2 max-w-md text-sm leading-6 text-slate-500">{{ t('diskCacheCleanup.disks.emptyIdleHint') }}</p>
          </div>

          <div
            v-else-if="localLoading && localRowCount === 0 && (localDiskTab === 'windows' || linuxServerList.length === 0)"
            class="flex min-h-[240px] flex-col items-center justify-center rounded-[20px] border border-dashed border-slate-200 bg-slate-50/60 px-6 py-8 text-center"
          >
            <Loader class="h-7 w-7 animate-spin text-sky-600" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.disks.loading') }}</p>
            <p class="mt-2 text-sm text-slate-500">{{ t('diskCacheCleanup.disks.loadingHint') }}</p>
          </div>

          <div
            v-else-if="localDiskTab === 'linux' && hasFetchedLocal && linuxServerList.length === 0 && !localLoading"
            class="flex min-h-[240px] flex-col items-center justify-center rounded-[20px] border border-dashed border-slate-200 bg-slate-50/60 px-6 py-8 text-center"
          >
            <Server class="h-8 w-8 text-slate-400" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.server.empty') }}</p>
            <p class="mt-2 text-sm leading-6 text-slate-500">{{ t('diskCacheCleanup.server.emptyHint') }}</p>
          </div>

          <div
            v-else-if="localDiskTab === 'linux' && linuxDisks.length === 0 && !localLoading"
            class="flex min-h-[240px] flex-col items-center justify-center rounded-[20px] border border-dashed border-slate-200 bg-slate-50/60 px-6 py-8 text-center"
          >
            <HardDrive class="h-8 w-8 text-slate-400" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.disks.empty') }}</p>
            <p class="mt-2 text-sm leading-6 text-slate-500">{{ t('diskCacheCleanup.disks.emptyHint') }}</p>
          </div>

          <div
            v-else-if="localDiskTab === 'windows' && windowsDisks.length === 0 && !localLoading"
            class="flex min-h-[240px] flex-col items-center justify-center rounded-[20px] border border-dashed border-slate-200 bg-slate-50/60 px-6 py-8 text-center"
          >
            <HardDrive class="h-8 w-8 text-slate-400" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.disks.empty') }}</p>
            <p class="mt-2 text-sm leading-6 text-slate-500">{{ t('diskCacheCleanup.disks.emptyHint') }}</p>
          </div>

          <div
            v-else-if="localDiskTab === 'linux'"
            class="overflow-x-auto"
            :class="{ 'opacity-70': localLoading }"
          >
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
                  v-for="disk in linuxDisks"
                  :key="disk.storageId"
                >
                  <tr class="hover:bg-slate-50/70 transition-colors">
                    <td class="px-3 py-3">
                      <button
                        type="button"
                        class="inline-flex h-8 w-8 items-center justify-center rounded-lg border border-slate-200 bg-white text-slate-500 transition hover:border-slate-300 hover:bg-slate-50"
                        :aria-expanded="localExpandedIds.has(disk.storageId)"
                        @click="toggleExpanded(disk.storageId)"
                      >
                        <ChevronDown
                          v-if="localExpandedIds.has(disk.storageId)"
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
                        v-if="!localRedisAvailable"
                        class="inline-flex items-center rounded-full border border-amber-200 bg-amber-50 px-2.5 py-1 text-xs font-semibold text-amber-700"
                      >
                        {{ t('diskCacheCleanup.cache.unavailable') }}
                      </span>
                      <span
                        v-else-if="localPresentCacheKeys.has(linuxDiskCacheKey(disk.storageId))"
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
                        v-if="localPresentCacheKeys.has(linuxDiskCacheKey(disk.storageId))"
                        type="button"
                        class="inline-flex items-center justify-center gap-2 rounded-xl bg-rose-500 px-3 py-2 text-xs font-semibold text-white transition hover:bg-rose-600 disabled:cursor-not-allowed disabled:bg-slate-300"
                        :disabled="!localRedisAvailable || localCleaningKeys.has(linuxDiskCacheKey(disk.storageId))"
                        :title="!localRedisAvailable ? t('diskCacheCleanup.disabled.redisDown') : undefined"
                        @click="cleanLocalKeys([linuxDiskCacheKey(disk.storageId)], linuxDiskCacheKey(disk.storageId))"
                      >
                        <Loader
                          v-if="localCleaningKeys.has(linuxDiskCacheKey(disk.storageId))"
                          class="h-3.5 w-3.5 animate-spin"
                        />
                        <Trash2 v-else class="h-3.5 w-3.5" />
                        <span>{{ localCleaningKeys.has(linuxDiskCacheKey(disk.storageId)) ? t('diskCacheCleanup.actions.cleaningOne') : t('diskCacheCleanup.actions.cleanOne') }}</span>
                      </button>
                    </td>
                  </tr>

                  <tr v-if="localExpandedIds.has(disk.storageId)" class="bg-slate-50/70">
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
                              <div class="mt-1 break-all font-mono text-sm text-slate-800">{{ linuxDiskCacheKey(disk.storageId) }}</div>
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

                          <div
                            v-if="localPresentCacheKeys.has(linuxDiskCacheKey(disk.storageId))"
                            class="rounded-xl border border-indigo-100 bg-indigo-50/60 px-3 py-3"
                          >
                            <div class="flex items-center justify-between gap-3">
                              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-500">
                                {{ t('diskCacheCleanup.cache.content') }}
                              </div>
                              <span class="inline-flex items-center rounded-full border border-indigo-200 bg-white px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-indigo-700">
                                {{ cacheValueType(localCacheContentEntry(linuxDiskCacheKey(disk.storageId))) }}
                              </span>
                            </div>
                            <pre class="mt-2 whitespace-pre-wrap break-all font-mono text-xs leading-5 text-slate-700">{{ cachePreviewText(localCacheContentEntry(linuxDiskCacheKey(disk.storageId))) }}</pre>
                            <p
                              v-if="localCacheContentEntry(linuxDiskCacheKey(disk.storageId))?.truncated"
                              class="mt-2 text-[11px] text-slate-400"
                            >
                              {{ t('diskCacheCleanup.cache.truncated') }}
                            </p>
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

          <section
            v-else
            class="space-y-4"
            :class="{ 'opacity-70': localLoading }"
          >
            <article
              v-for="disk in windowsDisks"
              :key="disk.diskId"
              class="overflow-hidden rounded-[20px] border border-slate-200 bg-white shadow-sm"
            >
              <header class="flex flex-col gap-2 border-b border-slate-200 bg-slate-50/80 px-5 py-4 md:flex-row md:items-center md:justify-between">
                <div>
                  <div class="text-sm font-semibold text-slate-900">
                    {{ t('diskCacheCleanup.windows.diskHeader', { number: disk.diskNumber, name: disk.diskName || '--' }) }}
                  </div>
                  <div class="mt-1 font-mono text-xs text-slate-500">{{ disk.diskId }}</div>
                </div>
                <div class="font-mono text-sm font-semibold text-slate-700">
                  {{ formatCapacity(disk.totalCapacity) }}
                </div>
              </header>

              <div class="overflow-x-auto">
                <table class="min-w-[860px] w-full text-sm">
                  <thead>
                    <tr class="border-b border-slate-200 bg-white text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-500">
                      <th class="w-24 px-4 py-3 text-left">{{ t('diskCacheCleanup.windows.columns.partitionSeq') }}</th>
                      <th class="px-4 py-3 text-left">{{ t('diskCacheCleanup.windows.columns.partitionGuid') }}</th>
                      <th class="w-28 px-4 py-3 text-right">{{ t('diskCacheCleanup.windows.columns.capacity') }}</th>
                      <th class="w-40 px-4 py-3 text-left">{{ t('diskCacheCleanup.windows.columns.usage') }}</th>
                      <th class="w-44 px-4 py-3 text-left">{{ t('diskCacheCleanup.windows.columns.status') }}</th>
                      <th class="w-36 px-4 py-3 text-left">{{ t('diskCacheCleanup.windows.columns.cache') }}</th>
                      <th class="w-40 px-4 py-3 text-right">{{ t('diskCacheCleanup.windows.columns.actions') }}</th>
                    </tr>
                  </thead>
                  <tbody class="divide-y divide-slate-100">
                    <tr
                      v-for="partition in disk.partitionList"
                      :key="partition.partitionGUID"
                      class="hover:bg-slate-50/70 transition-colors"
                    >
                      <td class="px-4 py-3 font-mono text-slate-700">
                        {{ partition.partitionSeq }}
                      </td>
                      <td class="px-4 py-3">
                        <div class="font-mono text-xs text-slate-800">{{ partition.partitionGUID }}</div>
                        <div class="mt-1 font-mono text-[11px] text-slate-400">{{ partition.partitionOffset || '--' }}</div>
                      </td>
                      <td class="px-4 py-3 text-right font-mono text-slate-700">
                        {{ formatCapacity(partition.capacity) }}
                      </td>
                      <td class="px-4 py-3">
                        <span
                          class="inline-flex items-center rounded-full border px-2.5 py-1 text-xs font-semibold"
                          :class="usageBadgeClass(partition.usage)"
                        >
                          {{ t(usageLabelKey(partition.usage)) }}
                        </span>
                      </td>
                      <td class="px-4 py-3">
                        <span
                          class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold"
                          :class="statusBadgeClass(partition.partitionStatus)"
                        >
                          <span
                            class="h-1.5 w-1.5 rounded-full bg-current"
                            :class="{ 'animate-pulse': statusIsBusy(partition.partitionStatus) }"
                          ></span>
                          {{ t(statusLabelKey(partition.partitionStatus)) }}
                        </span>
                      </td>
                      <td class="px-4 py-3">
                        <span
                          v-if="!localRedisAvailable"
                          class="inline-flex items-center rounded-full border border-amber-200 bg-amber-50 px-2.5 py-1 text-xs font-semibold text-amber-700"
                        >
                          {{ t('diskCacheCleanup.cache.unavailable') }}
                        </span>
                        <div
                          v-else-if="localPresentCacheKeys.has(windowsPartitionCacheKey(partition.partitionGUID))"
                          class="space-y-2"
                        >
                          <span
                            class="inline-flex items-center rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 text-xs font-semibold text-indigo-700"
                          >
                            {{ t('diskCacheCleanup.cache.present') }}
                          </span>
                          <div class="max-w-[280px] rounded-xl border border-indigo-100 bg-indigo-50/60 px-3 py-2">
                            <div class="text-[10px] font-semibold uppercase tracking-[0.12em] text-indigo-700">
                              {{ cacheValueType(localCacheContentEntry(windowsPartitionCacheKey(partition.partitionGUID))) }}
                            </div>
                            <pre class="mt-1 whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-slate-700">{{ cachePreviewText(localCacheContentEntry(windowsPartitionCacheKey(partition.partitionGUID))) }}</pre>
                            <p
                              v-if="localCacheContentEntry(windowsPartitionCacheKey(partition.partitionGUID))?.truncated"
                              class="mt-1 text-[10px] text-slate-400"
                            >
                              {{ t('diskCacheCleanup.cache.truncated') }}
                            </p>
                          </div>
                        </div>
                        <span v-else class="text-sm text-slate-400">
                          {{ t('diskCacheCleanup.cache.absent') }}
                        </span>
                      </td>
                      <td class="px-4 py-3 text-right">
                        <button
                          v-if="localPresentCacheKeys.has(windowsPartitionCacheKey(partition.partitionGUID))"
                          type="button"
                          class="inline-flex items-center justify-center gap-2 rounded-xl bg-rose-500 px-3 py-2 text-xs font-semibold text-white transition hover:bg-rose-600 disabled:cursor-not-allowed disabled:bg-slate-300"
                          :disabled="!localRedisAvailable || localCleaningKeys.has(windowsPartitionCacheKey(partition.partitionGUID))"
                          :title="!localRedisAvailable ? t('diskCacheCleanup.disabled.redisDown') : undefined"
                          @click="cleanLocalKeys([windowsPartitionCacheKey(partition.partitionGUID)], windowsPartitionCacheKey(partition.partitionGUID))"
                        >
                          <Loader
                            v-if="localCleaningKeys.has(windowsPartitionCacheKey(partition.partitionGUID))"
                            class="h-3.5 w-3.5 animate-spin"
                          />
                          <Trash2 v-else class="h-3.5 w-3.5" />
                          <span>{{ localCleaningKeys.has(windowsPartitionCacheKey(partition.partitionGUID)) ? t('diskCacheCleanup.actions.cleaningOne') : t('diskCacheCleanup.actions.cleanOne') }}</span>
                        </button>
                      </td>
                    </tr>

                    <tr v-if="disk.partitionList.length === 0">
                      <td colspan="7" class="px-4 py-6 text-center text-sm text-slate-400">
                        {{ t('diskCacheCleanup.disks.empty') }}
                      </td>
                    </tr>
                  </tbody>
                </table>
              </div>
            </article>
          </section>
        </div>
      </section>

      <section class="rounded-[24px] border border-orange-200/80 bg-white/90 shadow-[0_14px_40px_rgba(15,23,42,0.06)]">
        <div class="flex flex-col gap-4 border-b border-orange-100 px-5 py-5 md:flex-row md:items-center md:justify-between">
          <div>
            <h2 class="text-lg font-bold text-slate-900">{{ t('diskCacheCleanup.ipsan.title') }}</h2>
            <p class="mt-1 text-sm text-slate-500">
              {{ ipsanSummaryText }}
            </p>
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 transition hover:border-slate-300 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50"
              :disabled="!hostIp.trim() || ipsanLoading"
              @click="handleRefreshIpsan"
            >
              <RefreshCw class="h-4 w-4" :class="{ 'animate-spin': ipsanLoading }" />
              {{ t('diskCacheCleanup.ipsan.actions.refresh') }}
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-2xl px-4 py-2.5 text-sm font-semibold text-white transition disabled:cursor-not-allowed disabled:bg-slate-300"
              :class="ipsanRedisAvailable && ipsanCleanableKeys.length > 0 ? 'bg-rose-500 hover:bg-rose-600' : 'bg-slate-300'"
              :disabled="!ipsanRedisAvailable || ipsanCleanableKeys.length === 0 || ipsanBatchCleaning"
              :title="!ipsanRedisAvailable ? t('diskCacheCleanup.disabled.redisDown') : undefined"
              @click="cleanIpsanKeys(ipsanCleanableKeys)"
            >
              <Loader v-if="ipsanBatchCleaning" class="h-4 w-4 animate-spin" />
              <Trash2 v-else class="h-4 w-4" />
              {{ ipsanBatchCleaning ? t('diskCacheCleanup.actions.cleaningAll') : t('diskCacheCleanup.ipsan.actions.cleanAll', { count: ipsanCleanableKeys.length }) }}
            </button>
          </div>
        </div>

        <div class="space-y-4 p-5">
          <section
            v-if="ipsanError"
            class="flex items-start gap-3 rounded-[20px] border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700 shadow-sm"
          >
            <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
            <span>{{ ipsanError }}</span>
          </section>

          <section
            v-if="ipsanRedisError && !ipsanRedisAvailable"
            class="flex items-start gap-3 rounded-[20px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800 shadow-sm"
          >
            <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
            <span>{{ ipsanRedisError }}</span>
          </section>

          <div
            v-if="!hasFetchedIpsan && !ipsanLoading"
            class="flex min-h-[220px] flex-col items-center justify-center rounded-[20px] border border-dashed border-orange-200 bg-orange-50/50 px-6 py-8 text-center"
          >
            <Server class="h-8 w-8 text-orange-300" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.ipsan.title') }}</p>
            <p class="mt-2 max-w-md text-sm leading-6 text-slate-500">{{ t('diskCacheCleanup.ipsan.description') }}</p>
          </div>

          <div
            v-else-if="ipsanLoading && ipsans.length === 0"
            class="flex min-h-[220px] flex-col items-center justify-center rounded-[20px] border border-dashed border-orange-200 bg-orange-50/50 px-6 py-8 text-center"
          >
            <Loader class="h-7 w-7 animate-spin text-orange-500" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.disks.loading') }}</p>
            <p class="mt-2 text-sm text-slate-500">{{ t('diskCacheCleanup.ipsan.description') }}</p>
          </div>

          <div
            v-else-if="ipsans.length === 0"
            class="flex min-h-[220px] flex-col items-center justify-center rounded-[20px] border border-dashed border-orange-200 bg-orange-50/50 px-6 py-8 text-center"
          >
            <Server class="h-8 w-8 text-orange-300" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.disks.empty') }}</p>
            <p class="mt-2 text-sm leading-6 text-slate-500">{{ t('diskCacheCleanup.ipsan.description') }}</p>
          </div>

          <div
            v-else
            class="overflow-x-auto"
            :class="{ 'opacity-70': ipsanLoading }"
          >
            <table class="min-w-[920px] w-full text-sm">
              <thead>
                <tr class="border-b border-orange-100 bg-orange-50/60 text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-500">
                  <th class="px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.columns.name') }}</th>
                  <th class="w-48 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.columns.id') }}</th>
                  <th class="w-40 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.columns.status') }}</th>
                  <th class="w-28 px-4 py-3 text-right">{{ t('diskCacheCleanup.ipsan.columns.capacity') }}</th>
                  <th class="w-40 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.columns.usage') }}</th>
                  <th class="w-36 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.columns.cache') }}</th>
                  <th class="w-40 px-4 py-3 text-right">{{ t('diskCacheCleanup.ipsan.columns.actions') }}</th>
                </tr>
              </thead>
              <tbody class="divide-y divide-orange-50">
                <tr
                  v-for="item in ipsans"
                  :key="item.IPSANId"
                  class="hover:bg-orange-50/40 transition-colors"
                >
                  <td class="px-4 py-3">
                    <div class="font-medium text-slate-900">{{ item.IPSANName || item.IPSANIp || '--' }}</div>
                    <div class="mt-1 font-mono text-xs text-slate-400">{{ item.IPSANIp || '--' }}</div>
                  </td>
                  <td class="px-4 py-3 font-mono text-xs text-slate-700">
                    {{ item.IPSANId }}
                  </td>
                  <td class="px-4 py-3">
                    <span
                      class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold"
                      :class="statusBadgeClass(item.IPSANStatus)"
                    >
                      <span
                        class="h-1.5 w-1.5 rounded-full bg-current"
                        :class="{ 'animate-pulse': statusIsBusy(item.IPSANStatus) }"
                      ></span>
                      {{ t(statusLabelKey(item.IPSANStatus)) }}
                    </span>
                  </td>
                  <td class="px-4 py-3 text-right font-mono text-slate-700">
                    {{ formatCapacity(item.totalCapacity) }}
                  </td>
                  <td class="px-4 py-3">
                    <span
                      class="inline-flex items-center rounded-full border px-2.5 py-1 text-xs font-semibold"
                      :class="usageBadgeClass(item.usage)"
                    >
                      {{ t(usageLabelKey(item.usage)) }}
                    </span>
                  </td>
                  <td class="px-4 py-3">
                    <span
                      v-if="!ipsanRedisAvailable"
                      class="inline-flex items-center rounded-full border border-amber-200 bg-amber-50 px-2.5 py-1 text-xs font-semibold text-amber-700"
                    >
                      {{ t('diskCacheCleanup.cache.unavailable') }}
                    </span>
                    <div
                      v-else-if="ipsanPresentCacheKeys.has(ipsanCacheKey(item.IPSANId))"
                      class="space-y-2"
                    >
                      <span
                        class="inline-flex items-center rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 text-xs font-semibold text-indigo-700"
                      >
                        {{ t('diskCacheCleanup.cache.present') }}
                      </span>
                      <div class="max-w-[280px] rounded-xl border border-indigo-100 bg-indigo-50/60 px-3 py-2">
                        <div class="text-[10px] font-semibold uppercase tracking-[0.12em] text-indigo-700">
                          {{ cacheValueType(ipsanCacheContentEntry(ipsanCacheKey(item.IPSANId))) }}
                        </div>
                        <pre class="mt-1 whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-slate-700">{{ cachePreviewText(ipsanCacheContentEntry(ipsanCacheKey(item.IPSANId))) }}</pre>
                        <p
                          v-if="ipsanCacheContentEntry(ipsanCacheKey(item.IPSANId))?.truncated"
                          class="mt-1 text-[10px] text-slate-400"
                        >
                          {{ t('diskCacheCleanup.cache.truncated') }}
                        </p>
                      </div>
                    </div>
                    <span v-else class="text-sm text-slate-400">
                      {{ t('diskCacheCleanup.cache.absent') }}
                    </span>
                  </td>
                  <td class="px-4 py-3 text-right">
                    <button
                      v-if="ipsanPresentCacheKeys.has(ipsanCacheKey(item.IPSANId))"
                      type="button"
                      class="inline-flex items-center justify-center gap-2 rounded-xl bg-rose-500 px-3 py-2 text-xs font-semibold text-white transition hover:bg-rose-600 disabled:cursor-not-allowed disabled:bg-slate-300"
                      :disabled="!ipsanRedisAvailable || ipsanCleaningKeys.has(ipsanCacheKey(item.IPSANId))"
                      :title="!ipsanRedisAvailable ? t('diskCacheCleanup.disabled.redisDown') : undefined"
                      @click="cleanIpsanKeys([ipsanCacheKey(item.IPSANId)], ipsanCacheKey(item.IPSANId))"
                    >
                      <Loader
                        v-if="ipsanCleaningKeys.has(ipsanCacheKey(item.IPSANId))"
                        class="h-3.5 w-3.5 animate-spin"
                      />
                      <Trash2 v-else class="h-3.5 w-3.5" />
                      <span>{{ ipsanCleaningKeys.has(ipsanCacheKey(item.IPSANId)) ? t('diskCacheCleanup.actions.cleaningOne') : t('diskCacheCleanup.actions.cleanOne') }}</span>
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>
