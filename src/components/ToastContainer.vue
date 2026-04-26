<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from 'vue';

import Toast from '@/components/Toast.vue';
import { dismissToast, useToast } from '@/composables/useToast';

defineOptions({ name: 'ToastContainer' });

const MAX_VISIBLE = 4;

const { toasts } = useToast();

// The newest toast renders on top; older ones fall off after the cap is hit.
const visibleToasts = computed(() => {
  const list = toasts.value;
  if (list.length <= MAX_VISIBLE) {
    return [...list].reverse();
  }
  return [...list.slice(list.length - MAX_VISIBLE)].reverse();
});

function handleDismiss(id: string) {
  dismissToast(id);
}

function onKeydown(event: KeyboardEvent) {
  if (event.key !== 'Escape') return;
  if (visibleToasts.value.length === 0) return;
  // Topmost = newest = first item in `visibleToasts` after reverse().
  const top = visibleToasts.value[0];
  dismissToast(top.id);
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown);
});

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown);
});
</script>

<template>
  <div
    class="pointer-events-none fixed bottom-6 right-6 z-[100] flex flex-col items-end gap-2"
    aria-live="polite"
  >
    <transition-group
      name="fst-toast"
      tag="div"
      class="flex flex-col items-end gap-2"
    >
      <div
        v-for="toast in visibleToasts"
        :key="toast.id"
        class="pointer-events-auto"
      >
        <Toast :toast="toast" @dismiss="handleDismiss" />
      </div>
    </transition-group>
  </div>
</template>

<style scoped>
.fst-toast-enter-from {
  opacity: 0;
  transform: translateX(24px);
}
.fst-toast-enter-active {
  transition: opacity 160ms ease-out, transform 160ms ease-out;
}
.fst-toast-enter-to {
  opacity: 1;
  transform: translateX(0);
}
.fst-toast-leave-from {
  opacity: 1;
}
.fst-toast-leave-active {
  transition: opacity 120ms ease-in;
}
.fst-toast-leave-to {
  opacity: 0;
}
@media (prefers-reduced-motion: reduce) {
  .fst-toast-enter-from,
  .fst-toast-enter-to,
  .fst-toast-leave-from,
  .fst-toast-leave-to {
    transform: none;
  }
  .fst-toast-enter-active,
  .fst-toast-leave-active {
    transition: opacity 120ms linear;
  }
}
</style>
