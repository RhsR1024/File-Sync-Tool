<script setup lang="ts">
import { computed, markRaw, type Component } from 'vue';
import { useI18n } from 'vue-i18n';
import { ArrowRight, BarChart3, Clipboard, Globe, KeyRound, MonitorUp, Share2, Shield, type LucideIcon } from 'lucide-vue-next';
import { useRouter } from 'vue-router';
import { appStore } from '@/lib/store';

defineOptions({
  name: 'ToolsHubPage',
});

const { t } = useI18n();
const router = useRouter();

interface ToolCard {
  key: string;
  titleKey: string;
  descriptionKey: string;
  path: string;
  icon: Component;
  iconClasses: string;
  chipKey: string;
}

function isToolActive(key: string) {
  if (key === 'screen-share') {
    return appStore.toolRuntime.screenShare;
  }

  if (key === 'file-share') {
    return appStore.toolRuntime.fileShare;
  }

  return false;
}

const toolCards = computed<ToolCard[]>(() => [
  {
    key: 'framework-password',
    titleKey: 'sidebar.frameworkPassword',
    descriptionKey: 'tools.frameworkPassword.description',
    path: '/tools/framework-password',
    icon: markRaw(KeyRound as LucideIcon),
    iconClasses: 'from-amber-500 to-orange-600 shadow-amber-500/20',
    chipKey: 'toolsHub.cards.frameworkPassword.chip',
  },
  {
    key: 'appliance-ssh',
    titleKey: 'sidebar.applianceSsh',
    descriptionKey: 'tools.applianceSsh.description',
    path: '/tools/appliance-ssh',
    icon: markRaw(Shield as LucideIcon),
    iconClasses: 'from-sky-500 to-indigo-600 shadow-sky-500/20',
    chipKey: 'toolsHub.cards.applianceSsh.chip',
  },
  {
    key: 'code-statistics',
    titleKey: 'sidebar.codeStatistics',
    descriptionKey: 'codeStatistics.description',
    path: '/tools/code-statistics',
    icon: markRaw(BarChart3 as LucideIcon),
    iconClasses: 'from-emerald-500 to-teal-600 shadow-emerald-500/20',
    chipKey: 'toolsHub.cards.codeStatistics.chip',
  },
  {
    key: 'network-tools',
    titleKey: 'sidebar.networkTools',
    descriptionKey: 'toolsHub.cards.networkTools.description',
    path: '/tools/network',
    icon: markRaw(Globe as LucideIcon),
    iconClasses: 'from-violet-500 to-fuchsia-600 shadow-violet-500/20',
    chipKey: 'toolsHub.cards.networkTools.chip',
  },
  {
    key: 'screen-share',
    titleKey: 'sidebar.screenShare',
    descriptionKey: 'toolsHub.cards.screenShare.description',
    path: '/tools/screen-share',
    icon: markRaw(MonitorUp as LucideIcon),
    iconClasses: 'from-purple-500 to-indigo-600 shadow-purple-500/20',
    chipKey: 'toolsHub.cards.screenShare.chip',
  },
  {
    key: 'file-share',
    titleKey: 'sidebar.fileShare',
    descriptionKey: 'toolsHub.cards.fileShare.description',
    path: '/tools/file-share',
    icon: markRaw(Share2 as LucideIcon),
    iconClasses: 'from-cyan-500 to-teal-600 shadow-cyan-500/20',
    chipKey: 'toolsHub.cards.fileShare.chip',
  },
  {
    key: 'clipboard-manager',
    titleKey: 'sidebar.clipboardManager',
    descriptionKey: 'toolsHub.cards.clipboardManager.description',
    path: '/tools/clipboard',
    icon: markRaw(Clipboard as LucideIcon),
    iconClasses: 'from-rose-500 to-pink-600 shadow-rose-500/20',
    chipKey: 'toolsHub.cards.clipboardManager.chip',
  },
]);
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-[radial-gradient(circle_at_top_left,_rgba(59,130,246,0.16),_transparent_30%),linear-gradient(180deg,_#f8fbff_0%,_#eef4fb_42%,_#f8fafc_100%)]">
    <div class="mx-auto flex w-full max-w-6xl flex-col gap-8 px-6 py-6 pb-10">
      <section class="relative overflow-hidden rounded-[28px] border border-white/70 bg-white/80 px-6 py-7 shadow-[0_18px_60px_rgba(15,23,42,0.08)] backdrop-blur">
        <div class="absolute -right-16 -top-16 h-40 w-40 rounded-full bg-sky-100/80 blur-3xl"></div>
        <div class="absolute bottom-0 right-8 h-24 w-24 rounded-full bg-amber-100/70 blur-2xl"></div>

        <div class="relative flex items-center justify-between gap-6">
          <div class="space-y-2">
            <div class="flex items-center gap-2">
              <span class="h-1.5 w-1.5 rounded-full bg-blue-500"></span>
              <span class="text-[11px] font-bold uppercase tracking-[0.12em] text-slate-500">{{ t('toolsHub.eyebrow') }}</span>
            </div>
            <h1 class="text-2xl font-bold tracking-tight text-slate-950">
              {{ t('toolsHub.title') }}
            </h1>
            <p class="text-sm text-slate-500">{{ t('toolsHub.description') }}</p>
          </div>

          <div class="flex shrink-0 gap-2.5">
            <div class="flex h-11 w-11 items-center justify-center rounded-[14px] bg-gradient-to-br from-amber-500 to-orange-600 shadow-lg shadow-amber-500/25">
              <KeyRound class="h-5 w-5 text-white" />
            </div>
            <div class="flex h-11 w-11 items-center justify-center rounded-[14px] bg-gradient-to-br from-sky-400 to-indigo-500 shadow-lg shadow-sky-400/25">
              <Shield class="h-5 w-5 text-white" />
            </div>
            <div class="flex h-11 w-11 items-center justify-center rounded-[14px] bg-gradient-to-br from-emerald-500 to-teal-500 shadow-lg shadow-emerald-500/25">
              <BarChart3 class="h-5 w-5 text-white" />
            </div>
            <div class="flex h-11 w-11 items-center justify-center rounded-[14px] bg-gradient-to-br from-violet-500 to-fuchsia-500 shadow-lg shadow-violet-500/25">
              <Globe class="h-5 w-5 text-white" />
            </div>
            <div class="flex h-11 w-11 items-center justify-center rounded-[14px] bg-gradient-to-br from-purple-500 to-indigo-500 shadow-lg shadow-purple-500/25">
              <MonitorUp class="h-5 w-5 text-white" />
            </div>
            <div class="flex h-11 w-11 items-center justify-center rounded-[14px] bg-gradient-to-br from-cyan-500 to-teal-500 shadow-lg shadow-cyan-500/25">
              <Share2 class="h-5 w-5 text-white" />
            </div>
            <div class="flex h-11 w-11 items-center justify-center rounded-[14px] bg-gradient-to-br from-rose-500 to-pink-600 shadow-lg shadow-rose-500/25">
              <Clipboard class="h-5 w-5 text-white" />
            </div>
          </div>
        </div>
      </section>

      <section class="grid grid-cols-1 gap-5 md:grid-cols-2 xl:grid-cols-4">
        <article
          v-for="card in toolCards"
          :key="card.key"
          class="group flex min-h-[320px] flex-col rounded-[24px] border border-slate-200/80 bg-white/90 p-5 shadow-[0_14px_40px_rgba(15,23,42,0.06)] transition-all duration-200 hover:-translate-y-1 hover:border-slate-300 hover:shadow-[0_22px_48px_rgba(15,23,42,0.10)]"
        >
          <div class="flex items-start justify-between gap-3">
            <div
              class="relative flex h-14 w-14 items-center justify-center rounded-2xl bg-gradient-to-br text-white shadow-lg"
              :class="card.iconClasses"
            >
              <span
                v-if="isToolActive(card.key)"
                class="absolute -right-1 -top-1 flex h-4 w-4 items-center justify-center"
                aria-hidden="true"
              >
                <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400/70"></span>
                <span class="relative inline-flex h-2.5 w-2.5 rounded-full border border-white bg-emerald-500 shadow-[0_0_0_2px_rgba(16,185,129,0.18)]"></span>
              </span>
              <component :is="card.icon" class="h-6 w-6" />
            </div>
            <span class="inline-flex rounded-full border border-slate-200 bg-slate-50 px-2.5 py-1 text-[11px] font-semibold tracking-[0.14em] text-slate-500 uppercase">
              {{ t(card.chipKey) }}
            </span>
          </div>

          <div class="mt-5 flex flex-1 flex-col">
            <h2 class="min-h-[3.5rem] text-xl font-bold leading-7 text-slate-950">
              {{ t(card.titleKey) }}
            </h2>
            <p class="mt-3 min-h-[5.25rem] text-sm leading-7 text-slate-600 tools-card-description">
              {{ t(card.descriptionKey) }}
            </p>

            <div class="mt-auto pt-6">
              <button
                type="button"
                class="inline-flex w-full items-center justify-between rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3 text-left text-sm font-semibold text-slate-800 transition-colors hover:border-slate-300 hover:bg-slate-100 focus:outline-none focus:ring-2 focus:ring-sky-500/30"
                @click="router.push(card.path)"
              >
                <span>{{ t('toolsHub.openAction') }}</span>
                <ArrowRight class="h-4 w-4 transition-transform group-hover:translate-x-0.5" />
              </button>
            </div>
          </div>
        </article>
      </section>
    </div>
  </div>
</template>

<style scoped>
.tools-card-description {
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
</style>
