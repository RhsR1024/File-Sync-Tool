<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import {
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  HardDrive,
  Info,
  Loader,
  RefreshCw,
  Server,
  Trash2,
  X,
} from 'lucide-vue-next';

import {
  diskCleanupCheckCacheKeys,
  diskCleanupDeleteCacheKeys,
  diskCleanupGetCacheKeyContents,
  diskCleanupListIpsans,
  diskCleanupListIpsanResourceGroups,
  diskCleanupListLinuxDisks,
  diskCleanupListLinuxServers,
  diskCleanupListMainlineServers,
  diskCleanupListWindowsDisks,
  getConfig,
  saveConfig,
  type AppConfig,
  type CacheKeyContentEntry,
  type DiskCleanupLinuxMode,
  type DiskInfoItem,
  type DiskServerItem,
  type IpsanItem,
  type IpsanResourceGroupItem,
  type WindowsDiskItem,
} from '../lib/tauri';
import { buildCacheDetailPreview } from '../lib/diskCacheCleanupCacheDetail';
import { getSuggestedDiskCleanupHosts } from '../lib/diskCacheCleanupPresentation';
import { mergeRecentItems, normalizeRecentItems } from '../lib/recentHistory';
import CacheStateInline from '../components/CacheStateInline.vue';
import Empty from '../components/Empty.vue';
import LoadingSkeleton from '../components/LoadingSkeleton.vue';
import { useToast } from '../composables/useToast';

defineOptions({
  name: 'DiskCacheCleanupPage',
});

type LocalDiskTab = 'windows' | 'linux';
type IpsanSubTab = 'devices' | 'resourceGroups';
type LegendTone = 'normal' | 'pending' | 'warning' | 'error';

const { t } = useI18n();
const { pushToast } = useToast();

const RECENT_KEY = 'diskCacheCleanup.recentHosts';
const MAX_RECENT_HOSTS = 10;
const TIMEOUT_OPTIONS = [1, 2, 3, 5, 10, 15, 30] as const;
const BATCH_CONFIRM_TIMEOUT_MS = 3000;
// Real appliances cap at ~24 disks per chassis and a small handful of IPSAN
// entries; virtualizing the cache key list would add complexity without a
// payoff. If a future device reports >100 entries, revisit this decision.

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
const windowsTabRef = ref<HTMLButtonElement | null>(null);
const linuxTabRef = ref<HTMLButtonElement | null>(null);
const ipsanSubTab = ref<IpsanSubTab>('devices');
const ipsanDevicesTabRef = ref<HTMLButtonElement | null>(null);
const ipsanResourceGroupsTabRef = ref<HTMLButtonElement | null>(null);
const legendOpen = ref(false);

const linuxMode = ref<DiskCleanupLinuxMode>('componentized');
const linuxModeComponentizedRef = ref<HTMLButtonElement | null>(null);
const linuxModeMainlineRef = ref<HTMLButtonElement | null>(null);
const linuxServerList = ref<DiskServerItem[]>([]);
const selectedLinuxServerIp = ref('');
const linuxDisks = ref<DiskInfoItem[]>([]);
const windowsDisks = ref<WindowsDiskItem[]>([]);
const localExpandedIds = ref<Set<string>>(new Set());
const localPresentCacheKeys = ref<Set<string>>(new Set());
const localCleaningKeys = ref<Set<string>>(new Set());
const localLoading = ref(false);
const localBatchCleaning = ref(false);
const localBatchPendingCount = ref(0);
const localBatchConfirmPending = ref(false);
const localError = ref<string | null>(null);
const localRedisAvailable = ref(true);
const localRedisError = ref<string | null>(null);
const hasFetchedLocal = ref(false);

const ipsans = ref<IpsanItem[]>([]);
const ipsanResourceGroups = ref<IpsanResourceGroupItem[]>([]);
const ipsanExpandedGroupIds = ref<Set<string>>(new Set());
const highlightedIpsanResourceGroupId = ref('');
const ipsanPresentCacheKeys = ref<Set<string>>(new Set());
const ipsanCleaningKeys = ref<Set<string>>(new Set());
const ipsanLoading = ref(false);
const ipsanBatchCleaning = ref(false);
const ipsanBatchPendingCount = ref(0);
const ipsanBatchConfirmPending = ref(false);
const ipsanError = ref<string | null>(null);
const ipsanRedisAvailable = ref(true);
const ipsanRedisError = ref<string | null>(null);
const ipsanResourceGroupPresentCacheKeys = ref<Set<string>>(new Set());
const ipsanResourceGroupCleaningKeys = ref<Set<string>>(new Set());
const ipsanResourceGroupBatchCleaning = ref(false);
const ipsanResourceGroupBatchPendingCount = ref(0);
const ipsanResourceGroupBatchConfirmPending = ref(false);
const ipsanResourceGroupError = ref<string | null>(null);
const ipsanResourceGroupRedisAvailable = ref(true);
const ipsanResourceGroupRedisError = ref<string | null>(null);
const hasFetchedIpsan = ref(false);
const cacheDetailOpen = ref(false);
const cacheDetailLoading = ref(false);
const cacheDetailError = ref<string | null>(null);
const cacheDetailKey = ref('');
const cacheDetailEntry = ref<CacheKeyContentEntry | null>(null);
const cacheDetailModalRef = ref<HTMLDivElement | null>(null);

let localRequestSeq = 0;
let ipsanRequestSeq = 0;
let cacheDetailRequestSeq = 0;
let localBatchConfirmTimer: ReturnType<typeof setTimeout> | null = null;
let ipsanBatchConfirmTimer: ReturnType<typeof setTimeout> | null = null;
let ipsanResourceGroupBatchConfirmTimer: ReturnType<typeof setTimeout> | null = null;

const savedSshHosts = computed(() => {
  return getSuggestedDiskCleanupHosts(config.value?.servers, recentHosts.value);
});

const selectedLinuxServer = computed(
  () => linuxServerList.value.find((item) => item.serverIp === selectedLinuxServerIp.value) ?? null,
);

const ipsanItemById = computed(() =>
  new Map(ipsans.value.map((item) => [item.IPSANId, item])),
);

const ipsanResourceGroupMetaByMemberId = computed(() => {
  const map = new Map<string, {
    groupId: string;
    groupName: string;
    memberCount: number;
    groupStatus: number;
    usage: number;
  }>();

  for (const group of ipsanResourceGroups.value) {
    const memberCount = group.resourceInfoList?.length ?? 0;
    for (const member of group.resourceInfoList ?? []) {
      if (map.has(member.IPSANId)) continue;
      map.set(member.IPSANId, {
        groupId: group.groupId,
        groupName: group.groupName,
        memberCount,
        groupStatus: group.groupStatus,
        usage: group.usage,
      });
    }
  }

  return map;
});

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

const initialFetchPending = computed(
  () => fetchAllLoading.value && !hasFetchedLocal.value && !hasFetchedIpsan.value,
);

const legendEntries = computed<Array<{
  key: LegendTone;
  label: string;
  badgeClass: string;
  codes: number[];
}>>(() => [
  {
    key: 'normal',
    label: t('diskCacheCleanup.legend.normal'),
    badgeClass: 'border-emerald-200 bg-emerald-50 text-emerald-700',
    codes: [...STATUS_GREEN].sort((a, b) => a - b),
  },
  {
    key: 'pending',
    label: t('diskCacheCleanup.legend.pending'),
    badgeClass: 'border-sky-200 bg-sky-50 text-sky-700',
    codes: [...STATUS_BLUE].sort((a, b) => a - b),
  },
  {
    key: 'warning',
    label: t('diskCacheCleanup.legend.warning'),
    badgeClass: 'border-amber-200 bg-amber-50 text-amber-700',
    codes: [...STATUS_AMBER].sort((a, b) => a - b),
  },
  {
    key: 'error',
    label: t('diskCacheCleanup.legend.error'),
    badgeClass: 'border-rose-200 bg-rose-50 text-rose-700',
    codes: [...STATUS_RED].sort((a, b) => a - b),
  },
]);

const localRowCount = computed(() => {
  if (localDiskTab.value === 'windows') {
    return windowsDisks.value.reduce((sum, disk) => sum + disk.partitionList.length, 0);
  }
  return linuxDisks.value.length;
});

