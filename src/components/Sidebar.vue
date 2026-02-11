<script setup lang="ts">
import { Home, Settings, Activity, Server, ShieldCheck, History, ListChecks } from 'lucide-vue-next';
import { useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { computed } from 'vue';

const route = useRoute();
const { t } = useI18n();

const menuItems = computed(() => [
  { name: t('sidebar.console'), path: '/', icon: Activity },
  { name: t('sidebar.tasks'), path: '/tasks', icon: ListChecks },
  { name: t('sidebar.history'), path: '/history', icon: History },
  { name: t('sidebar.settings'), path: '/settings', icon: Settings },
]);
</script>

<template>
  <div class="w-56 bg-[#0f172a] text-white h-screen flex flex-col border-r border-slate-800 shadow-xl z-10">
    <div class="p-6 border-b border-slate-800 bg-slate-900/50">
      <h1 class="text-lg font-bold flex items-center gap-3 tracking-tight">
        <div class="w-8 h-8 bg-blue-600 rounded-md flex items-center justify-center shadow-lg shadow-blue-500/20 shrink-0">
          <Server class="w-5 h-5 text-white" />
        </div>
        <span class="bg-gradient-to-r from-blue-400 to-cyan-300 bg-clip-text text-transparent truncate">
          {{ t('sidebar.title') }}
        </span>
      </h1>
    </div>
    
    <nav class="flex-1 p-4 space-y-2">
      <router-link
        v-for="item in menuItems"
        :key="item.path"
        :to="item.path"
        class="group flex items-center gap-3 px-4 py-3 rounded-md transition-all duration-200 border border-transparent"
        :class="route.path === item.path 
          ? 'bg-blue-600/10 text-blue-400 border-blue-500/20 shadow-sm' 
          : 'text-slate-400 hover:bg-slate-800/50 hover:text-slate-200'"
      >
        <component 
          :is="item.icon" 
          class="w-5 h-5 transition-transform group-hover:scale-110"
          :class="route.path === item.path ? 'text-blue-400' : 'text-slate-500 group-hover:text-slate-300'"
        />
        <span class="font-medium tracking-wide">{{ item.name }}</span>
        <div v-if="route.path === item.path" class="ml-auto w-1.5 h-1.5 rounded-full bg-blue-400"></div>
      </router-link>
    </nav>

    <div class="p-6 border-t border-slate-800 bg-slate-900/30">
      <div class="flex items-center gap-3 text-xs text-slate-500 font-mono">
        <ShieldCheck class="w-4 h-4" />
        <span>{{ t('sidebar.version') }}</span>
      </div>
    </div>
  </div>
</template>
