<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { pingScan, cancelPingScan, saveTextFile, type PingResult, type PingScanRequest } from '../../lib/tauri';
import { mergeRecentItems, normalizeRecentItems } from '../../lib/recentHistory';

defineOptions({ name: 'PingScanTab' });

const { t } = useI18n();

// ── Persistence helpers ───────────────────────────────────────────────────

const KV_KEY = 'networkTools.pingScanConfig';
const RECENT_PREFIXES_KEY = 'networkTools.pingScan.recentPrefixes';
const RECENT_PREFIXES_LIMIT = 10;

async function saveConfig() {
  try {
    await invoke('save_kv', {
      key: KV_KEY,
      value: { prefix: prefix.value, start: start.value, end: end.value, timeoutMs: timeoutMs.value },
    });
  } catch { /* ignore */ }
}

// ── State ──────────────────────────────────────────────────────────────────

const prefix = ref('192.168.1');
const start = ref(1);
const end = ref(254);
const timeoutMs = ref(1000);
const recentPrefixes = ref<string[]>([]);

function isValidPrefixValue(value: string) {
  if (!/^\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(value)) return false;
  const octets = value.split('.').map(Number);
  return octets.every(octet => octet >= 0 && octet <= 255);
}

function isRecentPrefixSelected(value: string) {
  return prefix.value.trim() === value;
}

function selectRecentPrefix(value: string) {
  if (isScanning.value) {
    return;
  }
  prefix.value = value;
}

async function storeRecentPrefixes(items: readonly string[]) {
  const normalized = normalizeRecentItems(items, RECENT_PREFIXES_LIMIT);
  recentPrefixes.value = normalized;
  try {
    await invoke('save_kv', {
      key: RECENT_PREFIXES_KEY,
      value: normalized,
    });
  } catch {
    /* Recent prefixes are best-effort only. */
  }
}

async function rememberRecentPrefix(value: string) {
  const normalizedValue = value.trim();
  if (!isValidPrefixValue(normalizedValue)) {
    return;
  }
  await storeRecentPrefixes(mergeRecentItems(recentPrefixes.value, normalizedValue, RECENT_PREFIXES_LIMIT));
}

// Load persisted config on mount
onMounted(async () => {
  try {
    const saved = await invoke<{ prefix?: string; start?: number; end?: number; timeoutMs?: number } | null>('load_kv', { key: KV_KEY });
    if (saved) {
      if (saved.prefix !== undefined) prefix.value = saved.prefix;
      if (saved.start !== undefined) start.value = saved.start;
      if (saved.end !== undefined) end.value = saved.end;
      if (saved.timeoutMs !== undefined) timeoutMs.value = saved.timeoutMs;
    }
  } catch { /* ignore */ }

  try {
    const saved = await invoke<string[] | null>('load_kv', { key: RECENT_PREFIXES_KEY });
    recentPrefixes.value = normalizeRecentItems(saved, RECENT_PREFIXES_LIMIT);
  } catch {
    /* Ignore malformed recent prefixes from older builds. */
  }
});

// Persist on change
watch([prefix, start, end, timeoutMs], saveConfig);
const isScanning = ref(false);
const results = ref(new Map<string, PingResult>());
const viewMode = ref<'grid' | 'table'>('grid');
const tableFilter = ref<'all' | 'online' | 'offline'>('all');

// ── Validation ─────────────────────────────────────────────────────────────

const prefixError = computed(() => {
  const v = prefix.value.trim();
  if (!v || !isValidPrefixValue(v)) return t('networkTools.ping.prefixError');
  return '';
});

const rangeError = computed(() => {
  const s = start.value;
  const e = end.value;
  if (s < 0 || s > 255 || e < 0 || e > 255 || s > e) return t('networkTools.ping.rangeError');
  return '';
});

const isFormValid = computed(() => !prefixError.value && !rangeError.value);

// ── Computed stats ─────────────────────────────────────────────────────────

const totalIps = computed(() => end.value - start.value + 1);
const scannedCount = computed(() => results.value.size);
const onlineCount = computed(() => {
  let n = 0;
  for (const r of results.value.values()) {
    if (r.alive) n++;
  }
  return n;
});
const offlineCount = computed(() => scannedCount.value - onlineCount.value);
const progressPct = computed(() =>
  totalIps.value > 0 ? Math.round((scannedCount.value / totalIps.value) * 100) : 0,
);