const localCachedCount = computed(() => localCleanableKeys.value.length);
const ipsanCachedCount = computed(() => ipsanCleanableKeys.value.length);
const ipsanResourceGroupCachedCount = computed(() => ipsanResourceGroupCleanableKeys.value.length);
const ipsanTotalCachedCount = computed(
  () => ipsanCachedCount.value + ipsanResourceGroupCachedCount.value,
);
const ipsanGroupedCount = computed(() =>
  ipsans.value.filter((item) => ipsanResourceGroupMetaByMemberId.value.has(item.IPSANId)).length,
);
const ipsanUngroupedCount = computed(() =>
  Math.max(ipsans.value.length - ipsanGroupedCount.value, 0),
);

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
  return t('diskCacheCleanup.ipsan.summary', {
    devices: ipsans.value.length,
    groups: ipsanResourceGroups.value.length,
    grouped: ipsanGroupedCount.value,
    ungrouped: ipsanUngroupedCount.value,
    cachedDevices: ipsanCachedCount.value,
    cachedGroups: ipsanResourceGroupCachedCount.value,
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
const ipsanResourceGroupCleanableKeys = computed(() =>
  ipsanResourceGroupCacheKeys(ipsanResourceGroups.value)
    .filter((key) => ipsanResourceGroupPresentCacheKeys.value.has(key)),
);
const activeIpsanRedisAvailable = computed(() =>
  ipsanSubTab.value === 'devices'
    ? ipsanRedisAvailable.value
    : ipsanResourceGroupRedisAvailable.value,
);
const activeIpsanRedisError = computed(() =>
  ipsanSubTab.value === 'devices'
    ? ipsanRedisError.value
    : ipsanResourceGroupRedisError.value,
);
const activeIpsanBatchCleaning = computed(() =>
  ipsanSubTab.value === 'devices'
    ? ipsanBatchCleaning.value
    : ipsanResourceGroupBatchCleaning.value,
);
const activeIpsanBatchPendingCount = computed(() =>
  ipsanSubTab.value === 'devices'
    ? ipsanBatchPendingCount.value
    : ipsanResourceGroupBatchPendingCount.value,
);
const activeIpsanBatchConfirmPending = computed(() =>
  ipsanSubTab.value === 'devices'
    ? ipsanBatchConfirmPending.value
    : ipsanResourceGroupBatchConfirmPending.value,
);
const activeIpsanCleanableCount = computed(() =>
  ipsanSubTab.value === 'devices'
    ? ipsanCleanableKeys.value.length
    : ipsanResourceGroupCleanableKeys.value.length,
);
const activeIpsanCleanAllLabel = computed(() =>
  ipsanSubTab.value === 'devices'
    ? t('diskCacheCleanup.ipsan.actions.cleanAll', { count: activeIpsanCleanableCount.value })
    : t('diskCacheCleanup.ipsan.resourceGroups.actions.cleanAll', {
      count: activeIpsanCleanableCount.value,
    }),
);
const cacheDetailPreview = computed(() => {
  const entry = cacheDetailEntry.value;
  if (!entry) {
    return t('diskCacheCleanup.cache.emptyContent');
  }

  const preview = buildCacheDetailPreview(entry.full_value, entry.preview);
  return preview || t('diskCacheCleanup.cache.emptyContent');
});
const cacheDetailValue = computed(() =>
  cacheDetailEntry.value?.full_value || t('diskCacheCleanup.cache.emptyContent'),
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

function ipsanResourceGroupCacheKey(groupId: string) {
  return `Storage:${groupId}`;
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

function ipsanResourceGroupCacheKeys(items: IpsanResourceGroupItem[]) {
  return items.map((item) => ipsanResourceGroupCacheKey(item.groupId));
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

function resetLocalCacheState() {
  localExpandedIds.value = new Set();
  localPresentCacheKeys.value = new Set();
  localCleaningKeys.value = new Set();
  localRedisAvailable.value = true;
  localRedisError.value = null;
}

function resetIpsanCacheState() {
  ipsanPresentCacheKeys.value = new Set();
  ipsanCleaningKeys.value = new Set();
  ipsanRedisAvailable.value = true;
  ipsanRedisError.value = null;
  ipsanResourceGroupPresentCacheKeys.value = new Set();
  ipsanResourceGroupCleaningKeys.value = new Set();
  ipsanResourceGroupRedisAvailable.value = true;
  ipsanResourceGroupRedisError.value = null;
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
  } catch (error) {
    if (requestSeq !== localRequestSeq) return;
    linuxDisks.value = [];
    localPresentCacheKeys.value = new Set();
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
    const servers = linuxMode.value === 'mainline'
      ? await diskCleanupListMainlineServers(host, timeoutSecs.value)
      : await diskCleanupListLinuxServers(host, timeoutSecs.value);
    if (requestSeq !== localRequestSeq) return;

    linuxServerList.value = servers;
    const nextServerIp = servers.find((item) => item.serverIp === selectedLinuxServerIp.value)?.serverIp
      ?? servers[0]?.serverIp
      ?? '';
    selectedLinuxServerIp.value = nextServerIp;

    if (!nextServerIp) {
      linuxDisks.value = [];
      localPresentCacheKeys.value = new Set();
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
  } catch (error) {
    if (requestSeq !== localRequestSeq) return;
    windowsDisks.value = [];
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
  ipsanResourceGroupError.value = null;
  highlightedIpsanResourceGroupId.value = '';
  resetIpsanCacheState();

  try {
    const [itemsResult, resourceGroupsResult] = await Promise.allSettled([
      diskCleanupListIpsans(host, timeoutSecs.value),
      diskCleanupListIpsanResourceGroups(host, timeoutSecs.value),
    ]);
    if (requestSeq !== ipsanRequestSeq) return;

    if (itemsResult.status === 'rejected') {
      ipsans.value = [];
      ipsanResourceGroups.value = [];
      ipsanPresentCacheKeys.value = new Set();
      ipsanResourceGroupPresentCacheKeys.value = new Set();
      ipsanError.value = t('diskCacheCleanup.errors.ipsanHttp', {
        reason: formatError(itemsResult.reason),
      });
      return;
    }

    const items = itemsResult.value;
    ipsans.value = items;
    ipsanResourceGroups.value = resourceGroupsResult.status === 'fulfilled'
      ? resourceGroupsResult.value
      : [];
    ipsanExpandedGroupIds.value = new Set();

    if (resourceGroupsResult.status === 'rejected') {
      ipsanResourceGroupError.value = t('diskCacheCleanup.errors.ipsanResourceGroupHttp', {
        reason: formatError(resourceGroupsResult.reason),
      });
    }

    const [ipsanCacheResult, ipsanResourceGroupCacheResult] = await Promise.all([
      diskCleanupCheckCacheKeys(host, ipsanCacheKeys(items)),
      diskCleanupCheckCacheKeys(
        host,
        ipsanResourceGroupCacheKeys(ipsanResourceGroups.value),
      ),
    ]);
    if (requestSeq !== ipsanRequestSeq) return;

    ipsanRedisAvailable.value = ipsanCacheResult.redis_available;
    ipsanRedisError.value = ipsanCacheResult.error;
    ipsanPresentCacheKeys.value = new Set(ipsanCacheResult.present_keys ?? []);
    ipsanResourceGroupRedisAvailable.value = ipsanResourceGroupCacheResult.redis_available;
    ipsanResourceGroupRedisError.value = ipsanResourceGroupCacheResult.error;
    ipsanResourceGroupPresentCacheKeys.value = new Set(
      ipsanResourceGroupCacheResult.present_keys ?? [],
    );
  } catch (error) {
    if (requestSeq !== ipsanRequestSeq) return;
    ipsans.value = [];
    ipsanResourceGroups.value = [];
    ipsanPresentCacheKeys.value = new Set();
    ipsanResourceGroupPresentCacheKeys.value = new Set();
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

function toggleIpsanResourceGroupExpanded(groupId: string) {
  const next = new Set(ipsanExpandedGroupIds.value);
  if (next.has(groupId)) {
    next.delete(groupId);
  } else {
    next.add(groupId);
  }
  ipsanExpandedGroupIds.value = next;
}

function jumpToIpsanResourceGroup(groupId: string) {
  ipsanSubTab.value = 'resourceGroups';
  highlightedIpsanResourceGroupId.value = groupId;
  const next = new Set(ipsanExpandedGroupIds.value);
  next.add(groupId);
  ipsanExpandedGroupIds.value = next;
}

function onIpsanSubTabKeydown(event: KeyboardEvent, currentTab: IpsanSubTab) {
  const order: IpsanSubTab[] = ['devices', 'resourceGroups'];
  const currentIndex = order.indexOf(currentTab);
  let nextIndex = currentIndex;
  if (event.key === 'ArrowRight' || event.key === 'ArrowDown') {
    nextIndex = (currentIndex + 1) % order.length;
  } else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') {
    nextIndex = (currentIndex - 1 + order.length) % order.length;
  } else if (event.key === 'Home') {
    nextIndex = 0;
  } else if (event.key === 'End') {
    nextIndex = order.length - 1;
  } else {
    return;
  }
  event.preventDefault();
  const nextTab = order[nextIndex];
  ipsanSubTab.value = nextTab;
  highlightedIpsanResourceGroupId.value = '';
  nextTick(() => {
    const nextRef = nextTab === 'devices'
      ? ipsanDevicesTabRef.value
      : ipsanResourceGroupsTabRef.value;
  nextRef?.focus();
  });
}

function ipsanGroupMetaFor(ipsanId: string) {
  return ipsanResourceGroupMetaByMemberId.value.get(ipsanId) ?? null;
}

function ipsanResourceGroupTitle(group: IpsanResourceGroupItem) {
  return group.groupName || group.groupId || '--';
}

function ipsanMemberReturned(memberId: string) {
  return ipsans.value.some((item) => item.IPSANId === memberId);
}

function ipsanItemFor(memberId: string) {
  return ipsanItemById.value.get(memberId) ?? null;
}

function closeCacheDetail() {
  cacheDetailRequestSeq += 1;
  cacheDetailOpen.value = false;
  cacheDetailLoading.value = false;
  cacheDetailError.value = null;
  cacheDetailKey.value = '';
  cacheDetailEntry.value = null;
}

async function openCacheDetail(key: string) {
  const host = hostIp.value.trim();
  if (!host) return;

  const requestSeq = ++cacheDetailRequestSeq;
  cacheDetailOpen.value = true;
  cacheDetailLoading.value = true;
  cacheDetailError.value = null;
  cacheDetailKey.value = key;
  cacheDetailEntry.value = null;

  try {
    const result = await diskCleanupGetCacheKeyContents(host, [key]);
    if (requestSeq !== cacheDetailRequestSeq) return;

    if (!result.redis_available || result.error) {
      cacheDetailError.value = t('diskCacheCleanup.errors.cacheDetail', {
        reason: result.error ?? t('diskCacheCleanup.cache.unavailable'),
      });
      return;
    }

    const entry = result.entries?.[0] ?? null;
    if (!entry) {
      cacheDetailError.value = t('diskCacheCleanup.errors.cacheDetail', {
        reason: t('diskCacheCleanup.cache.notFound'),
      });
      return;
    }

    cacheDetailEntry.value = entry;
  } catch (error) {
    if (requestSeq !== cacheDetailRequestSeq) return;
    cacheDetailError.value = t('diskCacheCleanup.errors.cacheDetail', {
      reason: formatError(error),
    });
  } finally {
    if (requestSeq === cacheDetailRequestSeq) {
      cacheDetailLoading.value = false;
    }
  }
}

async function cleanLocalKeys(keys: string[], singleKey?: string) {
  const host = hostIp.value.trim();
  if (!host || keys.length === 0) return;

  if (singleKey) {
    const next = new Set(localCleaningKeys.value);
    next.add(singleKey);
    localCleaningKeys.value = next;
  } else {
    if (!localBatchConfirmPending.value) {
      armLocalBatchConfirm();
      return;
    }
    cancelLocalBatchConfirm();
    localBatchCleaning.value = true;
    localBatchPendingCount.value = keys.length;
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

    if (cacheDetailEntry.value && keys.includes(cacheDetailEntry.value.key)) {
      closeCacheDetail();
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
      localBatchPendingCount.value = 0;
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
    if (!ipsanBatchConfirmPending.value) {
      armIpsanBatchConfirm();
      return;
    }
    cancelIpsanBatchConfirm();
    ipsanBatchCleaning.value = true;
    ipsanBatchPendingCount.value = keys.length;
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

    if (cacheDetailEntry.value && keys.includes(cacheDetailEntry.value.key)) {
      closeCacheDetail();
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
      ipsanBatchPendingCount.value = 0;
    }
  }
}

async function cleanIpsanResourceGroupKeys(keys: string[], singleKey?: string) {
  const host = hostIp.value.trim();
  if (!host || keys.length === 0) return;

  if (singleKey) {
    const next = new Set(ipsanResourceGroupCleaningKeys.value);
    next.add(singleKey);
    ipsanResourceGroupCleaningKeys.value = next;
  } else {
    if (!ipsanResourceGroupBatchConfirmPending.value) {
      armIpsanResourceGroupBatchConfirm();
      return;
    }
    cancelIpsanResourceGroupBatchConfirm();
    ipsanResourceGroupBatchCleaning.value = true;
    ipsanResourceGroupBatchPendingCount.value = keys.length;
  }

  ipsanResourceGroupError.value = null;

  try {
    const result = await diskCleanupDeleteCacheKeys(host, keys);
    if (!result.redis_available || result.error) {
      ipsanResourceGroupRedisAvailable.value = result.redis_available;
      ipsanResourceGroupRedisError.value = result.error;
      ipsanResourceGroupError.value = t('diskCacheCleanup.errors.ipsanResourceGroupDelete', {
        reason: result.error ?? t('diskCacheCleanup.cache.unavailable'),
      });
      return;
    }

    if (cacheDetailEntry.value && keys.includes(cacheDetailEntry.value.key)) {
      closeCacheDetail();
    }

    await fetchIpsanRegion();
  } catch (error) {
    ipsanResourceGroupError.value = t('diskCacheCleanup.errors.ipsanResourceGroupDelete', {
      reason: formatError(error),
    });
  } finally {
    if (singleKey) {
      const next = new Set(ipsanResourceGroupCleaningKeys.value);
      next.delete(singleKey);
      ipsanResourceGroupCleaningKeys.value = next;
    } else {
      ipsanResourceGroupBatchCleaning.value = false;
      ipsanResourceGroupBatchPendingCount.value = 0;
    }
  }
}

function handleActiveIpsanBatchClean() {
  if (ipsanSubTab.value === 'devices') {
    void cleanIpsanKeys(ipsanCleanableKeys.value);
    return;
  }
  void cleanIpsanResourceGroupKeys(ipsanResourceGroupCleanableKeys.value);
}

function armLocalBatchConfirm() {
  cancelIpsanBatchConfirm();
  localBatchConfirmPending.value = true;
  if (localBatchConfirmTimer) {
    clearTimeout(localBatchConfirmTimer);
  }
  localBatchConfirmTimer = setTimeout(() => {
    localBatchConfirmPending.value = false;
    localBatchConfirmTimer = null;
  }, BATCH_CONFIRM_TIMEOUT_MS);
}

function cancelLocalBatchConfirm() {
  if (localBatchConfirmTimer) {
    clearTimeout(localBatchConfirmTimer);
    localBatchConfirmTimer = null;
  }
  localBatchConfirmPending.value = false;
}

function armIpsanBatchConfirm() {
  cancelLocalBatchConfirm();
  cancelIpsanResourceGroupBatchConfirm();
  ipsanBatchConfirmPending.value = true;
  if (ipsanBatchConfirmTimer) {
    clearTimeout(ipsanBatchConfirmTimer);
  }
  ipsanBatchConfirmTimer = setTimeout(() => {
    ipsanBatchConfirmPending.value = false;
    ipsanBatchConfirmTimer = null;
  }, BATCH_CONFIRM_TIMEOUT_MS);
}

function cancelIpsanBatchConfirm() {
  if (ipsanBatchConfirmTimer) {
    clearTimeout(ipsanBatchConfirmTimer);
    ipsanBatchConfirmTimer = null;
  }
  ipsanBatchConfirmPending.value = false;
}

function armIpsanResourceGroupBatchConfirm() {
  cancelLocalBatchConfirm();
  cancelIpsanBatchConfirm();
  ipsanResourceGroupBatchConfirmPending.value = true;
  if (ipsanResourceGroupBatchConfirmTimer) {
    clearTimeout(ipsanResourceGroupBatchConfirmTimer);
  }
  ipsanResourceGroupBatchConfirmTimer = setTimeout(() => {
    ipsanResourceGroupBatchConfirmPending.value = false;
    ipsanResourceGroupBatchConfirmTimer = null;
  }, BATCH_CONFIRM_TIMEOUT_MS);
}

function cancelIpsanResourceGroupBatchConfirm() {
  if (ipsanResourceGroupBatchConfirmTimer) {
    clearTimeout(ipsanResourceGroupBatchConfirmTimer);
    ipsanResourceGroupBatchConfirmTimer = null;
  }
  ipsanResourceGroupBatchConfirmPending.value = false;
}

async function clearRecentHosts() {
  cancelLocalBatchConfirm();
  cancelIpsanBatchConfirm();
  cancelIpsanResourceGroupBatchConfirm();
  await persistRecentHosts([]);
  pushToast(t('diskCacheCleanup.recent.clear'), 'info');
}

function toggleLegend() {
  legendOpen.value = !legendOpen.value;
}

function onTabKeydown(event: KeyboardEvent, currentTab: LocalDiskTab) {
  const order: LocalDiskTab[] = ['windows', 'linux'];
  const currentIndex = order.indexOf(currentTab);
  let nextIndex = currentIndex;
  if (event.key === 'ArrowRight' || event.key === 'ArrowDown') {
    nextIndex = (currentIndex + 1) % order.length;
  } else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') {
    nextIndex = (currentIndex - 1 + order.length) % order.length;
  } else if (event.key === 'Home') {
    nextIndex = 0;
  } else if (event.key === 'End') {
    nextIndex = order.length - 1;
  } else {
    return;
  }
  event.preventDefault();
  const nextTab = order[nextIndex];
  localDiskTab.value = nextTab;
  nextTick(() => {
    const nextRef = nextTab === 'windows' ? windowsTabRef.value : linuxTabRef.value;
    nextRef?.focus();
  });
}

function onModalKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    closeCacheDetail();
  }
}

function onLinuxModeKeydown(event: KeyboardEvent, currentMode: DiskCleanupLinuxMode) {
  const order: DiskCleanupLinuxMode[] = ['componentized', 'mainline'];
  const currentIndex = order.indexOf(currentMode);
  let nextIndex = currentIndex;
  if (event.key === 'ArrowRight' || event.key === 'ArrowDown') {
    nextIndex = (currentIndex + 1) % order.length;
  } else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') {
    nextIndex = (currentIndex - 1 + order.length) % order.length;
  } else if (event.key === 'Home') {
    nextIndex = 0;
  } else if (event.key === 'End') {
    nextIndex = order.length - 1;
  } else {
    return;
  }
  event.preventDefault();
  const nextMode = order[nextIndex];
  selectLinuxMode(nextMode);
  nextTick(() => {
    const nextRef = nextMode === 'componentized'
      ? linuxModeComponentizedRef.value
      : linuxModeMainlineRef.value;
    nextRef?.focus();
  });
}

async function selectLinuxMode(nextMode: DiskCleanupLinuxMode) {
  if (linuxMode.value === nextMode) return;

  linuxMode.value = nextMode;

  // Reset server selection so the previous mode's IP doesn't leak across endpoints.
  selectedLinuxServerIp.value = '';
  linuxServerList.value = [];
  linuxDisks.value = [];
  resetLocalCacheState();

  if (config.value) {
    config.value.disk_cleanup_linux_mode = nextMode;
    try {
      await saveConfig(config.value);
    } catch (error) {
      localError.value = t('diskCacheCleanup.errors.localHttp', {
        reason: formatError(error),
      });
    }
  }

  cancelLocalBatchConfirm();
  if (hasFetchedLocal.value && hostIp.value.trim() && localDiskTab.value === 'linux') {
    await fetchLocalRegion();
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
  cancelLocalBatchConfirm();
  if (!hasFetchedLocal.value || !hostIp.value.trim()) return;
  await fetchLocalRegion();
});

watch(ipsanSubTab, () => {
  cancelIpsanBatchConfirm();
  cancelIpsanResourceGroupBatchConfirm();
  if (ipsanSubTab.value !== 'resourceGroups') {
    highlightedIpsanResourceGroupId.value = '';
  }
});

watch(cacheDetailOpen, async (isOpen) => {
  if (!isOpen) return;
  await nextTick();
  cacheDetailModalRef.value?.focus();
});

onMounted(async () => {
  try {
    const loaded = await getConfig();
    config.value = loaded;
    timeoutSecs.value = Number(config.value.disk_cleanup_http_timeout_secs ?? 5);
    linuxMode.value = config.value.disk_cleanup_linux_mode === 'mainline'
      ? 'mainline'
      : 'componentized';
  } catch (error) {
    localError.value = t('diskCacheCleanup.errors.localHttp', {
      reason: formatError(error),
    });
  }

  await loadRecentHosts();
});

onBeforeUnmount(() => {
  cancelLocalBatchConfirm();
  cancelIpsanBatchConfirm();
  cancelIpsanResourceGroupBatchConfirm();
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
              <div class="mt-2 font-mono text-2xl font-bold text-slate-900">{{ ipsanTotalCachedCount }}</div>
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
                <div class="mb-2 flex items-center justify-between gap-2">
                  <p class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                    {{ t('diskCacheCleanup.hostIp.recentGroup') }}
                  </p>
                  <button
                    type="button"
                    class="inline-flex items-center gap-1 rounded-full border border-slate-200 bg-white px-2.5 py-1 text-[11px] font-semibold text-slate-500 transition hover:border-rose-200 hover:text-rose-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40 focus-visible:ring-offset-1"
                    :title="t('diskCacheCleanup.recent.clear')"
                    :aria-label="t('diskCacheCleanup.recent.clear')"
                    @click="clearRecentHosts"
                  >
                    <X class="h-3 w-3" aria-hidden="true" />
                    <span>{{ t('diskCacheCleanup.recent.clear') }}</span>
                  </button>
                </div>
                <div class="flex flex-wrap gap-2">
                  <button
                    v-for="item in recentHosts"
                    :key="`recent-${item}`"
                    type="button"
                    class="rounded-full border border-sky-200 bg-sky-50 px-3 py-1.5 font-mono text-xs text-sky-700 transition hover:border-sky-300 hover:bg-sky-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/40 focus-visible:ring-offset-1"
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
                    class="rounded-full border border-slate-200 bg-slate-50 px-3 py-1.5 font-mono text-xs text-slate-700 transition hover:border-slate-300 hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40 focus-visible:ring-offset-1"
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
              <div
                class="mt-2 inline-flex gap-1 rounded-full border border-slate-200 bg-slate-100 p-1"
                role="tablist"
                :aria-label="t('diskCacheCleanup.localDisk.title')"
              >
                <button
                  ref="windowsTabRef"
                  id="local-disk-tab-windows"
                  type="button"
                  role="tab"
                  :aria-selected="localDiskTab === 'windows'"
                  :tabindex="localDiskTab === 'windows' ? 0 : -1"
                  aria-controls="local-disk-panel"
                  class="rounded-full px-4 py-2 text-sm font-semibold transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50"
                  :class="localDiskTab === 'windows' ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-500 hover:text-slate-700'"
                  @click="localDiskTab = 'windows'"
                  @keydown="onTabKeydown($event, 'windows')"
                >
                  {{ t('diskCacheCleanup.localDisk.tabs.windows') }}
                </button>
                <button
                  ref="linuxTabRef"
                  id="local-disk-tab-linux"
                  type="button"
                  role="tab"
                  :aria-selected="localDiskTab === 'linux'"
                  :tabindex="localDiskTab === 'linux' ? 0 : -1"
                  aria-controls="local-disk-panel"
                  class="rounded-full px-4 py-2 text-sm font-semibold transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50"
                  :class="localDiskTab === 'linux' ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-500 hover:text-slate-700'"
                  @click="localDiskTab = 'linux'"
                  @keydown="onTabKeydown($event, 'linux')"
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
              class="inline-flex items-center gap-1.5 rounded-2xl border border-slate-200 bg-white px-3 py-2.5 text-xs font-semibold text-slate-600 transition hover:border-slate-300 hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40 focus-visible:ring-offset-1"
              :aria-expanded="legendOpen"
              :aria-label="legendOpen ? t('diskCacheCleanup.legend.toggleHide') : t('diskCacheCleanup.legend.toggleShow')"
              :title="legendOpen ? t('diskCacheCleanup.legend.toggleHide') : t('diskCacheCleanup.legend.toggleShow')"
              @click="toggleLegend"
            >
              <Info class="h-3.5 w-3.5" aria-hidden="true" />
              <span>{{ t('diskCacheCleanup.legend.title') }}</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 transition hover:border-slate-300 hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40 focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:opacity-50"
              :disabled="!hostIp.trim() || localLoading"
              @click="handleRefreshLocal"
            >
              <RefreshCw class="h-4 w-4" :class="{ 'animate-spin': localLoading }" />
              {{ t('diskCacheCleanup.localDisk.actions.refresh') }}
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-2xl px-4 py-2.5 text-sm font-semibold text-white transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-rose-500/40 focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:bg-slate-300"
              :class="localRedisAvailable && localCleanableKeys.length > 0
                ? (localBatchConfirmPending ? 'bg-rose-600 hover:bg-rose-700 ring-2 ring-rose-300 ring-offset-1' : 'bg-rose-500 hover:bg-rose-600')
                : 'bg-slate-300'"
              :disabled="!localRedisAvailable || localCleanableKeys.length === 0 || localBatchCleaning"
              :title="!localRedisAvailable ? t('diskCacheCleanup.disabled.redisDown') : (localBatchConfirmPending ? t('diskCacheCleanup.batch.confirmAria') : undefined)"
              :aria-pressed="localBatchConfirmPending"
              @click="cleanLocalKeys(localCleanableKeys)"
            >
              <Loader v-if="localBatchCleaning" class="h-4 w-4 animate-spin" />
              <Trash2 v-else class="h-4 w-4" />
              <span v-if="localBatchCleaning">{{ t('diskCacheCleanup.batch.progress', { count: localBatchPendingCount }) }}</span>
              <span v-else-if="localBatchConfirmPending">{{ t('diskCacheCleanup.batch.confirm') }}</span>
              <span v-else>{{ t('diskCacheCleanup.localDisk.actions.cleanAll', { count: localCleanableKeys.length }) }}</span>
            </button>
          </div>
        </div>

        <div
          v-if="legendOpen"
          class="border-b border-slate-200/80 bg-slate-50/60 px-5 py-4"
          aria-live="polite"
        >
          <div class="flex flex-wrap items-start gap-3">
            <div class="flex flex-wrap items-start gap-3">
              <div
                v-for="entry in legendEntries"
                :key="entry.key"
                class="flex flex-col gap-1.5 rounded-2xl border border-slate-200 bg-white px-3 py-2"
              >
                <span
                  class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold"
                  :class="entry.badgeClass"
                >
                  <span class="h-1.5 w-1.5 rounded-full bg-current" aria-hidden="true"></span>
                  {{ entry.label }}
                </span>
                <span class="text-[11px] text-slate-500">
                  {{ t('diskCacheCleanup.legend.codes') }}: {{ entry.codes.join(', ') }}
                </span>
              </div>
            </div>
          </div>
        </div>

        <div
          id="local-disk-panel"
          role="tabpanel"
          :aria-labelledby="`local-disk-tab-${localDiskTab}`"
          class="space-y-4 p-5"
        >
          <div
            v-if="localError"
            class="flex items-start gap-3 rounded-[20px] border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700 shadow-sm"
            role="alert"
          >
            <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
            <span>{{ localError }}</span>
          </div>

          <div
            v-if="localRedisError && !localRedisAvailable"
            class="flex items-start gap-3 rounded-[20px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800 shadow-sm"
            role="status"
          >
            <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
            <span>{{ localRedisError }}</span>
          </div>

          <section
            v-if="localDiskTab === 'linux'"
            class="rounded-[20px] border border-slate-200 bg-slate-50/70 px-4 py-3"
          >
            <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div class="space-y-1">
                <p class="text-sm font-semibold text-slate-800">
                  {{ t('diskCacheCleanup.linuxMode.title') }}
                </p>
                <p class="text-xs text-slate-500">
                  {{ linuxMode === 'componentized'
                    ? t('diskCacheCleanup.linuxMode.descriptions.componentized')
                    : t('diskCacheCleanup.linuxMode.descriptions.mainline') }}
                </p>
              </div>
              <div
                class="inline-flex rounded-full border border-slate-200 bg-white p-1 shadow-sm"
                role="tablist"
                :aria-label="t('diskCacheCleanup.linuxMode.title')"
              >
                <button
                  ref="linuxModeComponentizedRef"
                  type="button"
                  role="tab"
                  :aria-selected="linuxMode === 'componentized'"
                  :tabindex="linuxMode === 'componentized' ? 0 : -1"
                  :disabled="localLoading"
                  class="rounded-full px-3 py-1.5 text-xs font-semibold transition disabled:cursor-not-allowed disabled:opacity-60"
                  :class="linuxMode === 'componentized' ? 'bg-sky-600 text-white shadow' : 'text-slate-600 hover:text-slate-900'"
                  @click="selectLinuxMode('componentized')"
                  @keydown="onLinuxModeKeydown($event, 'componentized')"
                >
                  {{ t('diskCacheCleanup.linuxMode.componentized') }}
                </button>
                <button
                  ref="linuxModeMainlineRef"
                  type="button"
                  role="tab"
                  :aria-selected="linuxMode === 'mainline'"
                  :tabindex="linuxMode === 'mainline' ? 0 : -1"
                  :disabled="localLoading"
                  class="rounded-full px-3 py-1.5 text-xs font-semibold transition disabled:cursor-not-allowed disabled:opacity-60"
                  :class="linuxMode === 'mainline' ? 'bg-sky-600 text-white shadow' : 'text-slate-600 hover:text-slate-900'"
                  @click="selectLinuxMode('mainline')"
                  @keydown="onLinuxModeKeydown($event, 'mainline')"
                >
                  {{ t('diskCacheCleanup.linuxMode.mainline') }}
                </button>
              </div>
            </div>
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

          <Empty
            v-if="!hasFetchedLocal && !localLoading && !hostIp.trim()"
            :icon="HardDrive"
            :title="t('diskCacheCleanup.empty.noHost')"
            :description="t('diskCacheCleanup.empty.noHostHint')"
            class="min-h-[240px]"
          />

          <Empty
            v-else-if="!hasFetchedLocal && !localLoading"
            :icon="HardDrive"
            :title="t('diskCacheCleanup.disks.emptyIdle')"
            :description="t('diskCacheCleanup.disks.emptyIdleHint')"
            :action-label="t('diskCacheCleanup.actions.fetch')"
            class="min-h-[240px]"
            @action="handleFetchAll"
          />

          <div
            v-else-if="initialFetchPending && localRowCount === 0"
            class="rounded-[20px] border border-dashed border-slate-200 bg-slate-50/60 px-5 py-6"
            role="status"
            aria-live="polite"
          >
            <p class="mb-3 text-sm font-semibold text-slate-700">{{ t('diskCacheCleanup.disks.loading') }}</p>
            <LoadingSkeleton variant="list-row" :count="4" />
          </div>

          <div
            v-else-if="localLoading && localRowCount === 0 && (localDiskTab === 'windows' || linuxServerList.length === 0)"
            class="flex min-h-[240px] flex-col items-center justify-center rounded-[20px] border border-dashed border-slate-200 bg-slate-50/60 px-6 py-8 text-center"
            role="status"
            aria-live="polite"
          >
            <Loader class="h-7 w-7 animate-spin text-sky-600" aria-hidden="true" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.disks.loading') }}</p>
            <p class="mt-2 text-sm text-slate-500">{{ t('diskCacheCleanup.disks.loadingHint') }}</p>
          </div>

          <Empty
            v-else-if="localDiskTab === 'linux' && hasFetchedLocal && linuxServerList.length === 0 && !localLoading"
            :icon="Server"
            :title="t('diskCacheCleanup.server.empty')"
            :description="t('diskCacheCleanup.server.emptyHint')"
            class="min-h-[240px]"
          />

          <Empty
            v-else-if="localDiskTab === 'linux' && linuxDisks.length === 0 && !localLoading"
            :icon="HardDrive"
            :title="t('diskCacheCleanup.disks.empty')"
            :description="t('diskCacheCleanup.disks.emptyHint')"
            class="min-h-[240px]"
          />

          <Empty
            v-else-if="localDiskTab === 'windows' && windowsDisks.length === 0 && !localLoading"
            :icon="HardDrive"
            :title="t('diskCacheCleanup.disks.empty')"
            :description="t('diskCacheCleanup.disks.emptyHint')"
            class="min-h-[240px]"
          />

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
                      <CacheStateInline
                        :present="localPresentCacheKeys.has(linuxDiskCacheKey(disk.storageId))"
                        :redis-available="localRedisAvailable"
                        :present-label="t('diskCacheCleanup.cache.present')"
                        :absent-label="t('diskCacheCleanup.cache.absent')"
                        :unavailable-label="t('diskCacheCleanup.cache.unavailable')"
                        :detail-label="t('diskCacheCleanup.cache.detailCompact')"
                        :detail-aria-label="t('diskCacheCleanup.actions.viewDetails')"
                        @open-detail="openCacheDetail(linuxDiskCacheKey(disk.storageId))"
                      />
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
                        <CacheStateInline
                          :present="localPresentCacheKeys.has(windowsPartitionCacheKey(partition.partitionGUID))"
                          :redis-available="localRedisAvailable"
                          :present-label="t('diskCacheCleanup.cache.present')"
                          :absent-label="t('diskCacheCleanup.cache.absent')"
                          :unavailable-label="t('diskCacheCleanup.cache.unavailable')"
                          :detail-label="t('diskCacheCleanup.cache.detailCompact')"
                          :detail-aria-label="t('diskCacheCleanup.actions.viewDetails')"
                          @open-detail="openCacheDetail(windowsPartitionCacheKey(partition.partitionGUID))"
                        />
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
              class="inline-flex items-center gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-2.5 text-sm font-semibold text-slate-700 transition hover:border-slate-300 hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40 focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:opacity-50"
              :disabled="!hostIp.trim() || ipsanLoading"
              @click="handleRefreshIpsan"
            >
              <RefreshCw class="h-4 w-4" :class="{ 'animate-spin': ipsanLoading }" />
              {{ t('diskCacheCleanup.ipsan.actions.refresh') }}
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-2xl px-4 py-2.5 text-sm font-semibold text-white transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-rose-500/40 focus-visible:ring-offset-1 disabled:cursor-not-allowed disabled:bg-slate-300"
              :class="activeIpsanRedisAvailable && activeIpsanCleanableCount > 0
                ? (activeIpsanBatchConfirmPending ? 'bg-rose-600 hover:bg-rose-700 ring-2 ring-rose-300 ring-offset-1' : 'bg-rose-500 hover:bg-rose-600')
                : 'bg-slate-300'"
              :disabled="!activeIpsanRedisAvailable || activeIpsanCleanableCount === 0 || activeIpsanBatchCleaning"
              :title="!activeIpsanRedisAvailable ? t('diskCacheCleanup.disabled.redisDown') : (activeIpsanBatchConfirmPending ? t('diskCacheCleanup.batch.confirmAria') : undefined)"
              :aria-pressed="activeIpsanBatchConfirmPending"
              @click="handleActiveIpsanBatchClean"
            >
              <Loader v-if="activeIpsanBatchCleaning" class="h-4 w-4 animate-spin" />
              <Trash2 v-else class="h-4 w-4" />
              <span v-if="activeIpsanBatchCleaning">{{ t('diskCacheCleanup.batch.progress', { count: activeIpsanBatchPendingCount }) }}</span>
              <span v-else-if="activeIpsanBatchConfirmPending">{{ t('diskCacheCleanup.batch.confirm') }}</span>
              <span v-else>{{ activeIpsanCleanAllLabel }}</span>
            </button>
          </div>
        </div>

        <div class="space-y-4 p-5">
          <div
            v-if="ipsanError"
            class="flex items-start gap-3 rounded-[20px] border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700 shadow-sm"
            role="alert"
          >
            <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
            <span>{{ ipsanError }}</span>
          </div>

          <Empty
            v-if="!hasFetchedIpsan && !ipsanLoading && !hostIp.trim()"
            :icon="Server"
            :title="t('diskCacheCleanup.empty.noHost')"
            :description="t('diskCacheCleanup.empty.noHostHint')"
            class="min-h-[220px]"
          />

          <Empty
            v-else-if="!hasFetchedIpsan && !ipsanLoading"
            :icon="Server"
            :title="t('diskCacheCleanup.ipsan.title')"
            :description="t('diskCacheCleanup.ipsan.description')"
            :action-label="t('diskCacheCleanup.actions.fetch')"
            class="min-h-[220px]"
            @action="handleFetchAll"
          />

          <div
            v-else-if="initialFetchPending && ipsans.length === 0"
            class="rounded-[20px] border border-dashed border-orange-200 bg-orange-50/40 px-5 py-6"
            role="status"
            aria-live="polite"
          >
            <p class="mb-3 text-sm font-semibold text-slate-700">{{ t('diskCacheCleanup.disks.loading') }}</p>
            <LoadingSkeleton variant="list-row" :count="3" />
          </div>

          <div
            v-else-if="ipsanLoading && ipsans.length === 0"
            class="flex min-h-[220px] flex-col items-center justify-center rounded-[20px] border border-dashed border-orange-200 bg-orange-50/50 px-6 py-8 text-center"
            role="status"
            aria-live="polite"
          >
            <Loader class="h-7 w-7 animate-spin text-orange-500" aria-hidden="true" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.disks.loading') }}</p>
            <p class="mt-2 text-sm text-slate-500">{{ t('diskCacheCleanup.ipsan.description') }}</p>
          </div>

          <Empty
            v-else-if="ipsans.length === 0"
            :icon="Server"
            :title="t('diskCacheCleanup.disks.empty')"
            :description="t('diskCacheCleanup.ipsan.emptyNoIpsans')"
            class="min-h-[220px]"
          />

          <div
            v-else
            class="space-y-4"
            :class="{ 'opacity-70': ipsanLoading }"
          >
            <div
              class="inline-flex gap-1 rounded-full border border-orange-100 bg-orange-50/80 p-1"
              role="tablist"
              :aria-label="t('diskCacheCleanup.ipsan.title')"
            >
              <button
                ref="ipsanDevicesTabRef"
                type="button"
                role="tab"
                :aria-selected="ipsanSubTab === 'devices'"
                :tabindex="ipsanSubTab === 'devices' ? 0 : -1"
                class="rounded-full px-4 py-2 text-sm font-semibold transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange-500/40 focus-visible:ring-offset-1"
                :class="ipsanSubTab === 'devices' ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-500 hover:text-slate-700'"
                @click="ipsanSubTab = 'devices'"
                @keydown="onIpsanSubTabKeydown($event, 'devices')"
              >
                {{ t('diskCacheCleanup.ipsan.subTabs.devices') }}
              </button>
              <button
                ref="ipsanResourceGroupsTabRef"
                type="button"
                role="tab"
                :aria-selected="ipsanSubTab === 'resourceGroups'"
                :tabindex="ipsanSubTab === 'resourceGroups' ? 0 : -1"
                class="rounded-full px-4 py-2 text-sm font-semibold transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-orange-500/40 focus-visible:ring-offset-1"
                :class="ipsanSubTab === 'resourceGroups' ? 'bg-white text-slate-900 shadow-sm' : 'text-slate-500 hover:text-slate-700'"
                @click="ipsanSubTab = 'resourceGroups'"
                @keydown="onIpsanSubTabKeydown($event, 'resourceGroups')"
              >
                {{ t('diskCacheCleanup.ipsan.subTabs.resourceGroups') }}
              </button>
            </div>

            <div
              v-if="activeIpsanRedisError && !activeIpsanRedisAvailable"
              class="flex items-start gap-3 rounded-[20px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800 shadow-sm"
              role="status"
            >
              <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
              <span>{{ activeIpsanRedisError }}</span>
            </div>

            <div
              v-if="ipsanSubTab === 'resourceGroups' && ipsanResourceGroupError"
              class="flex items-start gap-3 rounded-[20px] border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800 shadow-sm"
              role="status"
            >
              <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
              <span>{{ ipsanResourceGroupError }}</span>
            </div>

            <div
              v-if="ipsanSubTab === 'devices'"
              class="overflow-x-auto"
            >
              <table class="min-w-[1120px] w-full text-sm">
                <thead>
                  <tr class="border-b border-orange-100 bg-orange-50/60 text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-500">
                    <th class="px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.columns.name') }}</th>
                    <th class="w-48 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.columns.id') }}</th>
                    <th class="w-40 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.columns.status') }}</th>
                    <th class="w-28 px-4 py-3 text-right">{{ t('diskCacheCleanup.ipsan.columns.capacity') }}</th>
                    <th class="w-40 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.columns.purpose') }}</th>
                    <th class="w-56 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.columns.resourceGroup') }}</th>
                    <th class="w-40 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.columns.cache') }}</th>
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
                      <template v-if="ipsanGroupMetaFor(item.IPSANId)">
                        <span class="inline-flex items-center rounded-full border border-sky-200 bg-sky-50 px-2.5 py-1 text-xs font-semibold text-sky-700">
                          RG: {{ ipsanGroupMetaFor(item.IPSANId)?.groupName || ipsanGroupMetaFor(item.IPSANId)?.groupId }}
                        </span>
                        <div class="mt-2 flex items-center gap-2 text-xs text-slate-500">
                          <span>{{ t('diskCacheCleanup.ipsan.groupMemberCount', { count: ipsanGroupMetaFor(item.IPSANId)?.memberCount || 0 }) }}</span>
                          <button
                            type="button"
                            class="font-semibold text-sky-700 transition hover:text-sky-800"
                            @click="jumpToIpsanResourceGroup(ipsanGroupMetaFor(item.IPSANId)?.groupId || '')"
                          >
                            {{ t('diskCacheCleanup.ipsan.actions.viewGroup') }}
                          </button>
                        </div>
                      </template>
                      <span
                        v-else
                        class="inline-flex items-center rounded-full border border-slate-200 bg-white px-2.5 py-1 text-xs font-semibold text-slate-500"
                      >
                        {{ t('diskCacheCleanup.ipsan.ungrouped') }}
                      </span>
                    </td>
                    <td class="px-4 py-3">
                      <CacheStateInline
                        :present="ipsanPresentCacheKeys.has(ipsanCacheKey(item.IPSANId))"
                        :redis-available="ipsanRedisAvailable"
                        :present-label="t('diskCacheCleanup.cache.present')"
                        :absent-label="t('diskCacheCleanup.cache.absent')"
                        :unavailable-label="t('diskCacheCleanup.cache.unavailable')"
                        :detail-label="t('diskCacheCleanup.cache.detailCompact')"
                        :detail-aria-label="t('diskCacheCleanup.actions.viewDetails')"
                        @open-detail="openCacheDetail(ipsanCacheKey(item.IPSANId))"
                      />
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

            <Empty
              v-else-if="ipsanResourceGroups.length === 0"
              :icon="Server"
              :title="t('diskCacheCleanup.ipsan.resourceGroups.empty')"
              :description="t('diskCacheCleanup.ipsan.resourceGroups.emptyHint')"
              class="min-h-[220px]"
            />

            <div v-else class="overflow-x-auto">
              <table class="min-w-[1160px] w-full text-sm">
                <thead>
                  <tr class="border-b border-orange-100 bg-orange-50/60 text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-500">
                    <th class="w-12 px-3 py-3 text-left"></th>
                    <th class="px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.resourceGroups.columns.name') }}</th>
                    <th class="w-48 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.resourceGroups.columns.id') }}</th>
                    <th class="w-40 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.resourceGroups.columns.status') }}</th>
                    <th class="w-28 px-4 py-3 text-right">{{ t('diskCacheCleanup.ipsan.resourceGroups.columns.capacity') }}</th>
                    <th class="w-40 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.resourceGroups.columns.purpose') }}</th>
                    <th class="w-28 px-4 py-3 text-right">{{ t('diskCacheCleanup.ipsan.resourceGroups.columns.members') }}</th>
                    <th class="w-40 px-4 py-3 text-left">{{ t('diskCacheCleanup.ipsan.resourceGroups.columns.cache') }}</th>
                    <th class="w-40 px-4 py-3 text-right">{{ t('diskCacheCleanup.ipsan.resourceGroups.columns.actions') }}</th>
                  </tr>
                </thead>
                <tbody class="divide-y divide-orange-50">
                  <template
                    v-for="group in ipsanResourceGroups"
                    :key="group.groupId"
                  >
                    <tr
                      class="transition-colors"
                      :class="highlightedIpsanResourceGroupId === group.groupId ? 'bg-orange-50/80' : 'hover:bg-orange-50/40'"
                    >
                      <td class="px-3 py-3">
                        <button
                          type="button"
                          class="inline-flex h-8 w-8 items-center justify-center rounded-lg border border-orange-100 bg-white text-slate-500 transition hover:border-orange-200 hover:bg-orange-50"
                          :aria-expanded="ipsanExpandedGroupIds.has(group.groupId)"
                          @click="toggleIpsanResourceGroupExpanded(group.groupId)"
                        >
                          <ChevronDown
                            v-if="ipsanExpandedGroupIds.has(group.groupId)"
                            class="h-4 w-4"
                          />
                          <ChevronRight v-else class="h-4 w-4" />
                        </button>
                      </td>
                      <td class="px-4 py-3">
                        <div class="font-medium text-slate-900">{{ ipsanResourceGroupTitle(group) }}</div>
                        <div class="mt-1 text-xs text-slate-500">
                          {{ t('diskCacheCleanup.ipsan.groupMemberCount', { count: group.resourceInfoList.length }) }}
                        </div>
                      </td>
                      <td class="px-4 py-3 font-mono text-xs text-slate-700">
                        {{ group.groupId }}
                      </td>
                      <td class="px-4 py-3">
                        <span
                          class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold"
                          :class="statusBadgeClass(group.groupStatus)"
                        >
                          <span
                            class="h-1.5 w-1.5 rounded-full bg-current"
                            :class="{ 'animate-pulse': statusIsBusy(group.groupStatus) }"
                          ></span>
                          {{ t(statusLabelKey(group.groupStatus)) }}
                        </span>
                      </td>
                      <td class="px-4 py-3 text-right font-mono text-slate-700">
                        {{ formatCapacity(group.totalCapacity) }}
                      </td>
                      <td class="px-4 py-3">
                        <span
                          class="inline-flex items-center rounded-full border px-2.5 py-1 text-xs font-semibold"
                          :class="usageBadgeClass(group.usage)"
                        >
                          {{ t(usageLabelKey(group.usage)) }}
                        </span>
                      </td>
                      <td class="px-4 py-3 text-right font-mono text-slate-700">
                        {{ group.resourceInfoList.length }}
                      </td>
                      <td class="px-4 py-3">
                        <CacheStateInline
                          :present="ipsanResourceGroupPresentCacheKeys.has(ipsanResourceGroupCacheKey(group.groupId))"
                          :redis-available="ipsanResourceGroupRedisAvailable"
                          :present-label="t('diskCacheCleanup.cache.present')"
                          :absent-label="t('diskCacheCleanup.cache.absent')"
                          :unavailable-label="t('diskCacheCleanup.cache.unavailable')"
                          :detail-label="t('diskCacheCleanup.cache.detailCompact')"
                          :detail-aria-label="t('diskCacheCleanup.actions.viewDetails')"
                          @open-detail="openCacheDetail(ipsanResourceGroupCacheKey(group.groupId))"
                        />
                      </td>
                      <td class="px-4 py-3 text-right">
                        <button
                          v-if="ipsanResourceGroupPresentCacheKeys.has(ipsanResourceGroupCacheKey(group.groupId))"
                          type="button"
                          class="inline-flex items-center justify-center gap-2 rounded-xl bg-rose-500 px-3 py-2 text-xs font-semibold text-white transition hover:bg-rose-600 disabled:cursor-not-allowed disabled:bg-slate-300"
                          :disabled="!ipsanResourceGroupRedisAvailable || ipsanResourceGroupCleaningKeys.has(ipsanResourceGroupCacheKey(group.groupId))"
                          :title="!ipsanResourceGroupRedisAvailable ? t('diskCacheCleanup.disabled.redisDown') : undefined"
                          @click="cleanIpsanResourceGroupKeys([ipsanResourceGroupCacheKey(group.groupId)], ipsanResourceGroupCacheKey(group.groupId))"
                        >
                          <Loader
                            v-if="ipsanResourceGroupCleaningKeys.has(ipsanResourceGroupCacheKey(group.groupId))"
                            class="h-3.5 w-3.5 animate-spin"
                          />
                          <Trash2 v-else class="h-3.5 w-3.5" />
                          <span>{{ ipsanResourceGroupCleaningKeys.has(ipsanResourceGroupCacheKey(group.groupId)) ? t('diskCacheCleanup.actions.cleaningOne') : t('diskCacheCleanup.actions.cleanOne') }}</span>
                        </button>
                      </td>
                    </tr>

                    <tr v-if="ipsanExpandedGroupIds.has(group.groupId)" class="bg-orange-50/40">
                      <td colspan="9" class="px-5 py-4">
                        <div class="rounded-2xl border border-orange-100 bg-white px-4 py-4">
                          <div class="mb-3 flex items-center justify-between gap-3">
                            <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                              {{ t('diskCacheCleanup.ipsan.resourceGroups.membersTitle') }}
                            </div>
                            <div
                              v-if="group.resourceInfoList.some((member) => !ipsanMemberReturned(member.IPSANId))"
                              class="inline-flex items-center rounded-full border border-amber-200 bg-amber-50 px-2.5 py-1 text-xs font-semibold text-amber-700"
                            >
                              {{ t('diskCacheCleanup.ipsan.resourceGroups.incompleteMembers') }}
                            </div>
                          </div>

                          <div class="overflow-x-auto">
                            <table class="min-w-[860px] w-full text-sm">
                              <thead>
                                <tr class="border-b border-orange-100 bg-orange-50/50 text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-500">
                                  <th class="px-3 py-3 text-left">{{ t('diskCacheCleanup.ipsan.resourceGroups.memberColumns.name') }}</th>
                                  <th class="w-48 px-3 py-3 text-left">{{ t('diskCacheCleanup.ipsan.resourceGroups.memberColumns.id') }}</th>
                                  <th class="w-40 px-3 py-3 text-left">{{ t('diskCacheCleanup.ipsan.resourceGroups.memberColumns.status') }}</th>
                                  <th class="w-28 px-3 py-3 text-right">{{ t('diskCacheCleanup.ipsan.resourceGroups.memberColumns.capacity') }}</th>
                                  <th class="w-40 px-3 py-3 text-left">{{ t('diskCacheCleanup.ipsan.resourceGroups.memberColumns.purpose') }}</th>
                                  <th class="w-36 px-3 py-3 text-left">{{ t('diskCacheCleanup.ipsan.resourceGroups.memberColumns.cache') }}</th>
                                </tr>
                              </thead>
                              <tbody class="divide-y divide-orange-50">
                                <tr
                                  v-for="member in group.resourceInfoList"
                                  :key="`${group.groupId}-${member.IPSANId}`"
                                  class="hover:bg-orange-50/30"
                                >
                                  <td class="px-3 py-3">
                                    <div class="font-medium text-slate-900">{{ member.IPSANName || member.IPSANIp || '--' }}</div>
                                    <div class="mt-1 font-mono text-xs text-slate-400">{{ member.IPSANIp || '--' }}</div>
                                    <div
                                      v-if="!ipsanMemberReturned(member.IPSANId)"
                                      class="mt-2 text-xs font-semibold text-amber-700"
                                    >
                                      {{ t('diskCacheCleanup.ipsan.resourceGroups.memberMissing') }}
                                    </div>
                                  </td>
                                  <td class="px-3 py-3 font-mono text-xs text-slate-700">
                                    {{ member.IPSANId }}
                                  </td>
                                  <td class="px-3 py-3">
                                    <span
                                      class="inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold"
                                      :class="statusBadgeClass(member.IPSANStatus)"
                                    >
                                      <span
                                        class="h-1.5 w-1.5 rounded-full bg-current"
                                        :class="{ 'animate-pulse': statusIsBusy(member.IPSANStatus) }"
                                      ></span>
                                      {{ t(statusLabelKey(member.IPSANStatus)) }}
                                    </span>
                                  </td>
                                  <td class="px-3 py-3 text-right font-mono text-slate-700">
                                    {{ formatCapacity(member.capacity) }}
                                  </td>
                                  <td class="px-3 py-3">
                                    <span
                                      v-if="ipsanItemFor(member.IPSANId)"
                                      class="inline-flex items-center rounded-full border px-2.5 py-1 text-xs font-semibold"
                                      :class="usageBadgeClass(ipsanItemFor(member.IPSANId)?.usage ?? -1)"
                                    >
                                      {{ t(usageLabelKey(ipsanItemFor(member.IPSANId)?.usage ?? -1)) }}
                                    </span>
                                    <span v-else class="text-sm text-slate-400">
                                      {{ t('diskCacheCleanup.usage.unknown') }}
                                    </span>
                                  </td>
                                  <td class="px-3 py-3">
                                    <CacheStateInline
                                      :present="ipsanPresentCacheKeys.has(ipsanCacheKey(member.IPSANId))"
                                      :redis-available="ipsanRedisAvailable"
                                      :present-label="t('diskCacheCleanup.cache.present')"
                                      :absent-label="t('diskCacheCleanup.cache.absent')"
                                      :unavailable-label="t('diskCacheCleanup.cache.unavailable')"
                                      :detail-label="t('diskCacheCleanup.cache.detailCompact')"
                                      :detail-aria-label="t('diskCacheCleanup.actions.viewDetails')"
                                      @open-detail="openCacheDetail(ipsanCacheKey(member.IPSANId))"
                                    />
                                  </td>
                                </tr>
                              </tbody>
                            </table>
                          </div>
                        </div>
                      </td>
                    </tr>
                  </template>
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </section>
    </div>

    <div
      v-if="cacheDetailOpen"
      class="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/45 px-4 py-6"
      @click.self="closeCacheDetail"
      @keydown="onModalKeydown"
    >
      <div
        ref="cacheDetailModalRef"
        class="max-h-[85vh] w-full max-w-4xl overflow-hidden rounded-[28px] border border-slate-200 bg-white shadow-[0_24px_80px_rgba(15,23,42,0.24)] focus:outline-none"
        role="dialog"
        aria-modal="true"
        aria-labelledby="disk-cache-detail-title"
        aria-describedby="disk-cache-detail-key"
        tabindex="-1"
      >
        <div class="flex flex-col gap-3 border-b border-slate-200 px-5 py-4 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0">
            <h3 id="disk-cache-detail-title" class="text-lg font-bold text-slate-900">{{ t('diskCacheCleanup.cache.detailTitle') }}</h3>
            <p id="disk-cache-detail-key" class="mt-1 break-all font-mono text-xs text-slate-500">
              {{ cacheDetailKey || '--' }}
            </p>
          </div>
          <button
            type="button"
            class="inline-flex items-center justify-center rounded-xl border border-slate-200 px-3 py-2 text-sm font-semibold text-slate-600 transition hover:border-slate-300 hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40 focus-visible:ring-offset-1"
            :aria-label="t('settings.close')"
            @click="closeCacheDetail"
          >
            {{ t('settings.close') }}
          </button>
        </div>

        <div class="space-y-4 overflow-y-auto p-5">
          <div
            v-if="cacheDetailLoading"
            class="flex min-h-[240px] flex-col items-center justify-center rounded-[20px] border border-dashed border-slate-200 bg-slate-50 px-6 py-8 text-center"
            role="status"
            aria-live="polite"
          >
            <Loader class="h-7 w-7 animate-spin text-indigo-500" aria-hidden="true" />
            <p class="mt-4 text-base font-semibold text-slate-900">{{ t('diskCacheCleanup.cache.loadingDetail') }}</p>
          </div>

          <div
            v-else-if="cacheDetailError"
            class="flex items-start gap-3 rounded-[20px] border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700 shadow-sm"
            role="alert"
          >
            <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
            <span>{{ cacheDetailError }}</span>
          </div>

          <template v-else-if="cacheDetailEntry">
            <div class="grid gap-3 sm:grid-cols-2">
              <div class="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3">
                <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                  {{ t('diskCacheCleanup.cache.type') }}
                </div>
                <div class="mt-2 font-mono text-sm font-semibold text-slate-900">
                  {{ cacheDetailEntry.value_type || '--' }}
                </div>
              </div>

              <div class="rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3">
                <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                  {{ t('diskCacheCleanup.cache.preview') }}
                </div>
                <div class="mt-2 whitespace-pre-wrap break-all font-mono text-xs leading-6 text-slate-700">
                  {{ cacheDetailPreview }}
                </div>
              </div>
            </div>

            <div class="rounded-[24px] border border-slate-200 bg-slate-50/80 px-4 py-4">
              <div class="text-[11px] font-semibold uppercase tracking-[0.14em] text-slate-400">
                {{ t('diskCacheCleanup.cache.fullValue') }}
              </div>
              <pre class="mt-3 max-h-[48vh] overflow-auto whitespace-pre-wrap break-all rounded-2xl border border-slate-200 bg-white px-4 py-4 font-mono text-xs leading-6 text-slate-700">{{ cacheDetailValue }}</pre>
            </div>
          </template>
        </div>
      </div>
    </div>
  </div>
</template>
