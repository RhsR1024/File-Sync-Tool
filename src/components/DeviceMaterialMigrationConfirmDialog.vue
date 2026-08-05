<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window';
import { AlertTriangle, ArrowRight, FolderInput, LoaderCircle, Move, X } from 'lucide-vue-next';
import { nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  open: boolean;
  busy: boolean;
  error: string;
  sourcePath: string;
  targetPath: string;
}>();
const emit = defineEmits<{ cancel: []; switchOnly: []; migrate: [] }>();
const { t } = useI18n();
const dialog = ref<HTMLElement | null>(null);
const cancelButton = ref<HTMLButtonElement | null>(null);
let previouslyFocused: HTMLElement | null = null;

const FOCUSABLE_SELECTOR = 'button:not([disabled]),[href],[tabindex]:not([tabindex="-1"])';

function cancel() {
  if (!props.busy) emit('cancel');
}

function keepFocusInside(event: KeyboardEvent) {
  const root = dialog.value;
  if (!root) return;
  const focusable = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
  if (focusable.length === 0) {
    event.preventDefault();
    root.focus();
    return;
  }
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (event.shiftKey && (document.activeElement === first || document.activeElement === root)) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

watch(() => props.open, async (open, previous) => {
  if (open && !previous) {
    previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    try {
      const window = getCurrentWindow();
      await window.show();
      await window.unminimize();
      await window.setFocus();
    } catch {
      // Web preview remains usable without a Tauri window.
    }
    await nextTick();
    cancelButton.value?.focus();
  } else if (!open && previous) {
    await nextTick();
    previouslyFocused?.focus();
    previouslyFocused = null;
  }
}, { immediate: true });

onBeforeUnmount(() => previouslyFocused?.focus());
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="fixed inset-0 z-[220] flex items-center justify-center bg-slate-950/50 p-4" role="presentation" @keydown.esc.stop.prevent="cancel">
      <section ref="dialog" class="w-full max-w-xl overflow-hidden rounded-xl border border-slate-200 bg-white shadow-2xl focus:outline-none focus-visible:ring-2 focus-visible:ring-amber-500/60" role="alertdialog" aria-modal="true" aria-labelledby="material-migration-title" aria-describedby="material-migration-description" tabindex="-1" @keydown.tab.stop="keepFocusInside">
        <header class="flex items-start gap-3 border-b border-slate-200 px-5 py-4">
          <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-amber-50 text-amber-700" aria-hidden="true"><FolderInput class="h-5 w-5" /></span>
          <div class="min-w-0 flex-1">
            <h2 id="material-migration-title" class="text-base font-semibold text-slate-900">{{ t('deviceSimulator.materialMigration.title') }}</h2>
            <p id="material-migration-description" class="mt-1 text-sm leading-6 text-slate-600">{{ t('deviceSimulator.materialMigration.description') }}</p>
          </div>
          <button type="button" class="flex h-11 w-11 shrink-0 cursor-pointer items-center justify-center rounded-lg text-slate-500 transition-colors duration-200 hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/60 disabled:cursor-not-allowed disabled:opacity-50 motion-reduce:transition-none" :disabled="busy" :aria-label="t('common.cancel')" @click="cancel"><X class="h-5 w-5" aria-hidden="true" /></button>
        </header>
        <div class="space-y-3 px-5 py-4">
          <div class="grid gap-2 rounded-lg border border-slate-200 bg-slate-50 p-3 sm:grid-cols-[minmax(0,1fr)_24px_minmax(0,1fr)] sm:items-center">
            <div class="min-w-0"><p class="text-xs font-semibold text-slate-500">{{ t('deviceSimulator.materialMigration.source') }}</p><code class="mt-1 block break-all text-xs leading-5 text-slate-800">{{ sourcePath }}</code></div>
            <ArrowRight class="hidden h-4 w-4 text-slate-400 sm:block" aria-hidden="true" />
            <div class="min-w-0"><p class="text-xs font-semibold text-slate-500">{{ t('deviceSimulator.materialMigration.target') }}</p><code class="mt-1 block break-all text-xs leading-5 text-slate-800">{{ targetPath }}</code></div>
          </div>
          <p class="flex items-start gap-2 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2.5 text-sm leading-6 text-amber-900"><AlertTriangle class="mt-1 h-4 w-4 shrink-0" aria-hidden="true" />{{ t('deviceSimulator.materialMigration.safety') }}</p>
          <p v-if="busy" class="flex items-center gap-2 rounded-lg border border-sky-200 bg-sky-50 px-3 py-2.5 text-sm leading-6 text-sky-900" aria-live="polite"><LoaderCircle class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />{{ t('deviceSimulator.materialMigration.migratingHint') }}</p>
          <p v-if="error" class="whitespace-pre-wrap break-all rounded-lg border border-rose-200 bg-rose-50 px-3 py-2.5 text-sm leading-6 text-rose-800" role="alert">{{ t('deviceSimulator.materialMigration.failed') }}<br>{{ error }}</p>
        </div>
        <footer class="flex flex-col-reverse gap-2 border-t border-slate-200 bg-slate-50 px-5 py-4 sm:flex-row sm:justify-end">
          <button ref="cancelButton" type="button" class="inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-slate-300 bg-white px-4 text-sm font-semibold text-slate-700 hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/60 disabled:cursor-not-allowed disabled:opacity-50" :disabled="busy" @click="cancel">{{ t('common.cancel') }}</button>
          <button type="button" class="inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-sky-300 bg-white px-4 text-sm font-semibold text-sky-800 hover:bg-sky-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/60 disabled:cursor-not-allowed disabled:opacity-50" :disabled="busy" @click="emit('switchOnly')">{{ t('deviceSimulator.materialMigration.switchOnly') }}</button>
          <button type="button" class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-lg bg-amber-700 px-4 text-sm font-semibold text-white hover:bg-amber-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500/60 disabled:cursor-not-allowed disabled:opacity-60" :disabled="busy" @click="emit('migrate')">
            <LoaderCircle v-if="busy" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" /><Move v-else class="h-4 w-4" aria-hidden="true" />
            {{ t(busy ? 'deviceSimulator.materialMigration.migrating' : 'deviceSimulator.materialMigration.migrateAndClear') }}
          </button>
        </footer>
      </section>
    </div>
  </Teleport>
</template>
