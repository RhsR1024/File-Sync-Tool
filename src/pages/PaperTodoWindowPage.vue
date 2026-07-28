<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window';
import { computed, nextTick, onBeforeUnmount, onMounted } from 'vue';
import { useRoute } from 'vue-router';
import { useI18n } from 'vue-i18n';

import PaperTodoPaper from '@/components/paper-todo/PaperTodoPaper.vue';
import { usePaperTodo } from '@/composables/usePaperTodo';

defineOptions({ name: 'PaperTodoWindowPage' });

const route = useRoute();
const { t } = useI18n();
const store = usePaperTodo();
const paperId = computed(() => String(route.params.id ?? ''));
const unlisteners: Array<() => void> = [];
const PAPER_WINDOW_CLASS = 'paper-todo-window';

function setPaperWindowTransparency(enabled: boolean): void {
  document.documentElement.classList.toggle(PAPER_WINDOW_CLASS, enabled);
  document.body.classList.toggle(PAPER_WINDOW_CLASS, enabled);
}

onMounted(async () => {
  setPaperWindowTransparency(true);
  try {
    await store.initialize();
    if (store.error.value) {
      await nextTick();
      await getCurrentWindow().show();
      return;
    }
    if (!store.state.value.papers.some((paper) => paper.id === paperId.value)) {
      await getCurrentWindow().close();
      return;
    }
    const currentWindow = getCurrentWindow();
    unlisteners.push(await currentWindow.onMoved(async ({ payload }) => {
      // Docking and edge-peek move the window themselves. Recording those
      // frames would persist an off-screen origin and restore the paper hidden
      // at the display edge on the next launch.
      if (store.geometryTrackingSuspended.value) return;
      const scale = await currentWindow.scaleFactor();
      store.updatePaper(paperId.value, (paper) => {
        paper.geometry.x = payload.x / scale;
        paper.geometry.y = payload.y / scale;
      });
    }));
    unlisteners.push(await currentWindow.onResized(async ({ payload }) => {
      const paper = store.state.value.papers.find((candidate) => candidate.id === paperId.value);
      if (paper?.collapsed) return;
      const scale = await currentWindow.scaleFactor();
      store.updatePaper(paperId.value, (value) => {
        value.geometry.width = payload.width / scale;
        value.geometry.height = payload.height / scale;
      });
    }));
    await currentWindow.show();
  } catch (reason) {
    store.error.value = String(reason);
    try {
      await nextTick();
      await getCurrentWindow().show();
    } catch {
      // The window may already be closing; there is nothing left to recover.
    }
  }
});

onBeforeUnmount(() => {
  unlisteners.splice(0).forEach((unlisten) => unlisten());
  setPaperWindowTransparency(false);
  void store.flush();
});
</script>

<template>
  <div class="h-screen w-screen overflow-hidden bg-transparent">
    <div v-if="store.loading.value" class="flex h-full items-center justify-center rounded-xl bg-slate-100/95 p-4 text-sm text-slate-600">
      {{ t('common.loading') }}
    </div>
    <div v-else-if="store.error.value" class="flex h-full flex-col items-center justify-center gap-3 rounded-xl bg-rose-50 p-5 text-center text-sm text-rose-700" role="alert">
      <p>{{ store.error.value }}</p>
      <button type="button" class="rounded border border-rose-300 bg-white px-3 py-1.5 font-semibold" @click="getCurrentWindow().close()">
        {{ t('common.close') }}
      </button>
    </div>
    <PaperTodoPaper v-else :paper-id="paperId" standalone @deleted="getCurrentWindow().close()" />
  </div>
</template>
