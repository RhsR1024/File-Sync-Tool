<script setup lang="ts">
import { FolderOpen, X } from 'lucide-vue-next';
import { nextTick, ref, watch } from 'vue';

const props = defineProps<{
  open: boolean;
  title: string;
  hint?: string;
  closeLabel: string;
  wide?: boolean;
}>();

const emit = defineEmits<{
  close: [];
}>();

const dialog = ref<HTMLElement | null>(null);
let previouslyFocused: HTMLElement | null = null;

function close() {
  emit('close');
}

function keepFocusInside(event: KeyboardEvent) {
  const root = dialog.value;
  if (!root) return;
  const focusable = Array.from(
    root.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ),
  );
  if (focusable.length === 0) {
    event.preventDefault();
    root.focus();
    return;
  }
  const first = focusable[0];
  const last = focusable.at(-1);
  if (event.shiftKey && (document.activeElement === first || document.activeElement === root)) {
    event.preventDefault();
    last?.focus();
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      void nextTick(() => dialog.value?.focus());
      return;
    }
    void nextTick(() => previouslyFocused?.focus());
  },
);
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/45 p-4"
      role="presentation"
      @keydown.esc.stop="close"
      @mousedown.self="close"
    >
      <section
        ref="dialog"
        class="flex max-h-[min(760px,calc(100vh-2rem))] w-full flex-col overflow-hidden rounded-xl bg-white shadow-2xl focus:outline-none"
        :class="wide ? 'max-w-5xl' : 'max-w-3xl'"
        role="dialog"
        aria-modal="true"
        tabindex="-1"
        :aria-label="title"
        @keydown.tab="keepFocusInside"
      >
        <header class="flex items-center gap-3 border-b border-slate-200 px-5 py-4">
          <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-blue-50 text-blue-600">
            <slot name="icon">
              <FolderOpen class="h-5 w-5" />
            </slot>
          </div>
          <div class="min-w-0 flex-1">
            <h3 class="text-sm font-semibold text-slate-900">{{ title }}</h3>
            <p v-if="hint" class="mt-0.5 text-xs text-slate-500">{{ hint }}</p>
          </div>
          <button
            type="button"
            class="flex h-9 w-9 cursor-pointer items-center justify-center rounded-lg text-slate-500 transition-colors hover:bg-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50"
            :aria-label="closeLabel"
            @click="close"
          >
            <X class="h-5 w-5" />
          </button>
        </header>
        <slot />
      </section>
    </div>
  </Teleport>
</template>
