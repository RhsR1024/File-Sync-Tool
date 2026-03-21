<script setup lang="ts">
import { Settings, Activity, Server, ShieldCheck, History, ListChecks, ChevronDown } from 'lucide-vue-next';
import { useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { computed, ref } from 'vue';

const route = useRoute();
const { t } = useI18n();
const expandedMenus = ref<Record<string, boolean>>({ tools: false });

interface MenuItem {
  name: string;
  path?: string;
  icon?: any;
  children?: MenuItem[];
  id?: string;
}

const menuItems = computed<MenuItem[]>(() => [
  { name: t('sidebar.tasks'), path: '/tasks', icon: ListChecks },
  { name: t('sidebar.console'), path: '/', icon: Activity },
  { name: t('sidebar.history'), path: '/history', icon: History },
  { name: t('sidebar.settings'), path: '/settings', icon: Settings },
  {
    id: 'tools',
    name: t('sidebar.tools'),
    icon: Server,
    children: [
      { name: t('sidebar.frameworkPassword'), path: '/tools/framework-password' },
      { name: t('sidebar.applianceSsh'), path: '/tools/appliance-ssh' },
      { name: t('sidebar.codeStatistics'), path: '/tools/code-statistics' },
    ],
  },
]);

const toggleMenu = (id: string) => {
  expandedMenus.value[id] = !expandedMenus.value[id];
};

const isRouteActive = (path?: string) => {
  if (!path) return false;
  return route.path === path || route.path.startsWith(path + '/');
};
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

    <nav class="flex-1 p-4 space-y-2 overflow-y-auto">
      <template v-for="item in menuItems" :key="item.path || item.id">
        <!-- Regular menu item (no children) -->
        <router-link
          v-if="!item.children"
          :to="item.path!"
          class="group flex items-center gap-3 px-4 py-3 rounded-md transition-all duration-200 border border-transparent"
          :class="isRouteActive(item.path)
            ? 'bg-blue-600/10 text-blue-400 border-blue-500/20 shadow-sm'
            : 'text-slate-400 hover:bg-slate-800/50 hover:text-slate-200'"
        >
          <component
            :is="item.icon"
            class="w-5 h-5 transition-transform group-hover:scale-110"
            :class="isRouteActive(item.path) ? 'text-blue-400' : 'text-slate-500 group-hover:text-slate-300'"
          />
          <span class="font-medium tracking-wide">{{ item.name }}</span>
          <div v-if="isRouteActive(item.path)" class="ml-auto w-1.5 h-1.5 rounded-full bg-blue-400"></div>
        </router-link>

        <!-- Expandable menu item (with children) -->
        <div v-else class="space-y-1">
          <button
            @click="toggleMenu(item.id!)"
            class="w-full group flex items-center gap-3 px-4 py-3 rounded-md transition-all duration-200 border border-transparent text-slate-400 hover:bg-slate-800/50 hover:text-slate-200"
          >
            <component
              :is="item.icon"
              class="w-5 h-5 transition-transform group-hover:scale-110 text-slate-500 group-hover:text-slate-300"
            />
            <span class="font-medium tracking-wide">{{ item.name }}</span>
            <ChevronDown
              class="ml-auto w-4 h-4 transition-transform"
              :class="{ 'rotate-180': expandedMenus[item.id!] }"
            />
          </button>

          <!-- Children items -->
          <transition name="slide">
            <div v-show="expandedMenus[item.id!]" class="pl-2 space-y-1">
              <router-link
                v-for="child in item.children"
                :key="child.path"
                :to="child.path!"
                class="group flex items-center gap-3 px-4 py-2 rounded-md transition-all duration-200 border border-transparent text-sm"
                :class="isRouteActive(child.path)
                  ? 'bg-blue-600/10 text-blue-400 border-blue-500/20 shadow-sm'
                  : 'text-slate-400 hover:bg-slate-800/50 hover:text-slate-200'"
              >
                <div class="w-1.5 h-1.5 rounded-full bg-current"></div>
                <span class="font-medium tracking-wide">{{ child.name }}</span>
                <div v-if="isRouteActive(child.path)" class="ml-auto w-1 h-1 rounded-full bg-blue-400"></div>
              </router-link>
            </div>
          </transition>
        </div>
      </template>
    </nav>

    <div class="p-6 border-t border-slate-800 bg-slate-900/30">
      <div class="flex items-center gap-3 text-xs text-slate-500 font-mono">
        <ShieldCheck class="w-4 h-4" />
        <span>{{ t('sidebar.version') }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.slide-enter-active,
.slide-leave-active {
  transition: all 0.3s ease;
}

.slide-enter-from {
  opacity: 0;
  max-height: 0;
}

.slide-leave-to {
  opacity: 0;
  max-height: 0;
}
</style>
