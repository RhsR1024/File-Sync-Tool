<script setup lang="ts">
import {
  DatabaseBackup,
  Download,
  Eye,
  EyeOff,
  Import,
  RotateCcw,
  Settings2,
  StickyNote,
} from 'lucide-vue-next';
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
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

async function importData(): Promise<void> {
  if (!window.confirm(t('paperTodo.confirmImport'))) return;
  busy.value = true;
  try {
    const imported = await importPaperTodoData();
    if (imported) {
      store.state.value = imported;
      status.value = t('paperTodo.imported');
    }
  } catch (reason) {
    store.error.value = String(reason);
  } finally {
    busy.value = false;
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
  <div class="flex min-h-0 flex-1 flex-col overflow-hidden bg-slate-50 text-slate-900">
    <header class="shrink-0 border-b border-slate-200 bg-white px-6 py-5">
      <div class="mx-auto flex w-full max-w-5xl flex-wrap items-center gap-4">
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

    <main class="min-h-0 flex-1 overflow-y-auto px-6">
      <div v-if="store.loading.value" class="flex h-56 items-center justify-center text-sm text-slate-500">{{ t('common.loading') }}</div>
      <div v-else class="mx-auto w-full max-w-5xl">
        <section class="flex flex-wrap items-center gap-4 border-b border-slate-200 py-5">
          <div class="flex min-w-0 flex-1 items-center gap-3">
            <StickyNote class="h-5 w-5 shrink-0 text-amber-600" />
            <div>
              <h2 class="text-sm font-semibold text-slate-900">{{ t('paperTodo.desktopStatus') }}</h2>
              <p class="mt-0.5 text-xs text-slate-500">
                {{ t('paperTodo.desktopStatusSummary', { todos: todoCount, notes: noteCount, open: openCount }) }}
              </p>
            </div>
          </div>
          <div class="flex items-center gap-2">
            <button type="button" class="settings-command" :title="t('paperTodo.showAll')" @click="changeAllWindows('show')">
              <Eye class="h-4 w-4" />{{ t('paperTodo.showAllShort') }}
            </button>
            <button type="button" class="settings-command" :title="t('paperTodo.hideAll')" @click="changeAllWindows('hide')">
              <EyeOff class="h-4 w-4" />{{ t('paperTodo.hideAllShort') }}
            </button>
          </div>
        </section>

        <PaperTodoSettingsPanel />

        <section class="border-t border-slate-200 py-6">
          <div class="flex flex-wrap items-center gap-4">
            <div class="flex min-w-0 flex-1 items-center gap-3">
              <DatabaseBackup class="h-5 w-5 shrink-0 text-slate-500" />
              <div>
                <h2 class="text-sm font-semibold text-slate-900">{{ t('paperTodo.dataManagement') }}</h2>
                <p class="mt-0.5 text-xs text-slate-500">{{ t('paperTodo.dataManagementHint') }}</p>
              </div>
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <button type="button" class="settings-command" :disabled="busy" @click="importData"><Import class="h-4 w-4" />{{ t('paperTodo.importData') }}</button>
              <button type="button" class="settings-command" :disabled="busy" @click="exportData"><Download class="h-4 w-4" />{{ t('paperTodo.exportData') }}</button>
              <button type="button" class="settings-command" :disabled="busy" @click="cleanAssets"><RotateCcw class="h-4 w-4" />{{ t('paperTodo.cleanAssets') }}</button>
            </div>
          </div>
        </section>
      </div>
    </main>
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
