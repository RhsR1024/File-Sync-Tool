<script setup lang="ts">
import { LoaderCircle, MousePointer2, ShieldCheck, ShieldX, X } from 'lucide-vue-next';
import { nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import type { ScreenShareControlRequest } from '@/lib/tauri';

const props = defineProps<{
  request: ScreenShareControlRequest | null;
  busy: boolean;
  error: string;
}>();

const emit = defineEmits<{
  allow: [];
  deny: [];
}>();

const { t } = useI18n();
const dialog = ref<HTMLElement | null>(null);
const denyButton = ref<HTMLButtonElement | null>(null);
let previouslyFocused: HTMLElement | null = null;

const FOCUSABLE_SELECTOR = [
  'button:not([disabled])',
  'a[href]',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

function deny() {
  if (!props.busy) emit('deny');
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

watch(
  () => props.request,
  (request, previous) => {
    if (request && !previous) {
      previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      void nextTick(() => denyButton.value?.focus());
      return;
    }
    if (!request && previous) {
      void nextTick(() => previouslyFocused?.focus());
      previouslyFocused = null;
    }
  },
  // The parent mounts this dialog only once a request exists, so the arriving
  // request is the initial value rather than a transition. Without `immediate`
  // the deny button would never take focus.
  { immediate: true },
);

onBeforeUnmount(() => previouslyFocused?.focus());
</script>

<template>
  <Teleport to="body">
    <div
      v-if="request"
      class="fixed inset-0 z-[200] flex items-center justify-center bg-slate-950/50 p-4"
      role="presentation"
      @keydown.esc.stop.prevent="deny"
    >
      <section
        ref="dialog"
        class="w-full max-w-md overflow-hidden rounded-lg border border-slate-200 bg-white shadow-2xl focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60"
        role="dialog"
        aria-modal="true"
        aria-labelledby="screen-share-control-request-title"
        aria-describedby="screen-share-control-request-hint"
        tabindex="-1"
        @keydown.tab.stop="keepFocusInside"
      >
        <header class="flex items-start gap-3 border-b border-slate-200 px-5 py-4">
          <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-amber-50 text-amber-700" aria-hidden="true">
            <MousePointer2 class="h-5 w-5" />
          </span>
          <div class="min-w-0 flex-1">
            <h2 id="screen-share-control-request-title" class="text-base font-semibold text-slate-900">
              {{ t('tools.screenShare.controlRequestTitle') }}
            </h2>
            <p id="screen-share-control-request-hint" class="mt-1 text-sm leading-6 text-slate-600">
              {{ t('tools.screenShare.controlRequestHint') }}
            </p>
          </div>
          <button
            ref="denyButton"
            type="button"
            class="flex h-11 w-11 shrink-0 cursor-pointer items-center justify-center rounded-lg text-slate-500 transition-colors hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="busy"
            :aria-label="t('tools.screenShare.denyControl')"
            @click="deny"
          >
            <X class="h-5 w-5" aria-hidden="true" />
          </button>
        </header>

        <div class="px-5 py-4">
          <div class="flex min-w-0 items-center gap-3 text-sm">
            <span class="shrink-0 font-semibold text-slate-700">{{ request.ip }}</span>
            <span class="h-4 w-px shrink-0 bg-slate-200" aria-hidden="true" />
            <span class="min-w-0 break-words text-slate-500">{{ request.user_agent }}</span>
          </div>
          <p v-if="error" class="mt-3 text-sm leading-5 text-red-700" role="alert">
            {{ error }}
          </p>
        </div>

        <footer class="flex flex-col-reverse gap-2 border-t border-slate-200 bg-slate-50 px-5 py-4 sm:flex-row sm:justify-end">
          <button
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-300 bg-white px-4 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500/50 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="busy"
            @click="deny"
          >
            <ShieldX class="h-4 w-4 text-red-600" aria-hidden="true" />
            {{ t('tools.screenShare.denyControl') }}
          </button>
          <button
            type="button"
            class="inline-flex min-h-11 cursor-pointer items-center justify-center gap-2 rounded-lg border border-emerald-700 bg-emerald-700 px-4 text-sm font-semibold text-white transition-colors hover:bg-emerald-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/60 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="busy"
            @click="emit('allow')"
          >
            <LoaderCircle v-if="busy" class="h-4 w-4 animate-spin" aria-hidden="true" />
            <ShieldCheck v-else class="h-4 w-4" aria-hidden="true" />
            {{ busy ? t('tools.screenShare.respondingControlRequest') : t('tools.screenShare.allowControl') }}
          </button>
        </footer>
      </section>
    </div>
  </Teleport>
</template>

<style scoped>
@media (prefers-reduced-motion: reduce) {
  button,
  .animate-spin {
    transition: none;
    animation: none;
  }
}
</style>
