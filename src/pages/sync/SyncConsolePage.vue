<script setup lang="ts">
import { Activity, ListChecks, Play, Send, Square } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';
import { appStore } from '@/lib/store';
import { startScheduler, stopScheduler } from '@/lib/scheduler';

defineOptions({ name: 'SyncConsolePage' });

const { t } = useI18n();
const tabs = [
  { key: 'overview', path: '/sync', labelKey: 'sync.tabs.overview', icon: Activity, exact: true },
  { key: 'tasks', path: '/sync/tasks', labelKey: 'sync.tabs.tasks', icon: ListChecks },
  { key: 'delivery', path: '/sync/delivery', labelKey: 'sync.tabs.delivery', icon: Send },
] as const;

function toggleScheduler() {
  if (appStore.isRunning) {
    stopScheduler();
  } else {
    startScheduler();
  }
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-slate-50">
    <header class="shrink-0 border-b border-slate-200 bg-white px-7 pt-5 shadow-[0_1px_2px_rgba(15,23,42,0.03)]">
      <div class="sync-console-workspace w-full">
        <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
          <div class="min-w-0">
            <h1 class="text-[21px] font-extrabold tracking-tight text-slate-950">{{ t('sync.title') }}</h1>
            <p class="mt-1 max-w-3xl text-[13px] leading-5 text-slate-500">{{ t('sync.description') }}</p>
          </div>

          <div class="flex flex-wrap items-center gap-3 xl:justify-end">
            <div
              class="inline-flex min-h-9 items-center gap-2 rounded-full border px-3.5 py-1.5 text-xs font-bold"
              :class="appStore.isRunning
                ? 'border-emerald-200 bg-emerald-50 text-emerald-700'
                : 'border-slate-200 bg-slate-100 text-slate-600'"
            >
              <span class="relative flex h-2 w-2" aria-hidden="true">
                <span
                  v-if="appStore.isRunning"
                  class="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-70 motion-reduce:animate-none"
                ></span>
                <span
                  class="relative inline-flex h-2 w-2 rounded-full"
                  :class="appStore.isRunning ? 'bg-emerald-500' : 'bg-slate-400'"
                ></span>
              </span>
              {{ appStore.isRunning ? t('console.running') : t('console.stopped') }}
            </div>

            <button
              type="button"
              class="inline-flex min-h-10 items-center justify-center gap-2 rounded-lg border px-4 py-2 text-sm font-bold shadow-sm transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2"
              :class="appStore.isRunning
                ? 'border-red-200 bg-red-50 text-red-600 hover:bg-red-100 focus-visible:ring-red-500/50'
                : 'border-emerald-600 bg-emerald-600 text-white hover:bg-emerald-700 focus-visible:ring-emerald-500/50'"
              :aria-label="appStore.isRunning ? t('console.stop') : t('console.start')"
              :title="appStore.isRunning ? t('console.stop') : t('console.start')"
              @click="toggleScheduler"
            >
              <component :is="appStore.isRunning ? Square : Play" class="h-4 w-4 fill-current" aria-hidden="true" />
              {{ appStore.isRunning ? t('console.stop') : t('console.start') }}
            </button>
          </div>
        </div>

        <nav class="mt-4 flex gap-1 overflow-x-auto pb-3" :aria-label="t('sync.navigation')">
          <router-link
            v-for="tab in tabs"
            :key="tab.key"
            :to="tab.path"
            class="group relative flex min-h-10 shrink-0 items-center gap-2 rounded-lg px-4 py-2 text-[13px] font-semibold transition-colors motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50 focus-visible:ring-offset-2"
            :class="$route.path === tab.path
              ? 'bg-blue-600 text-white shadow-sm shadow-blue-600/15'
              : 'text-slate-500 hover:bg-slate-100 hover:text-slate-800'"
            :aria-current="$route.path === tab.path ? 'page' : undefined"
          >
            <component :is="tab.icon" class="h-4 w-4" aria-hidden="true" />
            {{ t(tab.labelKey) }}
          </router-link>
        </nav>
      </div>
    </header>

    <div class="min-h-0 flex-1">
      <router-view v-slot="{ Component }">
        <keep-alive include="SyncOverviewPage,SyncTasksPage,SyncDeliveryPage">
          <component :is="Component" />
        </keep-alive>
      </router-view>
    </div>
  </div>
</template>
