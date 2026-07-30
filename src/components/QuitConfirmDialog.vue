<script setup lang="ts">
import { AlertTriangle, LoaderCircle, LogOut, Wrench, X } from 'lucide-vue-next';
import { nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  open: boolean;
  taskNames: string[];
  simulatorCleanupRequired: boolean;
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
  'a[href]',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

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

function cancel() {
  if (!props.busy) emit('cancel');
}

watch(
  () => props.open,
  (open, previous) => {
    if (open && !previous) {
      previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      // Cancel takes focus, not exit: Enter on a reflexive keypress must not
      // abandon a copy or start privileged simulator cleanup.
      void nextTick(() => cancelButton.value?.focus());
      return;
    }
    if (!open && previous) {
      void nextTick(() => previouslyFocused?.focus());
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
      class="fixed inset-0 z-[210] flex items-center justify-center bg-slate-950/50 p-4"
      role="presentation"
      @keydown.esc.stop.prevent="cancel"
    >
      <section
        ref="dialog"
        class="w-full max-w-md overflow-hidden rounded-lg border border-slate-200 bg-white shadow-2xl focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="quit-confirm-title"
        aria-describedby="quit-confirm-hint"
        tabindex="-1"
        @keydown.tab.stop="keepFocusInside"
      >
        <header class="flex items-start gap-3 border-b border-slate-200 px-5 py-4">
          <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-amber-50 text-amber-700" aria-hidden="true">
            <AlertTriangle class="h-5 w-5" />
          </span>
          <div class="min-w-0 flex-1">
            <h2 id="quit-confirm-title" class="text-base font-semibold text-slate-900">
              {{ simulatorCleanupRequired
                ? t('deviceSimulator.exit.confirmTitle')
                : t('common.quitWhileCopyingTitle') }}
            </h2>
            <p id="quit-confirm-hint" class="mt-1 text-sm leading-6 text-slate-600">
              {{ simulatorCleanupRequired
                ? t(taskNames.length
                  ? 'deviceSimulator.exit.confirmWithCopyHint'
                  : 'deviceSimulator.exit.confirmHint')
                : t('common.quitWhileCopyingConfirm') }}
            </p>
          </div>
          <button
            type="button"
            class="flex h-11 w-11 shrink-0 cursor-pointer items-center justify-center rounded-lg text-slate-500 transition-colors hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="busy"
            :aria-label="t('common.cancel')"
            @click="cancel"
          >
            <X class="h-5 w-5" aria-hidden="true" />
          </button>
        </header>

        <div class="px-5 py-4">
          <div
            v-if="simulatorCleanupRequired"
            class="flex items-start gap-3 rounded-lg border border-sky-200 bg-sky-50 px-3 py-3 text-sm leading-6 text-sky-900"
          >
            <Wrench class="mt-0.5 h-4 w-4 shrink-0 text-sky-700" aria-hidden="true" />
            <p>{{ t('deviceSimulator.exit.cleanupDetail') }}</p>
          </div>
          <p v-if="taskNames.length" class="text-sm leading-6 text-slate-600" :class="simulatorCleanupRequired ? 'mt-3' : ''">
            {{ t('common.quitWhileCopyingDetail') }}
          </p>
          <ul v-if="taskNames.length" class="mt-3 space-y-1.5">
            <li
              v-for="name in taskNames"
              :key="name"
              class="flex min-w-0 items-center gap-2 rounded-md bg-slate-50 px-3 py-2 text-sm text-slate-700"
            >
              <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-blue-500" aria-hidden="true" />
              <span class="min-w-0 break-all">{{ name }}</span>
            </li>
          </ul>
          <div
            v-if="error"
            class="mt-3 rounded-lg border border-red-200 bg-red-50 px-3 py-2.5 text-sm leading-5 text-red-700"
            role="alert"
          >
            <p class="font-semibold">{{ t('deviceSimulator.exit.cleanupFailed') }}</p>
            <p class="mt-1 break-words">{{ error }}</p>
          </div>
        </div>

        <footer class="flex flex-col-reverse gap-2 border-t border-slate-200 bg-slate-50 px-5 py-4 sm:flex-row sm:justify-end">
          <button
            ref="cancelButton"
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-300 bg-white px-4 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="busy"
            @click="cancel"
          >
            {{ t('deviceSimulator.exit.stayOpen') }}
          </button>
          <button
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-lg border border-red-600 bg-red-600 px-4 text-sm font-semibold text-white transition-colors hover:bg-red-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500/60 disabled:cursor-not-allowed disabled:opacity-60"
            :disabled="busy"
            @click="emit('confirm')"
          >
            <LoaderCircle v-if="busy" class="h-4 w-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
            <LogOut v-else class="h-4 w-4" aria-hidden="true" />
            {{ busy
              ? t('deviceSimulator.exit.cleaning')
              : error
                ? t('deviceSimulator.exit.retryCleanupAndExit')
                : simulatorCleanupRequired
                  ? t('deviceSimulator.exit.cleanupAndExit')
                  : t('common.quitWhileCopyingExit') }}
          </button>
        </footer>
      </section>
    </div>
  </Teleport>
</template>

<style scoped>
@media (prefers-reduced-motion: reduce) {
  button {
    transition: none;
  }
}
</style>
