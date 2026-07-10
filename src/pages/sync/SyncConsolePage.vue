<script setup lang="ts">
import { Activity, ListChecks, ScanSearch, Send, TerminalSquare } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';

defineOptions({ name: 'SyncConsolePage' });

const { t } = useI18n();
const tabs = [
  { key: 'overview', path: '/sync', labelKey: 'sync.tabs.overview', icon: Activity, exact: true },
  { key: 'tasks', path: '/sync/tasks', labelKey: 'sync.tabs.tasks', icon: ListChecks },
  { key: 'strategy', path: '/sync/strategy', labelKey: 'sync.tabs.strategy', icon: ScanSearch },
  { key: 'delivery', path: '/sync/delivery', labelKey: 'sync.tabs.delivery', icon: Send },
  { key: 'logs', path: '/sync/logs', labelKey: 'sync.tabs.logs', icon: TerminalSquare },
] as const;
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-slate-50">
    <header class="shrink-0 border-b border-slate-200 bg-white px-6 pt-5 shadow-sm">
      <div class="mx-auto max-w-7xl">
        <div class="flex items-start justify-between gap-6">
          <div>
            <h1 class="text-xl font-bold tracking-tight text-slate-950">{{ t('sync.title') }}</h1>
            <p class="mt-1 text-sm text-slate-500">{{ t('sync.description') }}</p>
          </div>
        </div>

        <nav class="mt-5 flex gap-1 overflow-x-auto" :aria-label="t('sync.navigation')">
          <router-link
            v-for="tab in tabs"
            :key="tab.key"
            :to="tab.path"
            class="group relative flex shrink-0 items-center gap-2 rounded-t-lg px-4 py-3 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50"
            :class="$route.path === tab.path
              ? 'bg-blue-50/70 text-blue-700'
              : 'text-slate-500 hover:bg-slate-50 hover:text-slate-800'"
            :aria-current="$route.path === tab.path ? 'page' : undefined"
          >
            <component :is="tab.icon" class="h-4 w-4" aria-hidden="true" />
            {{ t(tab.labelKey) }}
            <span
              class="absolute inset-x-3 bottom-0 h-0.5 rounded-full transition-colors"
              :class="$route.path === tab.path ? 'bg-blue-600' : 'bg-transparent group-hover:bg-slate-200'"
              aria-hidden="true"
            ></span>
          </router-link>
        </nav>
      </div>
    </header>

    <div class="min-h-0 flex-1">
      <router-view v-slot="{ Component }">
        <keep-alive include="SyncOverviewPage,SyncTasksPage,SyncStrategyPage,SyncDeliveryPage,SyncLogsPage">
          <component :is="Component" />
        </keep-alive>
      </router-view>
    </div>
  </div>
</template>
