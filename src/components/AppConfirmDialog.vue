<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { AlertTriangle, Trash2 } from 'lucide-vue-next';

const props = withDefaults(defineProps<{
  open: boolean;
  title: string;
  description: string;
  confirmLabel: string;
  cancelLabel: string;
  busy?: boolean;
  tone?: 'danger' | 'warning';
}>(), {
  busy: false,
  tone: 'danger',
});

const emit = defineEmits<{
  confirm: [];
  cancel: [];
}>();

const dialogRef = ref<HTMLElement | null>(null);
const cancelRef = ref<HTMLButtonElement | null>(null);
let previousFocus: HTMLElement | null = null;

function cancel() {
  if (!props.busy) emit('cancel');
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    cancel();
    return;
  }
  if (event.key !== 'Tab' || !dialogRef.value) return;
  const focusable = dialogRef.value.querySelectorAll<HTMLElement>('button:not([disabled])');
  if (!focusable.length) return;
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

watch(() => props.open, async open => {
  if (open) {
    previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    await nextTick();
    cancelRef.value?.focus();
  } else {
    await nextTick();
    previousFocus?.focus?.();
    previousFocus = null;
  }
});

onBeforeUnmount(() => previousFocus?.focus?.());
</script>

<template>
  <Teleport to="body">
    <Transition name="app-confirm-dialog">
      <div v-if="open" class="fixed inset-0 z-[90] flex items-center justify-center bg-slate-950/55 p-4" @click.self="cancel">
        <section
          ref="dialogRef"
          role="alertdialog"
          aria-modal="true"
          aria-labelledby="app-confirm-title"
          aria-describedby="app-confirm-description"
          class="w-full max-w-md rounded-2xl border border-slate-200 bg-white p-6 shadow-[0_24px_80px_rgba(15,23,42,0.28)]"
          @keydown="handleKeydown"
        >
          <div class="flex items-start gap-3">
            <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl" :class="tone === 'danger' ? 'bg-rose-100 text-rose-700' : 'bg-amber-100 text-amber-700'">
              <Trash2 v-if="tone === 'danger'" class="h-5 w-5" aria-hidden="true" />
              <AlertTriangle v-else class="h-5 w-5" aria-hidden="true" />
            </span>
            <div>
              <h2 id="app-confirm-title" class="text-base font-semibold text-slate-950">{{ title }}</h2>
              <p id="app-confirm-description" class="mt-1 text-sm leading-6 text-slate-600">{{ description }}</p>
            </div>
          </div>
          <div class="mt-6 flex justify-end gap-3">
            <button
              ref="cancelRef"
              type="button"
              class="min-h-11 cursor-pointer rounded-xl border border-slate-200 px-4 py-2 text-sm font-medium text-slate-700 transition-colors duration-200 hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-500/40"
              :disabled="busy"
              @click="cancel"
            >
              {{ cancelLabel }}
            </button>
            <button
              type="button"
              class="min-h-11 cursor-pointer rounded-xl px-4 py-2 text-sm font-semibold text-white transition-colors duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-60"
              :class="tone === 'danger' ? 'bg-rose-600 hover:bg-rose-700 focus-visible:ring-rose-500' : 'bg-amber-600 hover:bg-amber-700 focus-visible:ring-amber-500'"
              :disabled="busy"
              @click="emit('confirm')"
            >
              {{ confirmLabel }}
            </button>
          </div>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.app-confirm-dialog-enter-active,
.app-confirm-dialog-leave-active { transition: opacity 160ms ease; }
.app-confirm-dialog-enter-from,
.app-confirm-dialog-leave-to { opacity: 0; }
@media (prefers-reduced-motion: reduce) {
  .app-confirm-dialog-enter-active,
  .app-confirm-dialog-leave-active { transition: none; }
}
</style>
