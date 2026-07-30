<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Minus, Server, Square, X } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';

const { t } = useI18n();
const win = getCurrentWindow();

function toggleMaximizeOnDoubleClick(event: MouseEvent) {
  if ((event.target as HTMLElement).closest('button')) return;
  void win.toggleMaximize();
}
</script>

<template>
  <header
    class="flex h-8 flex-none select-none items-center gap-2 border-b border-slate-200 bg-white pl-2.5"
    data-tauri-drag-region
    @dblclick="toggleMaximizeOnDoubleClick"
  >
    <Server class="h-3.5 w-3.5 text-sky-600" aria-hidden="true" data-tauri-drag-region />
    <span class="text-xs font-semibold text-slate-700" data-tauri-drag-region>File Sync Tool</span>
    <span class="text-xs text-slate-500" data-tauri-drag-region>{{ t('sidebar.title') }}</span>
    <div class="ml-auto flex h-full items-stretch">
      <button type="button" class="flex h-8 w-11 cursor-pointer items-center justify-center text-slate-600 transition-colors hover:bg-slate-100 focus-visible:z-10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-sky-500" :aria-label="t('titlebar.minimize')" @click="win.minimize()">
        <Minus class="h-3.5 w-3.5" aria-hidden="true" />
      </button>
      <button type="button" class="flex h-8 w-11 cursor-pointer items-center justify-center text-slate-600 transition-colors hover:bg-slate-100 focus-visible:z-10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-sky-500" :aria-label="t('titlebar.maximize')" @click="win.toggleMaximize()">
        <Square class="h-3 w-3" aria-hidden="true" />
      </button>
      <button type="button" class="flex h-8 w-11 cursor-pointer items-center justify-center text-slate-600 transition-colors hover:bg-rose-600 hover:text-white focus-visible:z-10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-rose-500" :aria-label="t('titlebar.close')" @click="win.close()">
        <X class="h-3.5 w-3.5" aria-hidden="true" />
      </button>
    </div>
  </header>
</template>
