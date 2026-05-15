<script setup lang="ts">
import { Copy, Pencil, Trash2 } from 'lucide-vue-next';
import { computed, onUnmounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  cancelPortTest,
  testPorts,
  type PortPreset,
  type PortTestRequest,
  type PortTestResult,
  type SinglePortResult,
} from '../../lib/tauri';
import {
  buildOpenPortCards,
  buildPortGridCells,
  filterPortRows,
  parsePorts,
  type OpenPortCard,
  type PortGridCell,
  type PortGridState,
  type PortTableFilter,
} from '../../lib/portTestPresentation';
import Empty from '../Empty.vue';
import { pushToast } from '../../composables/useToast';

defineOptions({ name: 'PortTestTab' });

const { t } = useI18n();

const STORAGE_KEY = 'networkTools.portPresets';
const LARGE_SCAN_THRESHOLD = 1000;
const LARGE_SCAN_TIMEOUT_MS = 500;
const LARGE_SCAN_GRID_THRESHOLD = 1024;

function loadPresetsFromLocalStorage(): PortPreset[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    return JSON.parse(raw) as PortPreset[];
  } catch {
    return [];
  }
}

function savePresetsToLocalStorage(presets: PortPreset[]): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(presets));
}

const builtinPresets = computed<PortPreset[]>(() => [
  { name: 'Web', ports: '80,443' },
  { name: 'SSH', ports: '22' },
  { name: 'Database', ports: '3306,5432,6379' },
  { name: 'Common', ports: '22,80,443,3306,5432,6379,8080,8443' },
  { name: t('networkTools.port.allPorts'), ports: 'all' },
]);

const host = ref('');
const portsInput = ref('');
const timeoutMs = ref(500);
const isLoading = ref(false);
const resultHost = ref('');
const resolvedIp = ref<string | null>(null);
const requestedPorts = ref<number[]>([]);
const resultRows = ref(new Map<number, SinglePortResult>());
const viewMode = ref<'grid' | 'table'>('grid');
const tableFilter = ref<PortTableFilter>('open');
const customPresets = ref<PortPreset[]>(loadPresetsFromLocalStorage());

const showPresetForm = ref(false);
const editingPreset = ref<number | null>(null);
const presetName = ref('');
const presetPorts = ref('');
const errorMsg = ref('');

let unlistenResult: UnlistenFn | null = null;
let unlistenComplete: UnlistenFn | null = null;
let flushTimer: number | null = null;
const pendingRows = new Map<number, SinglePortResult>();

const tableRows = computed(() => filterPortRows(resultRows.value, tableFilter.value));
const allRows = computed(() => filterPortRows(resultRows.value, 'all'));
const totalPorts = computed(() => requestedPorts.value.length);
const scannedCount = computed(() => resultRows.value.size);
const openCount = computed(() => allRows.value.filter(row => row.open).length);
const closedCount = computed(() => scannedCount.value - openCount.value);
const progressPct = computed(() =>
  totalPorts.value > 0 ? Math.round((scannedCount.value / totalPorts.value) * 100) : 0,
);

const result = computed<PortTestResult | null>(() => {
  if (!resultHost.value && scannedCount.value === 0) return null;
  return {
    host: resultHost.value,
    resolvedIp: resolvedIp.value,
    results: allRows.value,
  };
});

const resultSummary = computed(() => {
  return allRows.value
    .map((row) => `${row.port}\t${row.open ? t('networkTools.port.open') : t('networkTools.port.closed')}\t${row.name || '-'}`)
    .join('\n');
});

const gridCells = computed(() => buildPortGridCells(requestedPorts.value, resultRows.value, isLoading.value));
const showCellLabels = computed(() => totalPorts.value <= 1024);
const gridStyle = computed(() => ({
  gridTemplateColumns: `repeat(auto-fill, minmax(${showCellLabels.value ? '56px' : '10px'}, 1fr))`,
  gap: '6px',
}));
const gridCellBaseClass = computed(() =>
  showCellLabels.value
    ? 'rounded-md flex aspect-square items-center justify-center text-xs font-mono font-medium cursor-default select-none transition-colors'
    : 'rounded-[2px] aspect-square min-h-2 cursor-default transition-colors',
);
const isLargeScan = computed(() => totalPorts.value > LARGE_SCAN_GRID_THRESHOLD);
const openPortCards = computed<OpenPortCard[]>(() => buildOpenPortCards(resultRows.value));

