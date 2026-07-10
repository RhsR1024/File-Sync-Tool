<script setup lang="ts">
import { Activity, ListChecks, ScanSearch, Send } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';

defineOptions({ name: 'SyncConsolePage' });

const { t } = useI18n();
const tabs = [
  { key: 'overview', path: '/sync', labelKey: 'sync.tabs.overview', icon: Activity, exact: true },
  { key: 'tasks', path: '/sync/tasks', labelKey: 'sync.tabs.tasks', icon: ListChecks },
  { key: 'strategy', path: '/sync/strategy', labelKey: 'sync.tabs.strategy', icon: ScanSearch },
  { key: 'delivery', path: '/sync/delivery', labelKey: 'sync.tabs.delivery', icon: Send },
] as const;
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-slate-50">
    <header class="shrink-0 border-b border-slate-200/80 bg-white px-6 pt-4 shadow-sm">
      <div class="sync-console-workspace mx-auto w-full">
        <div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
          <div class="min-w-0">
            <h1 class="text-[22px] font-bold tracking-tight text-slate-950">{{ t('sync.title') }}</h1>
            <p class="mt-1 max-w-3xl text-sm leading-6 text-slate-500">{{ t('sync.description') }}</p>
          </div>
        </div>

        <nav class="mt-4 flex gap-1 overflow-x-auto pb-3" :aria-label="t('sync.navigation')">
          <router-link
            v-for="tab in tabs"
            :key="tab.key"
            :to="tab.path"
            class="group relative flex min-h-11 shrink-0 items-center gap-2 rounded-lg px-3.5 py-2 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50 focus-visible:ring-offset-2"
            :class="$route.path === tab.path
              ? 'bg-blue-600 text-white shadow-sm shadow-blue-600/15'
              : 'text-slate-500 hover:bg-slate-100 hover:text-slate-800'"
            :aria-current="$route.path === tab.path ? 'page' : undefined"
          >
            <component :is="tab.icon" class="h-4 w-4" aria-hidden="true" />
            {{ t(tab.labelKey) }}
            <span
              class="absolute inset-x-3 bottom-1 h-0.5 rounded-full transition-colors"
              :class="$route.path === tab.path ? 'bg-white/80' : 'bg-transparent group-hover:bg-slate-300'"
              aria-hidden="true"
            ></span>
          </router-link>
        </nav>
      </div>
    </header>

    <div class="min-h-0 flex-1">
      <router-view v-slot="{ Component }">
        <keep-alive include="SyncOverviewPage,SyncTasksPage,SyncStrategyPage,SyncDeliveryPage">
          <component :is="Component" />
        </keep-alive>
      </router-view>
    </div>
  </div>
</template>
