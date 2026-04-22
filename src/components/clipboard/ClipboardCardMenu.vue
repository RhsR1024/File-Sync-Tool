<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import type {
  ClipboardContextActionId,
  ClipboardContextMenuItem,
} from '@/composables/clipboardContextMenuHelpers';

const props = defineProps<{
  open: boolean;
  x: number;
  y: number;
  items: ClipboardContextMenuItem[];
}>();

const emit = defineEmits<{
  select: [action: ClipboardContextActionId];
  close: [];
}>();

const { t } = useI18n();
const menuRef = ref<HTMLElement | null>(null);
const position = ref({ left: 0, top: 0 });

function onPointerDown(event: MouseEvent) {
  if (!props.open) return;
  const target = event.target as Node | null;
  if (target && menuRef.value?.contains(target)) return;
  emit('close');
}

function onWindowKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') emit('close');
}

async function syncPosition() {
  if (!props.open) return;
  await nextTick();

  const menu = menuRef.value;
  if (!menu) return;

  const margin = 8;
  const maxLeft = Math.max(margin, window.innerWidth - menu.offsetWidth - margin);
  const maxTop = Math.max(margin, window.innerHeight - menu.offsetHeight - margin);
  position.value = {
    left: Math.min(Math.max(props.x, margin), maxLeft),
    top: Math.min(Math.max(props.y, margin), maxTop),
  };
}

watch(
  () => [props.open, props.x, props.y, props.items.length],
  async ([open]) => {
    document.removeEventListener('mousedown', onPointerDown);
    window.removeEventListener('keydown', onWindowKeydown);

    if (!open) return;

    await syncPosition();
    document.addEventListener('mousedown', onPointerDown);
    window.addEventListener('keydown', onWindowKeydown);
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', onPointerDown);
  window.removeEventListener('keydown', onWindowKeydown);
});

const menuStyle = computed(() => ({
  left: `${position.value.left}px`,
  top: `${position.value.top}px`,
}));
</script>

<template>
  <Teleport to="body">
    <div v-if="props.open" class="fixed inset-0 z-[70]">
      <div
        ref="menuRef"
        class="fixed min-w-[220px] overflow-hidden rounded-xl border border-slate-200 bg-white p-1.5 shadow-2xl"
        :style="menuStyle"
      >
        <button
          v-for="item in props.items"
          :key="item.id"
          type="button"
          class="flex w-full items-center rounded-lg px-3 py-2 text-left text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-40"
          :class="item.destructive
            ? 'text-red-600 hover:bg-red-50'
            : 'text-slate-700 hover:bg-slate-100'"
          :disabled="item.disabled"
          @click="emit('select', item.id)"
        >
          {{ t(item.labelKey, item.labelParams) }}
        </button>
      </div>
    </div>
  </Teleport>
</template>