function applyPreset(ports: string): void {
  if (isLoading.value) return;
  portsInput.value = ports;
  if ((ports === 'all' || ports === '1-65535') && timeoutMs.value > LARGE_SCAN_TIMEOUT_MS) {
    timeoutMs.value = LARGE_SCAN_TIMEOUT_MS;
  }
}

function openAddPreset(): void {
  editingPreset.value = null;
  presetName.value = '';
  presetPorts.value = '';
  showPresetForm.value = true;
}

function openEditPreset(index: number): void {
  editingPreset.value = index;
  presetName.value = customPresets.value[index].name;
  presetPorts.value = customPresets.value[index].ports;
  showPresetForm.value = true;
}

function cancelPresetForm(): void {
  showPresetForm.value = false;
  editingPreset.value = null;
  presetName.value = '';
  presetPorts.value = '';
}

function savePreset(): void {
  const name = presetName.value.trim();
  const ports = presetPorts.value.trim();
  if (!name || !ports) return;

  if (editingPreset.value !== null) {
    customPresets.value[editingPreset.value] = { name, ports };
  } else {
    customPresets.value.push({ name, ports });
  }
  savePresetsToLocalStorage(customPresets.value);
  cancelPresetForm();
}

function deletePreset(index: number): void {
  customPresets.value.splice(index, 1);
  savePresetsToLocalStorage(customPresets.value);
}

function setResultRows(rows: SinglePortResult[]): void {
  const next = new Map<number, SinglePortResult>();
  for (const row of rows) next.set(row.port, row);
  resultRows.value = next;
}

function flushPendingRows(): void {
  if (pendingRows.size === 0) return;
  const next = new Map(resultRows.value);
  for (const [port, row] of pendingRows) next.set(port, row);
  pendingRows.clear();
  resultRows.value = next;
}

function scheduleFlush(): void {
  if (flushTimer !== null) return;
  flushTimer = window.setTimeout(() => {
    flushTimer = null;
    flushPendingRows();
  }, 80);
}

async function attachListeners(): Promise<void> {
  detachListeners();
  unlistenResult = await listen<SinglePortResult>('port-test-result', event => {
    pendingRows.set(event.payload.port, event.payload);
    scheduleFlush();
  });
  unlistenComplete = await listen('port-test-complete', () => {
    flushPendingRows();
    isLoading.value = false;
  });
}

function detachListeners(): void {
  unlistenResult?.();
  unlistenComplete?.();
  unlistenResult = null;
  unlistenComplete = null;
}

async function startTest(): Promise<void> {
  if (isLoading.value) return;

  errorMsg.value = '';
  const h = host.value.trim();
  if (!h) {
    errorMsg.value = t('networkTools.port.hostError');
    return;
  }

  const ports = parsePorts(portsInput.value);
  if (ports.length === 0) {
    errorMsg.value = t('networkTools.port.portsError');
    return;
  }

  if (ports.length > LARGE_SCAN_THRESHOLD && timeoutMs.value > LARGE_SCAN_TIMEOUT_MS) {
    timeoutMs.value = LARGE_SCAN_TIMEOUT_MS;
  }

  resultHost.value = h;
  resolvedIp.value = null;
  requestedPorts.value = ports;
  resultRows.value = new Map();
  pendingRows.clear();
  tableFilter.value = 'open';
  isLoading.value = true;

  try {
    await attachListeners();
    const request: PortTestRequest = {
      host: h,
      ports,
      timeoutMs: timeoutMs.value,
    };
    const finalResult = await testPorts(request);
    resultHost.value = finalResult.host;
    resolvedIp.value = finalResult.resolvedIp;
    flushPendingRows();
    setResultRows(finalResult.results);
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    flushPendingRows();
    isLoading.value = false;
    detachListeners();
  }
}

async function stopTest(): Promise<void> {
  try {
    await cancelPortTest();
  } catch (err) {
    console.error('cancelPortTest error:', err);
  } finally {
    flushPendingRows();
    isLoading.value = false;
    detachListeners();
  }
}