// ── Grid cells ─────────────────────────────────────────────────────────────

const gridCells = computed(() => {
  const cells: Array<{ octet: number; ip: string; state: 'online' | 'offline' | 'scanning' | 'waiting'; latencyMs: number | null }> = [];
  for (let i = start.value; i <= end.value; i++) {
    const ip = `${prefix.value}.${i}`;
    const res = results.value.get(ip);
    let state: 'online' | 'offline' | 'scanning' | 'waiting';
    if (res) {
      state = res.alive ? 'online' : 'offline';
    } else if (isScanning.value) {
      state = 'scanning';
    } else {
      state = 'waiting';
    }
    cells.push({ octet: i, ip, state, latencyMs: res?.latencyMs ?? null });
  }
  return cells;
});

// ── Table rows (filtered) ──────────────────────────────────────────────────

const tableRows = computed(() => {
  const rows: Array<{ ip: string; alive: boolean; latencyMs: number | null }> = [];
  for (let i = start.value; i <= end.value; i++) {
    const ip = `${prefix.value}.${i}`;
    const res = results.value.get(ip);
    if (!res) continue;
    if (tableFilter.value === 'online' && !res.alive) continue;
    if (tableFilter.value === 'offline' && res.alive) continue;
    rows.push({ ip, alive: res.alive, latencyMs: res.latencyMs });
  }
  return rows;
});

// ── Cell styling ───────────────────────────────────────────────────────────

function cellClass(state: 'online' | 'offline' | 'scanning' | 'waiting') {
  switch (state) {
    case 'online':
      return 'bg-emerald-500 text-white';
    case 'offline':
      return 'bg-slate-200 text-slate-400';
    case 'scanning':
      return 'bg-amber-400 text-white animate-pulse';
    case 'waiting':
    default:
      return 'bg-slate-700 text-slate-500';
  }
}

// ── Listeners ──────────────────────────────────────────────────────────────

let unlistenResult: UnlistenFn | null = null;
let unlistenComplete: UnlistenFn | null = null;

async function attachListeners() {
  unlistenResult = await listen<PingResult>('ping-result', event => {
    results.value = new Map(results.value).set(event.payload.ip, event.payload);
  });
  unlistenComplete = await listen('ping-scan-complete', () => {
    isScanning.value = false;
  });
}

function detachListeners() {
  unlistenResult?.();
  unlistenComplete?.();
  unlistenResult = null;
  unlistenComplete = null;
}

onUnmounted(() => {
  detachListeners();
});

// ── Scan control ───────────────────────────────────────────────────────────

async function startScan() {
  if (!isFormValid.value || isScanning.value) return;
  const normalizedPrefix = prefix.value.trim();
  await rememberRecentPrefix(normalizedPrefix);
  results.value = new Map();
  isScanning.value = true;
  await attachListeners();
  const request: PingScanRequest = {
    prefix: normalizedPrefix,
    start: start.value,
    end: end.value,
    timeoutMs: timeoutMs.value,
  };
  try {
    await pingScan(request);
  } catch (err) {
    isScanning.value = false;
    detachListeners();
    console.error('pingScan error:', err);
  }
}

async function stopScan() {
  try {
    await cancelPingScan();
  } catch (err) {
    console.error('cancelPingScan error:', err);
  } finally {
    isScanning.value = false;
    detachListeners();
  }
}

// ── CSV Export ─────────────────────────────────────────────────────────────

async function exportCsv() {
  const lines: string[] = ['IP,Status,Latency(ms)'];
  for (let i = start.value; i <= end.value; i++) {
    const ip = `${prefix.value}.${i}`;
    const res = results.value.get(ip);
    if (!res) continue;
    const status = res.alive ? t('networkTools.ping.online') : t('networkTools.ping.offline');
    const latency = res.latencyMs !== null ? String(res.latencyMs) : '';
    lines.push(`${ip},${status},${latency}`);
  }
  const content = lines.join('\n');
  await saveTextFile(content, 'ping-scan.csv', 'CSV', ['csv']);
}

