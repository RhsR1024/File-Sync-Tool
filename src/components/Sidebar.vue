<script setup lang="ts">
import {
  Activity,
  BarChart3,
  Clipboard,
  Globe,
  History,
  KeyRound,
  ListChecks,
  MonitorUp,
  Server,
  Settings,
  Share2,
  Shield,
  ShieldCheck,
} from 'lucide-vue-next';
import { computed, type Component } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRoute } from 'vue-router';

import { SIDEBAR_NAV_SECTIONS, isSidebarItemActive, type SidebarIconKey } from '@/lib/sidebarNavigation';
import { appStore } from '@/lib/store';

const route = useRoute();
const { t } = useI18n();

const iconMap: Record<SidebarIconKey, Component> = {
  tasks: ListChecks,
  console: Activity,
  history: History,
  settings: Settings,
  toolsOverview: Server,
  frameworkPassword: KeyRound,
  applianceSsh: Shield,
  codeStatistics: BarChart3,
  networkTools: Globe,
  screenShare: MonitorUp,
  fileShare: Share2,
  clipboardManager: Clipboard,
};

const sections = computed(() =>
  SIDEBAR_NAV_SECTIONS.map((section) => ({
    ...section,
    label: t(section.labelKey),
    items: section.items.map((item) => ({
      ...item,
      label: t(item.labelKey),
      icon: iconMap[item.iconKey],
      active: isSidebarItemActive(route.path, item),
      runtimeActive: item.runtimeKey ? appStore.toolRuntime[item.runtimeKey] : false,
    })),
  })),
);
</script>

<template>
  <div class="flex h-screen w-64 flex-col border-r border-slate-800 bg-[#0b1220] text-white shadow-[14px_0_40px_rgba(2,6,23,0.34)] z-10">
    <div class="border-b border-slate-800/90 bg-slate-950/40 px-5 py-5">
      <h1 class="flex items-center gap-3 text-lg font-bold tracking-tight">
        <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-blue-600 to-cyan-500 shadow-lg shadow-blue-500/20">
          <Server class="h-5 w-5 text-white" />
        </div>
        <span class="truncate bg-gradient-to-r from-blue-300 via-cyan-200 to-slate-200 bg-clip-text text-transparent">
          {{ t('sidebar.title') }}
        </span>
      </h1>
    </div>

    <nav class="flex-1 overflow-y-auto px-3 py-4">
      <div class="space-y-5">
        <section v-for="section in sections" :key="section.key" class="space-y-2">
          <div class="flex items-center gap-2 px-1.5">
            <span class="text-[11px] font-semibold uppercase tracking-[0.18em] text-slate-500">{{ section.label }}</span>
            <span class="rounded-full border border-slate-800 bg-slate-950/70 px-1.5 py-0.5 text-[10px] font-semibold text-slate-500">
              {{ section.items.length }}
            </span>
            <span class="h-px flex-1 bg-slate-800/70"></span>
          </div>

          <div class="rounded-2xl border border-slate-800/80 bg-slate-900/65 p-2 shadow-[inset_0_1px_0_rgba(255,255,255,0.03),0_14px_28px_rgba(2,6,23,0.18)]">
            <router-link
              v-for="item in section.items"
              :key="item.path"
              :to="item.path"
              class="group flex items-start gap-3 rounded-xl border px-3 py-3 transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400/40"
              :class="item.active
                ? 'border-sky-500/25 bg-sky-500/10 text-slate-50 shadow-[0_10px_22px_rgba(14,165,233,0.10)]'
                : 'border-transparent text-slate-400 hover:border-slate-800 hover:bg-slate-800/70 hover:text-slate-100'"
            >
              <component
                :is="item.icon"
                class="mt-0.5 h-[18px] w-[18px] shrink-0 transition-transform duration-200 group-hover:scale-110"
                :class="item.active ? 'text-sky-300' : 'text-slate-500 group-hover:text-slate-300'"
              />

              <span class="min-w-0 flex-1 text-sm font-medium leading-5 tracking-[0.01em]">
                {{ item.label }}
              </span>

              <div class="ml-auto flex items-center gap-2 pl-2">
                <span
                  v-if="item.runtimeActive"
                  class="h-2.5 w-2.5 shrink-0 rounded-full bg-emerald-400 shadow-[0_0_0_3px_rgba(52,211,153,0.12)]"
                  :class="item.active ? 'animate-pulse' : ''"
                  aria-hidden="true"
                ></span>
                <span
                  v-if="item.active"
                  class="h-7 w-1 shrink-0 rounded-full bg-gradient-to-b from-sky-300 to-cyan-300"
                  aria-hidden="true"
                ></span>
              </div>
            </router-link>
          </div>
        </section>
      </div>
    </nav>

    <div class="border-t border-slate-800/90 bg-slate-950/25 px-5 py-4">
      <div class="flex items-center gap-3 text-xs font-mono text-slate-500">
        <ShieldCheck class="h-4 w-4" />
        <span>{{ t('sidebar.version') }}</span>
      </div>
    </div>
  </div>
</template>
