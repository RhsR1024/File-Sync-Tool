<script setup lang="ts">
import { ref, computed, markRaw, type Component, onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { Globe } from 'lucide-vue-next';
import PingScanTab from '../components/network/PingScanTab.vue';
import TcpConnectionsTab from '../components/network/TcpConnectionsTab.vue';
import PortTestTab from '../components/network/PortTestTab.vue';
import WakeOnLanTab from '../components/network/WakeOnLanTab.vue';
import SubnetCalcTab from '../components/network/SubnetCalcTab.vue';

defineOptions({
  name: 'NetworkToolsPage',
});

const { t } = useI18n();

interface Tab {
  id: string;
  label: string;
  component: Component;
}

const STORAGE_KEY = 'networkTools.activeTab';
const tablistRef = ref<HTMLElement | null>(null);

const tabs: Tab[] = [
  { id: 'ping', label: 'networkTools.tabs.pingScan', component: markRaw(PingScanTab) },
  { id: 'tcp', label: 'networkTools.tabs.tcpConnections', component: markRaw(TcpConnectionsTab) },
  { id: 'port', label: 'networkTools.tabs.portTest', component: markRaw(PortTestTab) },
  { id: 'wol', label: 'networkTools.tabs.wol', component: markRaw(WakeOnLanTab) },
  { id: 'subnet', label: 'networkTools.tabs.subnetCalc', component: markRaw(SubnetCalcTab) },
];

const activeTab = ref('ping');
const activeComponent = computed(() => tabs.find(tab => tab.id === activeTab.value)?.component);

function setActiveTab(id: string) {
  if (!tabs.some(tab => tab.id === id)) {
    return;
  }
  activeTab.value = id;
  try {
    window.localStorage.setItem(STORAGE_KEY, id);
  } catch {
    // Persisting the last-opened tab is best-effort only.
  }
}

function focusTab(index: number) {
  const button = tablistRef.value?.querySelector<HTMLButtonElement>(`[data-tab-index="${index}"]`);
  button?.focus();
}

function onTabKeydown(event: KeyboardEvent, index: number) {
  const lastIndex = tabs.length - 1;
  if (event.key === 'ArrowRight') {
    event.preventDefault();
    const nextIndex = index >= lastIndex ? 0 : index + 1;
    setActiveTab(tabs[nextIndex].id);
    focusTab(nextIndex);
  } else if (event.key === 'ArrowLeft') {
    event.preventDefault();
    const nextIndex = index <= 0 ? lastIndex : index - 1;
    setActiveTab(tabs[nextIndex].id);
    focusTab(nextIndex);
  } else if (event.key === 'Home') {
    event.preventDefault();
    setActiveTab(tabs[0].id);
    focusTab(0);
  } else if (event.key === 'End') {
    event.preventDefault();
    setActiveTab(tabs[lastIndex].id);
    focusTab(lastIndex);
  }
}

onMounted(() => {
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved && tabs.some(tab => tab.id === saved)) {
      activeTab.value = saved;
    }
  } catch {
    // Ignore malformed saved state.
  }
});
</script>

<template>
  <div class="flex-1 flex flex-col bg-gradient-to-br from-slate-50 to-slate-100 overflow-y-auto">
    <div class="max-w-6xl w-full mx-auto p-6 pb-10 space-y-5">
      <!-- Header -->
      <div class="flex items-center gap-3">
        <div class="w-10 h-10 rounded-xl bg-gradient-to-br from-blue-500 to-indigo-600 flex items-center justify-center shadow-sm">
          <Globe class="w-5 h-5 text-white" />
        </div>
        <div>
          <h1 class="text-2xl font-bold text-slate-900">{{ t('networkTools.title') }}</h1>
        </div>
      </div>

      <!-- Tab container -->
      <div class="bg-white border border-slate-200/80 rounded-xl shadow-sm overflow-hidden">
        <!-- Tab bar -->
        <div
          ref="tablistRef"
          class="border-b border-slate-200 flex overflow-x-auto"
          role="tablist"
          :aria-label="t('networkTools.title')"
        >
          <button
            v-for="(tab, index) in tabs"
            :key="tab.id"
            :id="`network-tools-tab-${tab.id}`"
            :data-tab-index="index"
            type="button"
            role="tab"
            :aria-selected="activeTab === tab.id"
            :aria-controls="`network-tools-panel-${tab.id}`"
            :tabindex="activeTab === tab.id ? 0 : -1"
            @click="setActiveTab(tab.id)"
            @keydown="onTabKeydown($event, index)"
            class="px-5 py-3 text-sm font-medium whitespace-nowrap transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/40 focus-visible:ring-inset"
            :class="activeTab === tab.id
              ? 'text-blue-600 border-b-2 border-blue-600 bg-blue-50/50'
              : 'text-slate-500 hover:text-slate-700 hover:bg-slate-50 border-b-2 border-transparent'"
          >
            {{ t(tab.label) }}
          </button>
        </div>

        <!-- Tab content -->
        <div
          :id="`network-tools-panel-${activeTab}`"
          class="p-5"
          role="tabpanel"
          :aria-labelledby="`network-tools-tab-${activeTab}`"
        >
          <keep-alive>
            <component :is="activeComponent" />
          </keep-alive>
        </div>
      </div>
    </div>
  </div>
</template>
