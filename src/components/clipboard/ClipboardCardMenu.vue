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

function focusMenuItem(direction: 1 | -1 = 1) {
  const buttons = menuRef.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)');
  if (!buttons || buttons.length === 0) return;
  const active = document.activeElement as HTMLButtonElement | null;
  const currentIndex = active ? Array.from(buttons).indexOf(active) : -1;
  const nextIndex = currentIndex === -1
    ? (direction > 0 ? 0 : buttons.length - 1)
    : (currentIndex + direction + buttons.length) % buttons.length;
  buttons[nextIndex]?.focus();
}

function onPointerDown(event: MouseEvent) {
  if (!props.open) return;
  const target = event.target as Node | null;
  if (target && menuRef.value?.contains(target)) return;
  emit('close');
}

function onWindowKeydown(event: KeyboardEvent) {
  if (!props.open) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    emit('close');
  } else if (event.key === 'ArrowDown') {
    event.preventDefault();
    focusMenuItem(1);
  } else if (event.key === 'ArrowUp') {
    event.preventDefault();
    focusMenuItem(-1);
  } else if (event.key === 'Home') {
    event.preventDefault();
    const buttons = menuRef.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)');
    buttons?.[0]?.focus();
  } else if (event.key === 'End') {
    event.preventDefault();
    const buttons = menuRef.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)');
    buttons?.[buttons.length - 1]?.focus();
  }
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
    await nextTick();
    focusMenuItem(1);
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
        role="menu"
        :aria-label="t('clipboard.actions.moreActions')"
      >
        <button
          v-for="item in props.items"
          :key="item.id"
          type="button"
          role="menuitem"
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
