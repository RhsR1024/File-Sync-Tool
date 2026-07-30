<script setup lang="ts">
import {
  Settings2,
} from 'lucide-vue-next';
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import PaperTodoSettingsPanel from '@/components/paper-todo/PaperTodoSettingsPanel.vue';
import { usePaperTodo } from '@/composables/usePaperTodo';
import {
  cleanPaperTodoAssets,
  exportPaperTodoData,
  importPaperTodoData,
  setAllPaperWindows,
} from '@/lib/paperTodo';

defineOptions({ name: 'PaperTodoPage' });

const { t } = useI18n();
const store = usePaperTodo();
const status = ref('');
const busy = ref(false);
const importConfirmationOpen = ref(false);
const importDialog = ref<HTMLElement | null>(null);
const importError = ref('');
const todoCount = computed(() => store.papers.value.filter((paper) => paper.kind === 'todo').length);
const noteCount = computed(() => store.papers.value.filter((paper) => paper.kind === 'note').length);
const openCount = computed(() => store.papers.value.filter((paper) => paper.desktopOpen).length);

async function changeAllWindows(action: 'show' | 'hide'): Promise<void> {
  try {
    await setAllPaperWindows(action);
    status.value = t(action === 'show' ? 'paperTodo.allShown' : 'paperTodo.allHidden');
  } catch (reason) {
    store.error.value = String(reason);
  }
}

async function exportData(): Promise<void> {
  busy.value = true;
  try {
    const path = await exportPaperTodoData();
    if (path) status.value = t('paperTodo.exported');
  } catch (reason) {
    store.error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

async function requestImport(): Promise<void> {
  importError.value = '';
  importConfirmationOpen.value = true;
  await nextTick();
  importDialog.value?.focus();
}

async function importData(): Promise<void> {
  busy.value = true;
  try {
    const imported = await importPaperTodoData();
    if (imported) {
      store.state.value = imported;
      status.value = t('paperTodo.imported');
      importConfirmationOpen.value = false;
    }
  } catch (reason) {
    importError.value = String(reason);
    await nextTick();
    importDialog.value?.focus();
  } finally {
    busy.value = false;
  }
}

function onImportDialogKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape' && !busy.value) {
    event.preventDefault();
    importConfirmationOpen.value = false;
    return;
  }
  if (event.key !== 'Tab') return;
  const buttons = [...(importDialog.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
  if (!buttons.length) return;
  const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
  if ((!event.shiftKey && index === buttons.length - 1) || (event.shiftKey && index <= 0)) {
    event.preventDefault();
    buttons[event.shiftKey ? buttons.length - 1 : 0].focus();
  }
}

async function cleanAssets(): Promise<void> {
  busy.value = true;
  try {
    const count = await cleanPaperTodoAssets();
    status.value = t('paperTodo.assetsCleaned', { count });
  } catch (reason) {
    store.error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

onMounted(() => void store.initialize());
onBeforeUnmount(() => void store.flush());
</script>

<template>
  <div class="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-slate-50 text-slate-900">
    <header class="shrink-0 border-b border-slate-200 bg-white px-6 py-5">
      <div class="mx-auto flex w-full max-w-[1480px] flex-wrap items-center gap-4">
        <div class="flex min-w-0 flex-1 items-center gap-3">
          <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-md bg-slate-900 text-amber-300">
            <Settings2 class="h-5 w-5" />
          </div>
          <div class="min-w-0">
            <h1 class="truncate text-lg font-semibold">{{ t('paperTodo.settingsTitle') }}</h1>
            <p class="mt-0.5 text-xs text-slate-500">{{ t('paperTodo.settingsDescription') }}</p>
          </div>
        </div>
        <span class="text-xs text-slate-500" aria-live="polite">{{ status }}</span>
      </div>
    </header>

    <div v-if="store.error.value" class="shrink-0 border-b border-rose-200 bg-rose-50 px-6 py-2 text-sm text-rose-700" role="alert">
      {{ store.error.value }}
      <button type="button" class="ml-2 font-semibold underline" @click="store.error.value = ''">{{ t('common.close') }}</button>
    </div>

    <main class="min-h-0 flex-1 overflow-y-auto px-5 py-5">
      <div v-if="store.loading.value" class="flex h-56 items-center justify-center text-sm text-slate-500">{{ t('common.loading') }}</div>
      <div v-else class="mx-auto w-full max-w-[1480px]">
        <PaperTodoSettingsPanel
          :todo-count="todoCount"
          :note-count="noteCount"
          :open-count="openCount"
          :busy="busy"
          @show-all="changeAllWindows('show')"
          @hide-all="changeAllWindows('hide')"
          @import-data="requestImport"
          @export-data="exportData"
          @clean-assets="cleanAssets"
        />
      </div>
    </main>

    <div
      v-if="importConfirmationOpen"
      ref="importDialog"
      class="absolute inset-0 z-50 flex items-center justify-center bg-slate-950/35 p-5 backdrop-blur-[1px]"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      :aria-labelledby="'paper-todo-import-dialog-title'"
      @keydown="onImportDialogKeydown"
    >
      <div class="w-full max-w-sm rounded-xl border border-slate-200 bg-white p-5 shadow-2xl">
        <h2 id="paper-todo-import-dialog-title" class="text-base font-semibold text-slate-900">{{ t('paperTodo.importConfirmTitle') }}</h2>
        <p class="mt-2 text-sm leading-6 text-slate-600">{{ t('paperTodo.confirmImport') }}</p>
        <p v-if="importError" class="mt-3 rounded-lg bg-rose-50 px-3 py-2 text-sm text-rose-700" role="alert">{{ importError }}</p>
        <div class="mt-5 flex justify-end gap-2">
          <button type="button" class="settings-command" :disabled="busy" autofocus @click="importConfirmationOpen = false">{{ t('common.cancel') }}</button>
          <button type="button" class="settings-command border-amber-300 text-amber-800 hover:bg-amber-50" :disabled="busy" @click="importData">{{ busy ? t('common.loading') : t('paperTodo.importData') }}</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-command {
  display: inline-flex;
  min-height: 2.35rem;
  cursor: pointer;
  align-items: center;
  gap: 0.4rem;
  border: 1px solid rgb(203 213 225);
  border-radius: 6px;
  background: white;
  padding: 0 0.75rem;
  color: rgb(51 65 85);
  font-size: 0.78rem;
  font-weight: 600;
  transition: background-color 160ms ease, border-color 160ms ease, color 160ms ease;
}
.settings-command:hover:not(:disabled) { border-color: rgb(148 163 184); background: rgb(248 250 252); color: rgb(15 23 42); }
.settings-command:focus-visible { outline: 2px solid rgb(14 165 233 / 0.7); outline-offset: 2px; }
.settings-command:disabled { cursor: default; opacity: 0.45; }
@media (prefers-reduced-motion: reduce) { .settings-command { transition: none; } }
</style>