async function copyResultSummary(): Promise<void> {
  if (!resultSummary.value) return;
  try {
    await navigator.clipboard.writeText(resultSummary.value);
    pushToast(t('networkTools.copy.copied'), 'success', { ttlMs: 1800 });
  } catch (error) {
    pushToast(t('networkTools.copy.failed', { error: String(error) }), 'error', { ttlMs: 3600 });
  }
}

function portCellClass(state: PortGridState): string {
  switch (state) {
    case 'open':
      return 'bg-emerald-500 text-white';
    case 'closed':
      return 'bg-slate-200 text-slate-400';
    case 'scanning':
      return 'bg-amber-400 text-white animate-pulse';
    case 'waiting':
    default:
      return 'bg-slate-700 text-slate-500';
  }
}

function portStatusLabel(state: PortGridState): string {
  switch (state) {
    case 'open':
      return t('networkTools.port.open');
    case 'closed':
      return t('networkTools.port.closed');
    case 'scanning':
      return t('networkTools.port.scanning');
    case 'waiting':
    default:
      return t('networkTools.port.waiting');
  }
}

const tooltip = ref<{ cell: PortGridCell; x: number; y: number } | null>(null);

function showTooltip(cell: PortGridCell, event: MouseEvent): void {
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  tooltip.value = { cell, x: rect.left + rect.width / 2, y: rect.top };
}

function hideTooltip(): void {
  tooltip.value = null;
}

onUnmounted(() => {
  if (isLoading.value) {
    void cancelPortTest();
  }
  detachListeners();
  if (flushTimer !== null) window.clearTimeout(flushTimer);
});
</script>