// ── Tooltip state ──────────────────────────────────────────────────────────

const tooltip = ref<{ ip: string; latencyMs: number | null; x: number; y: number } | null>(null);

function showTooltip(cell: { ip: string; latencyMs: number | null }, event: MouseEvent) {
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  tooltip.value = { ip: cell.ip, latencyMs: cell.latencyMs, x: rect.left + rect.width / 2, y: rect.top };
}

function hideTooltip() {
  tooltip.value = null;
}
</script>

<template>
  <div class="space-y-4">
    <!-- Input area -->
    <div class="bg-white rounded-xl border border-slate-200 p-4 shadow-sm">
      <div class="flex flex-wrap items-start gap-4">
        <!-- Prefix -->
        <div class="flex-1 min-w-[180px]">
          <label class="block text-xs font-medium text-slate-600 mb-1.5">
            {{ t('networkTools.ping.prefix') }}
          </label>
          <div class="flex items-center gap-1.5">
            <input
              v-model="prefix"
              type="text"
              list="ping-scan-recent-prefixes"
              :placeholder="t('networkTools.ping.prefixPlaceholder')"
              :disabled="isScanning"
              :class="[
                'rounded-lg border px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition disabled:bg-slate-50 disabled:cursor-not-allowed w-full',
                prefixError ? 'border-red-400 focus:ring-red-400 focus:border-red-400' : 'border-slate-300',
              ]"
            />
            <span class="text-xs text-slate-400 whitespace-nowrap font-mono shrink-0">
              .{{ start }}–.{{ end }}
            </span>
          </div>
          <datalist id="ping-scan-recent-prefixes">
            <option v-for="item in recentPrefixes" :key="`ping-scan-recent-${item}`" :value="item" />
          </datalist>
          <p v-if="prefixError" class="mt-1 text-xs text-red-500">{{ prefixError }}</p>
          <div v-if="recentPrefixes.length > 0" class="mt-2 flex items-center gap-2 flex-wrap">
            <span class="text-xs font-medium text-slate-500">{{ t('history.title') }}:</span>
            <button
              v-for="item in recentPrefixes"
              :key="`ping-scan-history-${item}`"
              type="button"
              :disabled="isScanning"
              class="px-2.5 py-1 text-xs font-medium rounded-full border transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              :class="isRecentPrefixSelected(item)
                ? 'bg-blue-600 text-white border-blue-600'
                : 'bg-white text-slate-600 border-slate-300 hover:bg-slate-50 hover:border-slate-400'"
              @click="selectRecentPrefix(item)"
            >
              <span class="font-mono">{{ item }}</span>
            </button>
          </div>
        </div>

        <!-- Start -->
        <div class="w-24">
          <label class="block text-xs font-medium text-slate-600 mb-1.5">
            {{ t('networkTools.ping.start') }}
          </label>
          <input
            v-model.number="start"
            type="number"
            min="0"
            max="255"
            :disabled="isScanning"
            :class="[
              'rounded-lg border px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition disabled:bg-slate-50 disabled:cursor-not-allowed w-full',
              rangeError ? 'border-red-400 focus:ring-red-400 focus:border-red-400' : 'border-slate-300',
            ]"
          />
        </div>

        <!-- End -->
        <div class="w-24">
          <label class="block text-xs font-medium text-slate-600 mb-1.5">
            {{ t('networkTools.ping.end') }}
          </label>
          <input
            v-model.number="end"
            type="number"
            min="0"
            max="255"
            :disabled="isScanning"
            :class="[
              'rounded-lg border px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition disabled:bg-slate-50 disabled:cursor-not-allowed w-full',
              rangeError ? 'border-red-400 focus:ring-red-400 focus:border-red-400' : 'border-slate-300',
            ]"
          />
          <p v-if="rangeError" class="mt-1 text-xs text-red-500">{{ rangeError }}</p>
        </div>

        <!-- Timeout -->
        <div class="w-28">
          <label class="block text-xs font-medium text-slate-600 mb-1.5">
            {{ t('networkTools.ping.timeoutMs') }}
          </label>
          <input
            v-model.number="timeoutMs"
            type="number"
            min="100"
            max="10000"
            :disabled="isScanning"
            class="rounded-lg border border-slate-300 px-3 py-2 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition disabled:bg-slate-50 disabled:cursor-not-allowed w-full"
          />
        </div>

        <!-- Start/Stop button -->
        <div class="flex items-end pb-0.5">
          <button
            v-if="!isScanning"
            @click="startScan"
            :disabled="!isFormValid"
            class="bg-blue-600 text-white px-4 py-2 rounded-lg hover:bg-blue-700 disabled:opacity-40 disabled:cursor-not-allowed transition text-sm font-medium whitespace-nowrap"
          >
            {{ t('networkTools.ping.startScan') }}
          </button>
          <button
            v-else
            @click="stopScan"
            class="bg-red-500 hover:bg-red-600 text-white px-4 py-2 rounded-lg transition text-sm font-medium whitespace-nowrap"
          >
            {{ t('networkTools.ping.stopScan') }}
          </button>
        </div>
      </div>
    </div>

    <!-- Results area: only show when there are results or scanning -->
    <div v-if="isScanning || results.size > 0" class="space-y-3">
      <!-- Stats bar + view toggle -->
      <div class="flex flex-wrap items-center justify-between gap-3">
        <!-- Stats -->
        <div class="text-sm text-slate-600">
          <span v-if="isScanning" class="text-amber-600 font-medium">{{ t('networkTools.ping.scanning') }}:</span>
          <span v-else class="text-emerald-600 font-medium">{{ t('networkTools.ping.complete') }}</span>
          <span class="ml-1 tabular-nums">{{ scannedCount }}/{{ totalIps }}</span>
          <span class="mx-2 text-slate-300">|</span>
          <span class="text-emerald-600">{{ t('networkTools.ping.online') }} {{ onlineCount }}</span>
          <span class="mx-1 text-slate-300"></span>
          <span class="text-slate-500">{{ t('networkTools.ping.offline') }} {{ offlineCount }}</span>
        </div>

        <!-- View toggle + Export -->
        <div class="flex items-center gap-2">
          <!-- View mode pill -->
          <div class="inline-flex rounded-lg border border-slate-200 bg-slate-50 p-0.5 gap-0.5">
            <button
              @click="viewMode = 'grid'"
              :class="[
                'px-3 py-1 rounded-md text-xs font-medium transition',
                viewMode === 'grid' ? 'bg-white text-slate-800 shadow-sm' : 'text-slate-500 hover:text-slate-700',
              ]"
            >
              {{ t('networkTools.ping.gridView') }}
            </button>
            <button
              @click="viewMode = 'table'"
              :class="[
                'px-3 py-1 rounded-md text-xs font-medium transition',
                viewMode === 'table' ? 'bg-white text-slate-800 shadow-sm' : 'text-slate-500 hover:text-slate-700',
              ]"
            >
              {{ t('networkTools.ping.tableView') }}
            </button>
          </div>

          <!-- Export CSV -->
          <button
            v-if="results.size > 0"
            @click="exportCsv"
            class="px-3 py-1 text-xs font-medium rounded-lg border border-slate-200 bg-white text-slate-600 hover:bg-slate-50 transition"
          >
            {{ t('networkTools.ping.exportCsv') }}
          </button>
        </div>
      </div>

      <!-- Progress bar -->
      <div class="w-full bg-slate-100 rounded-full h-1.5 overflow-hidden">
        <div
          class="h-1.5 rounded-full transition-all duration-300"
          :class="isScanning ? 'bg-amber-400' : 'bg-emerald-500'"
          :style="{ width: `${progressPct}%` }"
        ></div>
      </div>

      <!-- Grid view -->
      <div v-if="viewMode === 'grid'" class="bg-white rounded-xl border border-slate-200 p-4 shadow-sm">
        <div class="grid gap-1" style="grid-template-columns: repeat(16, minmax(0, 1fr));">
          <div
            v-for="cell in gridCells"
            :key="cell.ip"
            :class="['rounded flex items-center justify-center text-[11px] font-mono font-medium cursor-default select-none aspect-square', cellClass(cell.state)]"
            @mouseenter="showTooltip(cell, $event)"
            @mouseleave="hideTooltip"
          >
            .{{ cell.octet }}
          </div>
        </div>

        <!-- Legend -->
        <div class="flex flex-wrap gap-3 mt-3 pt-3 border-t border-slate-100">
          <span class="flex items-center gap-1.5 text-xs text-slate-500">
            <span class="w-3 h-3 rounded bg-emerald-500 inline-block"></span>
            {{ t('networkTools.ping.online') }}
          </span>
          <span class="flex items-center gap-1.5 text-xs text-slate-500">
            <span class="w-3 h-3 rounded bg-slate-200 inline-block"></span>
            {{ t('networkTools.ping.offline') }}
          </span>
          <span class="flex items-center gap-1.5 text-xs text-slate-500">
            <span class="w-3 h-3 rounded bg-amber-400 inline-block"></span>
            {{ t('networkTools.ping.scanning') }}
          </span>
          <span class="flex items-center gap-1.5 text-xs text-slate-500">
            <span class="w-3 h-3 rounded bg-slate-700 inline-block"></span>
            {{ t('networkTools.ping.waiting') }}
          </span>
        </div>
      </div>

      <!-- Table view -->
      <div v-else class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
        <!-- Filter bar -->
        <div class="flex items-center gap-2 px-4 py-2.5 border-b border-slate-100 bg-slate-50/60">
          <span class="text-xs font-medium text-slate-500 mr-1">{{ t('networkTools.ping.status') }}:</span>
          <label
            v-for="opt in (['all', 'online', 'offline'] as const)"
            :key="opt"
            class="inline-flex items-center gap-1.5 cursor-pointer"
          >
            <input
              type="radio"
              v-model="tableFilter"
              :value="opt"
              class="accent-blue-600"
            />
            <span class="text-xs text-slate-600">
              {{ opt === 'all' ? t('networkTools.ping.filterAll') : opt === 'online' ? t('networkTools.ping.filterOnline') : t('networkTools.ping.filterOffline') }}
            </span>
          </label>
        </div>

        <div class="overflow-x-auto">
          <table class="w-full">
            <thead>
              <tr class="border-b border-slate-100 bg-slate-50/80">
                <th class="px-4 py-2.5 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                  {{ t('networkTools.ping.ipAddress') }}
                </th>
                <th class="px-4 py-2.5 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                  {{ t('networkTools.ping.status') }}
                </th>
                <th class="px-4 py-2.5 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">
                  {{ t('networkTools.ping.latency') }}
                </th>
              </tr>
            </thead>
            <tbody class="divide-y divide-slate-100">
              <tr
                v-for="row in tableRows"
                :key="row.ip"
                class="hover:bg-slate-50/60 transition-colors"
              >
                <td class="px-4 py-2.5 text-sm font-mono text-slate-800">{{ row.ip }}</td>
                <td class="px-4 py-2.5">
                  <span
                    :class="[
                      'inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium',
                      row.alive
                        ? 'bg-emerald-50 text-emerald-700'
                        : 'bg-slate-100 text-slate-500',
                    ]"
                  >
                    {{ row.alive ? t('networkTools.ping.online') : t('networkTools.ping.offline') }}
                  </span>
                </td>
                <td class="px-4 py-2.5 text-sm text-slate-600 tabular-nums">
                  {{ row.latencyMs !== null ? `${row.latencyMs} ms` : '—' }}
                </td>
              </tr>
              <tr v-if="tableRows.length === 0">
                <td colspan="3" class="px-4 py-6 text-center text-sm text-slate-400">
                  —
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>

    <!-- Tooltip (fixed position) -->
    <Teleport to="body">
      <div
        v-if="tooltip"
        class="fixed z-50 pointer-events-none px-2.5 py-1.5 bg-slate-800 text-white text-xs rounded-lg shadow-lg whitespace-nowrap -translate-x-1/2 -translate-y-full -mt-1"
        :style="{ left: `${tooltip.x}px`, top: `${tooltip.y - 6}px` }"
      >
        <div class="font-mono font-medium">{{ tooltip.ip }}</div>
        <div class="text-slate-300 mt-0.5">
          {{ tooltip.latencyMs !== null ? `${tooltip.latencyMs} ms` : t('networkTools.ping.waiting') }}
        </div>
      </div>
    </Teleport>
  </div>
</template>
