<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { AlertTriangle, LoaderCircle, RotateCcw, X } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  open: boolean;
  busy: boolean;
  error: string;
}>();

const emit = defineEmits<{
  confirm: [];
  cancel: [];
}>();

const { t } = useI18n();
const dialog = ref<HTMLElement | null>(null);
const cancelButton = ref<HTMLButtonElement | null>(null);
let previouslyFocused: HTMLElement | null = null;

const FOCUSABLE_SELECTOR = [
  'button:not([disabled])',
  '[href]',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

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

async function revealMainWindow() {
  try {
    const currentWindow = getCurrentWindow();
    await currentWindow.show();
    await currentWindow.unminimize();
    await currentWindow.setFocus();
  } catch {
    // The prompt remains usable when the window API is unavailable in web previews.
  }
}

watch(
  () => props.open,
  async (open, previous) => {
    if (open && !previous) {
      previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      await revealMainWindow();
      await nextTick();
      cancelButton.value?.focus();
      return;
    }
    if (!open && previous) {
      await nextTick();
      previouslyFocused?.focus();
      previouslyFocused = null;
    }
  },
  { immediate: true },
);

onBeforeUnmount(() => previouslyFocused?.focus());
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-[220] flex items-center justify-center bg-slate-950/50 p-4"
      role="presentation"
      @keydown.esc.stop.prevent="cancel"
    >
      <section
        ref="dialog"
        class="w-full max-w-md overflow-hidden rounded-xl border border-slate-200 bg-white shadow-2xl focus:outline-none focus-visible:ring-2 focus-visible:ring-amber-500/60"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="platform-replace-confirm-title"
        aria-describedby="platform-replace-confirm-description"
        tabindex="-1"
        @keydown.tab.stop="keepFocusInside"
      >
        <header class="flex items-start gap-3 border-b border-slate-200 px-5 py-4">
          <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-amber-50 text-amber-700" aria-hidden="true">
            <AlertTriangle class="h-5 w-5" />
          </span>
          <div class="min-w-0 flex-1">
            <h2 id="platform-replace-confirm-title" class="text-base font-semibold text-slate-900">
              {{ t('deviceSimulator.platformAdd.replaceConfirmTitle') }}
            </h2>
            <p id="platform-replace-confirm-description" class="mt-1 text-sm leading-6 text-slate-600">
              {{ t('deviceSimulator.platformAdd.replaceConfirmDescription') }}
            </p>
          </div>
          <button
            type="button"
            class="flex h-11 w-11 shrink-0 cursor-pointer items-center justify-center rounded-lg text-slate-500 transition-colors hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/60 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="busy"
            :aria-label="t('common.cancel')"
            @click="cancel"
          >
            <X class="h-5 w-5" aria-hidden="true" />
          </button>
        </header>

        <div class="space-y-3 px-5 py-4">
          <p class="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2.5 text-sm leading-6 text-amber-900">
            {{ t('deviceSimulator.platformAdd.replaceConfirmWarning') }}
          </p>
          <p v-if="error" class="whitespace-pre-wrap break-all rounded-lg border border-rose-200 bg-rose-50 px-3 py-2.5 text-sm leading-6 text-rose-800" role="alert">
            {{ t('deviceSimulator.platformAdd.replaceRetryFailed') }}<br>{{ error }}
          </p>
        </div>

        <footer class="flex flex-col-reverse gap-2 border-t border-slate-200 bg-slate-50 px-5 py-4 sm:flex-row sm:justify-end">
          <button
            ref="cancelButton"
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center justify-center rounded-lg border border-slate-300 bg-white px-4 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/60 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="busy"
            @click="cancel"
          >
            {{ t('common.cancel') }}
          </button>
          <button
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-lg border border-amber-700 bg-amber-700 px-4 text-sm font-semibold text-white transition-colors hover:bg-amber-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500/60 disabled:cursor-not-allowed disabled:opacity-60"
            :disabled="busy"
            @click="emit('confirm')"
          >
            <LoaderCircle v-if="busy" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
            <RotateCcw v-else class="h-4 w-4" aria-hidden="true" />
            {{ t(busy ? 'deviceSimulator.platformAdd.replacing' : 'deviceSimulator.platformAdd.replaceAction') }}
          </button>
        </footer>
      </section>
    </div>
  </Teleport>
</template>
