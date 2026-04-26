<script setup lang="ts">
import { computed, ref, watch, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { Activity, RefreshCw, Search } from 'lucide-vue-next';
import { getTcpConnections, type TcpConnectionStats } from '../../lib/tauri';
import Empty from '../Empty.vue';
import LoadingSkeleton from '../LoadingSkeleton.vue';

defineOptions({ name: 'TcpConnectionsTab' });

const { t } = useI18n();
const AUTO_REFRESH_SECONDS = 5;

const stats = ref<TcpConnectionStats | null>(null);
const isLoading = ref(false);
const autoRefresh = ref(false);
const lastUpdate = ref('');
const filterText = ref('');
let refreshTimer: ReturnType<typeof setInterval> | null = null;

async function fetchData() {
  if (isLoading.value) return;
  isLoading.value = true;
  try {
    stats.value = await getTcpConnections();
    lastUpdate.value = new Date().toLocaleTimeString();
  } catch (e) {
    console.error('Failed to get TCP connections:', e);
  } finally {
    isLoading.value = false;
  }
}

watch(autoRefresh, (val) => {
  if (val) {
    refreshTimer = setInterval(fetchData, AUTO_REFRESH_SECONDS * 1000);
  } else {
    if (refreshTimer !== null) {
      clearInterval(refreshTimer);
      refreshTimer = null;
    }
  }
});

onMounted(() => {
  fetchData();
});

onUnmounted(() => {
  if (refreshTimer !== null) {
    clearInterval(refreshTimer);
    refreshTimer = null;
  }
});

// Top 5 states for summary cards
function getTopStates() {
  if (!stats.value) return [];
  return [...stats.value.byState]
    .sort((a, b) => b.count - a.count)
    .slice(0, 5);
}

function stateCardClasses(state: string): string {
  switch (state) {
    case 'ESTABLISHED':
      return 'border-green-200 bg-green-50 text-green-700';
    case 'TIME_WAIT':
      return 'border-yellow-200 bg-yellow-50 text-yellow-700';
    case 'CLOSE_WAIT':
      return 'border-pink-200 bg-pink-50 text-pink-700';
    case 'LISTEN':
    case 'LISTENING':
      return 'border-blue-200 bg-blue-50 text-blue-700';
    default:
      return 'border-slate-200 bg-slate-50 text-slate-600';
  }
}

function stateNumberClasses(state: string): string {
  switch (state) {
    case 'ESTABLISHED':
      return 'text-green-600';
    case 'TIME_WAIT':
      return 'text-yellow-600';
    case 'CLOSE_WAIT':
      return 'text-pink-600';
    case 'LISTEN':
    case 'LISTENING':
      return 'text-blue-600';
    default:
      return 'text-slate-700';
  }
}

function getMaxRemoteIpCount(): number {
  if (!stats.value || stats.value.byRemoteIp.length === 0) return 1;
  return Math.max(...stats.value.byRemoteIp.map(r => r.count));
}

function getMaxPortCount(): number {
  if (!stats.value || stats.value.byRemotePort.length === 0) return 1;
  return Math.max(...stats.value.byRemotePort.map(r => r.count));
}

function barWidth(count: number, max: number): string {
  if (max === 0) return '0%';
  return `${Math.round((count / max) * 100)}%`;
}

function portLabel(port: number, name: string): string {
  return name ? `:${port} (${name})` : `:${port}`;
}

const stateLegend = ['ESTABLISHED', 'TIME_WAIT', 'CLOSE_WAIT', 'LISTEN'];

const filteredRemoteIps = computed(() => {
  const query = filterText.value.trim().toLowerCase();
  const rows = stats.value?.byRemoteIp ?? [];
  if (!query) return rows.slice(0, 20);
  return rows.filter(item => item.ip.toLowerCase().includes(query)).slice(0, 20);
});

const filteredRemotePorts = computed(() => {
  const query = filterText.value.trim().toLowerCase();
  const rows = stats.value?.byRemotePort ?? [];
  if (!query) return rows.slice(0, 20);
  return rows.filter((item) => {
    const combined = `${item.port} ${item.name ?? ''}`.toLowerCase();
    return combined.includes(query);
  }).slice(0, 20);
});
</script>

<template>
  <div class="p-5 space-y-5">
    <!-- Controls Row -->
    <div class="flex items-center gap-4 flex-wrap">
      <button
        class="flex items-center gap-1.5 px-3 py-1.5 bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        :disabled="isLoading"
        @click="fetchData"
      >
        <RefreshCw class="w-3.5 h-3.5" :class="{ 'animate-spin': isLoading }" />
        {{ t('networkTools.tcp.refresh') }}
      </button>

      <label class="flex items-center gap-2 cursor-pointer select-none text-sm text-slate-600">
        <input
          v-model="autoRefresh"
          type="checkbox"
          class="w-4 h-4 rounded border-slate-300 text-blue-600 cursor-pointer"
        />
        {{ t('networkTools.tcp.autoRefresh') }}
      </label>

      <div class="relative min-w-[220px] flex-1 max-w-sm">
        <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
        <input
          v-model="filterText"
          type="search"
          :placeholder="t('networkTools.tcp.filterPlaceholder')"
          class="w-full rounded-xl border border-slate-200 bg-white px-10 py-2 text-sm text-slate-700 outline-none transition-colors focus:border-blue-400 focus-visible:ring-2 focus-visible:ring-blue-500/30"
        >
      </div>

      <span v-if="lastUpdate" class="text-xs text-slate-400">
        {{ t('networkTools.tcp.lastUpdate') }}: {{ lastUpdate }}
      </span>
      <span class="text-xs text-slate-400">
        {{ t('networkTools.tcp.autoRefreshHint', { seconds: AUTO_REFRESH_SECONDS }) }}
      </span>
    </div>

    <div class="flex flex-wrap items-center gap-2 text-xs text-slate-500">
      <span class="font-medium">{{ t('networkTools.tcp.stateLegend') }}</span>
      <span
        v-for="state in stateLegend"
        :key="state"
        class="inline-flex items-center gap-1 rounded-full border px-2.5 py-1"
        :class="stateCardClasses(state)"
      >
        <span class="h-1.5 w-1.5 rounded-full bg-current"></span>
        {{ state }}
      </span>
    </div>

    <!-- Summary Cards -->
    <div v-if="stats" class="grid grid-cols-5 gap-3">
      <div
        v-for="item in getTopStates()"
        :key="item.state"
        class="rounded-lg border p-3 text-center"
        :class="stateCardClasses(item.state)"
      >
        <div class="text-2xl font-bold" :class="stateNumberClasses(item.state)">
          {{ item.count }}
        </div>
        <div class="text-xs font-medium mt-1 truncate" :title="item.state">
          {{ item.state }}
        </div>
      </div>
    </div>

    <!-- Skeleton cards while loading and no data -->
    <div v-else-if="isLoading" class="grid grid-cols-5 gap-3">
      <div v-for="n in 5" :key="n" class="rounded-lg border border-slate-200 bg-slate-50 p-3 text-center">
        <LoadingSkeleton variant="text-line" :lines="2" />
      </div>
    </div>

    <!-- Two panels side by side -->
    <div class="grid grid-cols-2 gap-4">
      <!-- By Remote IP -->
      <div class="bg-white border border-slate-200 rounded-xl overflow-hidden">
        <div class="px-4 py-3 border-b border-slate-100 bg-slate-50">
          <h3 class="text-sm font-semibold text-slate-700">{{ t('networkTools.tcp.byRemoteIp') }}</h3>
        </div>
        <div class="overflow-y-auto max-h-80">
          <table class="w-full text-xs">
            <thead class="bg-white sticky top-0 z-10">
              <tr class="text-slate-500">
                <th scope="col" class="px-3 py-2 text-left font-semibold">{{ t('networkTools.tcp.remoteIpColumn') }}</th>
                <th scope="col" class="px-2 py-2 text-right font-semibold">{{ t('networkTools.tcp.countColumn') }}</th>
                <th scope="col" class="px-3 py-2 text-left font-semibold">{{ t('networkTools.tcp.shareColumn') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="item in filteredRemoteIps"
                :key="item.ip"
                class="border-b border-slate-50 hover:bg-slate-50 transition-colors"
              >
                <td class="px-3 py-2 font-mono text-slate-700 whitespace-nowrap w-36">
                  {{ item.ip }}
                </td>
                <td class="px-2 py-2 text-right text-slate-600 whitespace-nowrap w-10">
                  {{ item.count }}
                </td>
                <td class="px-3 py-2 w-full">
                  <div class="bg-slate-100 rounded-full h-2 overflow-hidden">
                    <div
                      class="bg-blue-500 h-2 rounded-full transition-all"
                      :style="{ width: barWidth(item.count, getMaxRemoteIpCount()) }"
                    ></div>
                  </div>
                </td>
              </tr>
              <tr v-if="!stats || filteredRemoteIps.length === 0">
                <td colspan="3" class="px-3 py-6">
                  <Empty
                    :icon="Activity"
                    :title="t(filterText ? 'networkTools.tcp.noMatchTitle' : 'networkTools.tcp.emptyTitle')"
                    :description="t(filterText ? 'networkTools.tcp.noMatchDescription' : 'networkTools.tcp.emptyDescription')"
                    dashed
                  />
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <!-- By Port -->
      <div class="bg-white border border-slate-200 rounded-xl overflow-hidden">
        <div class="px-4 py-3 border-b border-slate-100 bg-slate-50">
          <h3 class="text-sm font-semibold text-slate-700">{{ t('networkTools.tcp.byPort') }}</h3>
        </div>
        <div class="overflow-y-auto max-h-80">
          <table class="w-full text-xs">
            <thead class="bg-white sticky top-0 z-10">
              <tr class="text-slate-500">
                <th scope="col" class="px-3 py-2 text-left font-semibold">{{ t('networkTools.tcp.portColumn') }}</th>
                <th scope="col" class="px-2 py-2 text-right font-semibold">{{ t('networkTools.tcp.countColumn') }}</th>
                <th scope="col" class="px-3 py-2 text-left font-semibold">{{ t('networkTools.tcp.shareColumn') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="item in filteredRemotePorts"
                :key="item.port"
                class="border-b border-slate-50 hover:bg-slate-50 transition-colors"
              >
                <td class="px-3 py-2 font-mono text-slate-700 whitespace-nowrap w-36">
                  {{ portLabel(item.port, item.name) }}
                </td>
                <td class="px-2 py-2 text-right text-slate-600 whitespace-nowrap w-10">
                  {{ item.count }}
                </td>
                <td class="px-3 py-2 w-full">
                  <div class="bg-slate-100 rounded-full h-2 overflow-hidden">
                    <div
                      class="bg-blue-500 h-2 rounded-full transition-all"
                      :style="{ width: barWidth(item.count, getMaxPortCount()) }"
                    ></div>
                  </div>
                </td>
              </tr>
              <tr v-if="!stats || filteredRemotePorts.length === 0">
                <td colspan="3" class="px-3 py-6">
                  <Empty
                    :icon="Activity"
                    :title="t(filterText ? 'networkTools.tcp.noMatchTitle' : 'networkTools.tcp.emptyTitle')"
                    :description="t(filterText ? 'networkTools.tcp.noMatchDescription' : 'networkTools.tcp.emptyDescription')"
                    dashed
                  />
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  </div>
</template>