<template>
  <div class="space-y-5">
    <div class="grid grid-cols-1 gap-4 lg:grid-cols-[minmax(180px,1fr)_minmax(220px,1.4fr)_minmax(220px,1fr)]">
      <div>
        <label class="mb-1 block text-xs font-medium text-slate-600">
          {{ t('networkTools.port.targetHost') }}
        </label>
        <input
          v-model="host"
          type="text"
          :disabled="isLoading"
          :placeholder="t('networkTools.port.targetPlaceholder')"
          class="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm focus:border-transparent focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:cursor-not-allowed disabled:bg-slate-50"
        />
      </div>

      <div>
        <label class="mb-1 block text-xs font-medium text-slate-600">
          {{ t('networkTools.port.ports') }}
        </label>
        <input
          v-model="portsInput"
          type="text"
          :disabled="isLoading"
          :placeholder="t('networkTools.port.portsPlaceholder')"
          class="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm focus:border-transparent focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:cursor-not-allowed disabled:bg-slate-50"
        />
      </div>

      <div>
        <label class="mb-1 block text-xs font-medium text-slate-600">
          {{ t('networkTools.port.timeoutMs') }}
        </label>
        <div class="flex gap-2">
          <input
            v-model.number="timeoutMs"
            type="number"
            min="100"
            max="30000"
            :disabled="isLoading"
            class="min-w-0 flex-1 rounded-lg border border-slate-300 px-3 py-2 text-sm focus:border-transparent focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:cursor-not-allowed disabled:bg-slate-50"
          />
          <button
            v-if="!isLoading"
            @click="startTest"
            class="cursor-pointer whitespace-nowrap rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/30"
          >
            {{ t('networkTools.port.startTest') }}
          </button>
          <button
            v-else
            @click="stopTest"
            class="cursor-pointer whitespace-nowrap rounded-lg bg-red-500 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-red-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500/30"
          >
            {{ t('networkTools.port.stopTest') }}
          </button>
        </div>
      </div>
    </div>

    <div v-if="errorMsg" class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-600">
      {{ errorMsg }}
    </div>

    <div class="space-y-3">
      <div>
        <p class="mb-2 text-xs font-medium text-slate-500">{{ t('networkTools.port.presets') }}</p>
        <div class="flex flex-wrap gap-2">
          <button
            v-for="preset in builtinPresets"
            :key="preset.name"
            :disabled="isLoading"
            @click="applyPreset(preset.ports)"
            class="cursor-pointer rounded-full border border-slate-300 px-3 py-1 text-xs text-slate-600 transition-colors hover:border-blue-400 hover:bg-blue-100 hover:text-blue-700 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {{ preset.name }}
          </button>
        </div>
      </div>

      <div>
        <div class="mb-2 flex items-center gap-2">
          <p class="text-xs font-medium text-slate-500">{{ t('networkTools.port.customPresets') }}</p>
          <button
            v-if="!showPresetForm"
            @click="openAddPreset"
            :disabled="isLoading"
            class="cursor-pointer text-xs font-medium text-blue-600 transition-colors hover:text-blue-800 disabled:cursor-not-allowed disabled:opacity-50"
          >
            + {{ t('networkTools.port.addPreset') }}
          </button>
        </div>

        <div class="flex flex-wrap gap-2">
          <div
            v-for="(preset, index) in customPresets"
            :key="index"
            class="group relative flex items-center gap-1 rounded-full border border-slate-300 px-3 py-1 text-xs text-slate-600 transition-colors hover:border-blue-400 hover:bg-blue-50 hover:text-blue-700"
          >
            <button
              type="button"
              :disabled="isLoading"
              class="cursor-pointer disabled:cursor-not-allowed disabled:opacity-50"
              @click="applyPreset(preset.ports)"
            >
              {{ preset.name }}
            </button>
            <span class="ml-1 hidden items-center gap-0.5 group-hover:flex">
              <button
                type="button"
                :disabled="isLoading"
                class="cursor-pointer rounded p-0.5 text-blue-500 hover:bg-blue-50 hover:text-blue-700 disabled:cursor-not-allowed disabled:opacity-50"
                :title="t('networkTools.port.editPreset')"
                @click.stop="openEditPreset(index)"
              >
                <Pencil class="h-3 w-3" />
              </button>
              <button
                type="button"
                :disabled="isLoading"
                class="cursor-pointer rounded p-0.5 text-red-500 hover:bg-red-50 hover:text-red-700 disabled:cursor-not-allowed disabled:opacity-50"
                :title="t('networkTools.port.deletePreset')"
                @click.stop="deletePreset(index)"
              >
                <Trash2 class="h-3 w-3" />
              </button>
            </span>
          </div>

          <span v-if="customPresets.length === 0 && !showPresetForm" class="text-xs italic text-slate-400">
            -
          </span>
        </div>

        <div v-if="showPresetForm" class="mt-3 flex flex-wrap items-end gap-2 rounded-lg border border-slate-200 bg-slate-50 p-3">
          <div>
            <label class="mb-1 block text-xs font-medium text-slate-600">{{ t('networkTools.port.presetName') }}</label>
            <input
              v-model="presetName"
              type="text"
              placeholder="e.g. My Ports"
              class="rounded-md border border-slate-300 px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>
          <div>
            <label class="mb-1 block text-xs font-medium text-slate-600">{{ t('networkTools.port.presetPorts') }}</label>
            <input
              v-model="presetPorts"
              type="text"
              placeholder="80,443,8080"
              class="rounded-md border border-slate-300 px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>
          <div class="flex gap-2">
            <button
              @click="savePreset"
              class="cursor-pointer rounded-md bg-blue-600 px-3 py-1 text-xs font-medium text-white transition-colors hover:bg-blue-700"
            >
              {{ t('networkTools.port.save') }}
            </button>
            <button
              @click="cancelPresetForm"
              class="cursor-pointer rounded-md bg-slate-200 px-3 py-1 text-xs font-medium text-slate-700 transition-colors hover:bg-slate-300"
            >
              {{ t('networkTools.port.cancel') }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <div v-if="isLoading || result" class="space-y-3">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="text-sm text-slate-600">
          <span v-if="isLoading" class="font-medium text-amber-600">{{ t('networkTools.port.scanning') }}:</span>
          <span v-else class="font-medium text-emerald-600">{{ t('networkTools.port.complete') }}</span>
          <span class="ml-1 tabular-nums">{{ scannedCount }}/{{ totalPorts }}</span>
          <span class="mx-2 text-slate-300">|</span>
          <span class="text-emerald-600">{{ t('networkTools.port.open') }} {{ openCount }}</span>
          <span class="mx-1 text-slate-300"></span>
          <span class="text-slate-500">{{ t('networkTools.port.closed') }} {{ closedCount }}</span>
          <span v-if="result?.resolvedIp" class="ml-2 text-slate-400">({{ result.resolvedIp }})</span>
        </div>

        <div class="flex items-center gap-2">
          <div class="inline-flex gap-0.5 rounded-lg border border-slate-200 bg-slate-50 p-0.5">
            <button
              @click="viewMode = 'grid'"
              :class="[
                'cursor-pointer rounded-md px-3 py-1 text-xs font-medium transition',
                viewMode === 'grid' ? 'bg-white text-slate-800 shadow-sm' : 'text-slate-500 hover:text-slate-700',
              ]"
            >
              {{ t('networkTools.port.gridView') }}
            </button>
            <button
              @click="viewMode = 'table'"
              :class="[
                'cursor-pointer rounded-md px-3 py-1 text-xs font-medium transition',
                viewMode === 'table' ? 'bg-white text-slate-800 shadow-sm' : 'text-slate-500 hover:text-slate-700',
              ]"
            >
              {{ t('networkTools.port.tableView') }}
            </button>
          </div>

          <button
            v-if="scannedCount > 0"
            type="button"
            class="inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-xs font-medium text-slate-600 transition-colors hover:bg-slate-50 hover:text-slate-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/30"
            @click="copyResultSummary"
          >
            <Copy class="h-3.5 w-3.5" />
            {{ t('networkTools.port.copyResults') }}
          </button>
        </div>
      </div>

      <div class="h-1.5 w-full overflow-hidden rounded-full bg-slate-100">
        <div
          class="h-1.5 rounded-full transition-all duration-300"
          :class="isLoading ? 'bg-amber-400' : 'bg-emerald-500'"
          :style="{ width: `${progressPct}%` }"
        ></div>
      </div>

      <div v-if="viewMode === 'grid'" class="rounded-xl border border-slate-200 bg-white p-3 shadow-sm">
        <template v-if="isLargeScan">
          <div v-if="openPortCards.length > 0" class="max-h-[520px] overflow-auto pr-1">
            <div
              class="grid gap-3"
              :style="{ gridTemplateColumns: 'repeat(auto-fill, minmax(96px, 1fr))' }"
            >
              <div
                v-for="card in openPortCards"
                :key="card.port"
                class="flex flex-col items-center gap-1 rounded-lg border border-emerald-200 bg-emerald-50 p-3"
              >
                <div class="font-mono text-lg font-bold leading-none text-emerald-700">{{ card.port }}</div>
                <div class="w-full truncate text-center text-[11px] leading-tight text-slate-500">
                  {{ card.name || '—' }}
                </div>
                <div class="text-[10px] tabular-nums text-slate-400">
                  {{ card.latencyMs !== null ? `${card.latencyMs.toFixed(1)} ms` : '—' }}
                </div>
              </div>
            </div>
          </div>
          <Empty
            v-else
            :title="isLoading
              ? t('networkTools.port.scanningNoOpenYet', { scanned: scannedCount, total: totalPorts })
              : t('networkTools.port.completeNoOpen', { total: totalPorts })"
            dashed
          />
        </template>

        <template v-else>
          <div class="max-h-[520px] overflow-auto pr-1">
            <div class="grid" :style="gridStyle">
              <div
                v-for="cell in gridCells"
                :key="cell.port"
                :class="[gridCellBaseClass, portCellClass(cell.state)]"
                :aria-label="`${cell.port} ${portStatusLabel(cell.state)}`"
                @mouseenter="showTooltip(cell, $event)"
                @mouseleave="hideTooltip"
              >
                <span v-if="showCellLabels">{{ cell.port }}</span>
              </div>
            </div>
          </div>

          <div class="mt-3 flex flex-wrap gap-3 border-t border-slate-100 pt-3">
            <span class="flex items-center gap-1.5 text-xs text-slate-500">
              <span class="inline-block h-3 w-3 rounded bg-emerald-500"></span>
              {{ t('networkTools.port.open') }}
            </span>
            <span class="flex items-center gap-1.5 text-xs text-slate-500">
              <span class="inline-block h-3 w-3 rounded bg-slate-200"></span>
              {{ t('networkTools.port.closed') }}
            </span>
            <span class="flex items-center gap-1.5 text-xs text-slate-500">
              <span class="inline-block h-3 w-3 rounded bg-amber-400"></span>
              {{ t('networkTools.port.scanning') }}
            </span>
            <span class="flex items-center gap-1.5 text-xs text-slate-500">
              <span class="inline-block h-3 w-3 rounded bg-slate-700"></span>
              {{ t('networkTools.port.waiting') }}
            </span>
          </div>
        </template>
      </div>

      <div v-else class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        <div class="flex items-center gap-2 border-b border-slate-100 bg-slate-50/60 px-4 py-2.5">
          <span class="mr-1 text-xs font-medium text-slate-500">{{ t('networkTools.port.status') }}:</span>
          <label
            v-for="opt in (['all', 'open', 'closed'] as const)"
            :key="opt"
            class="inline-flex cursor-pointer items-center gap-1.5"
          >
            <input
              v-model="tableFilter"
              type="radio"
              :value="opt"
              class="accent-blue-600"
            />
            <span class="text-xs text-slate-600">
              {{ opt === 'all' ? t('networkTools.port.filterAll') : opt === 'open' ? t('networkTools.port.filterOpen') : t('networkTools.port.filterClosed') }}
            </span>
          </label>
        </div>

        <div class="max-h-[520px] overflow-auto">
          <table class="w-full">
            <thead>
              <tr class="border-b border-slate-100 bg-slate-50/80">
                <th scope="col" class="px-4 py-2.5 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                  {{ t('networkTools.port.port') }}
                </th>
                <th scope="col" class="px-4 py-2.5 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                  {{ t('networkTools.port.service') }}
                </th>
                <th scope="col" class="px-4 py-2.5 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                  {{ t('networkTools.port.status') }}
                </th>
                <th scope="col" class="px-4 py-2.5 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                  {{ t('networkTools.port.latency') }}
                </th>
              </tr>
            </thead>
            <tbody class="divide-y divide-slate-100">
              <tr
                v-for="row in tableRows"
                :key="row.port"
                class="transition-colors hover:bg-slate-50/60"
              >
                <td class="px-4 py-2.5 font-mono text-sm text-slate-800">{{ row.port }}</td>
                <td class="px-4 py-2.5 text-sm text-slate-600">{{ row.name || '-' }}</td>
                <td class="px-4 py-2.5">
                  <span
                    :class="[
                      'inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium',
                      row.open ? 'bg-emerald-50 text-emerald-700' : 'bg-slate-100 text-slate-500',
                    ]"
                  >
                    {{ row.open ? t('networkTools.port.open') : t('networkTools.port.closed') }}
                  </span>
                </td>
                <td class="px-4 py-2.5 text-sm tabular-nums text-slate-600">
                  {{ row.latencyMs !== null ? `${row.latencyMs.toFixed(1)} ms` : '-' }}
                </td>
              </tr>
              <tr v-if="tableRows.length === 0">
                <td colspan="4" class="px-4 py-6 text-center text-sm text-slate-400">
                  -
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>

    <Empty
      v-else
      :title="t('networkTools.port.emptyTitle')"
      :description="t('networkTools.port.emptyDescription')"
      dashed
    />

    <Teleport to="body">
      <div
        v-if="tooltip"
        class="fixed z-50 -mt-1 -translate-x-1/2 -translate-y-full whitespace-nowrap rounded-lg bg-slate-800 px-2.5 py-1.5 text-xs text-white shadow-lg pointer-events-none"
        :style="{ left: `${tooltip.x}px`, top: `${tooltip.y - 6}px` }"
      >
        <div class="font-mono font-medium">:{{ tooltip.cell.port }}</div>
        <div class="mt-0.5 text-slate-300">
          {{ portStatusLabel(tooltip.cell.state) }}
          <span v-if="tooltip.cell.name"> / {{ tooltip.cell.name }}</span>
          <span v-if="tooltip.cell.latencyMs !== null"> / {{ tooltip.cell.latencyMs.toFixed(1) }} ms</span>
        </div>
      </div>
    </Teleport>
  </div>
</template>
