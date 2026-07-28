<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import VideoDeviceSimulatorPage from '@/pages/VideoDeviceSimulatorPage.vue';
import { useDeviceSimulator } from '@/composables/useDeviceSimulator';
import { currentScenario, setScenario, type PreviewScenario } from './mock-backend';

const { locale } = useI18n();
const simulator = useDeviceSimulator();

// Remounting is the honest way to show a scenario: the page keeps its draft and
// its last check result in a shared store, exactly as it does in the app.
const pageKey = ref(0);
const scenario = ref<PreviewScenario>(currentScenario());

/**
 * In the app the check runs when the user presses start. Here it is run for
 * them, so picking a scenario lands directly on the banner it is meant to show
 * rather than requiring a start first.
 */
function showCheckResult() {
  if (scenario.value === 'running') return;
  window.setTimeout(() => { void simulator.runPreflight(); }, 500);
}

onMounted(showCheckResult);

const SCENARIOS: { id: PreviewScenario; label: string; hint: string }[] = [
  { id: 'clear', label: '检查通过', hint: '全部通过，绿色横幅' },
  { id: 'warning', label: '有警告', hint: '地址未确认 + 服务器未验证' },
  { id: 'blocked', label: '有失败项', hint: '地址被占用，开启被拦截' },
  { id: 'running', label: '运行中', hint: '配置锁定，实况分页有数据' },
];

function choose(next: PreviewScenario) {
  scenario.value = next;
  setScenario(next);
  pageKey.value += 1;
  showCheckResult();
}
</script>

<template>
  <div class="flex h-screen flex-col bg-slate-100">
    <header class="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-slate-300 bg-slate-900 px-4 py-2.5 text-white">
      <span class="text-xs font-bold uppercase tracking-[0.18em] text-slate-400">界面预览</span>
      <div class="flex flex-wrap gap-1.5">
        <button
          v-for="item in SCENARIOS"
          :key="item.id"
          type="button"
          class="cursor-pointer rounded-lg px-3 py-1.5 text-xs font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400"
          :class="scenario === item.id ? 'bg-white text-slate-900' : 'bg-white/10 text-slate-200 hover:bg-white/20'"
          :title="item.hint"
          @click="choose(item.id)"
        >{{ item.label }}</button>
      </div>
      <div class="ml-auto flex items-center gap-1.5">
        <button
          v-for="option in ['zh', 'en']"
          :key="option"
          type="button"
          class="cursor-pointer rounded-lg px-3 py-1.5 text-xs font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400"
          :class="locale === option ? 'bg-white text-slate-900' : 'bg-white/10 text-slate-200 hover:bg-white/20'"
          @click="locale = option"
        >{{ option === 'zh' ? '中文' : 'EN' }}</button>
      </div>
      <p class="w-full text-xs leading-5 text-slate-400">
        使用真实页面组件，后端为固定假数据。点「虚拟设备开启」会跑检查并按上面选择的场景返回结果。
      </p>
    </header>
    <div class="min-h-0 flex-1">
      <VideoDeviceSimulatorPage :key="pageKey" />
    </div>
  </div>
</template>
