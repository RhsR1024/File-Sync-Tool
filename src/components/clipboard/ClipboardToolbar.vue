<script setup lang="ts">
import { CheckSquare, Lock, LockOpen, Settings } from 'lucide-vue-next';
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';

import type { ClipboardToolbarActionId } from '@/lib/clipboardSettingsUi';

const props = withDefaults(
  defineProps<{
    items: ClipboardToolbarActionId[];
    batchMode?: boolean;
    locked?: boolean;
    compact?: boolean;
  }>(),
  {
    batchMode: false,
    locked: false,
    compact: false,
  },
);

const emit = defineEmits<{
  batch: [];
  settings: [];
  lock: [];
}>();

const { t } = useI18n();

const buttonClass = computed(() =>
  props.compact
    ? 'inline-flex h-7 w-7 items-center justify-center rounded text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-800'
    : 'inline-flex h-9 w-9 items-center justify-center rounded-xl border border-slate-200 bg-white text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-800',
);

function titleFor(item: ClipboardToolbarActionId): string {
  switch (item) {
    case 'batch':
      return props.batchMode
        ? t('clipboard.actions.exitBatch')
        : t('clipboard.actions.batchSelect');
    case 'lock':
      return props.locked
        ? t('clipboard.actions.unlockWindow')
        : t('clipboard.actions.lockWindow');
    case 'settings':
    default:
      return t('clipboard.actions.openSettings');
  }
}

function emitAction(item: ClipboardToolbarActionId) {
  switch (item) {
    case 'batch':
      emit('batch');
      return;
    case 'lock':
      emit('lock');
      return;
    case 'settings':
      emit('settings');
      return;
  }
}
</script>

<template>
  <div class="flex items-center gap-1.5">
    <button
      v-for="item in props.items"
      :key="item"
      type="button"
      :class="[
        buttonClass,
        item === 'batch' && props.batchMode && 'bg-blue-50 text-blue-600 border-blue-100',
        item === 'lock' && props.locked && 'bg-amber-50 text-amber-600 border-amber-100',
      ]"
      :title="titleFor(item)"
      @click="emitAction(item)"
    >
      <CheckSquare
        v-if="item === 'batch'"
        class="h-4 w-4"
      />
      <Settings
        v-else-if="item === 'settings'"
        class="h-4 w-4"
      />
      <Lock
        v-else-if="props.locked"
        class="h-4 w-4"
      />
      <LockOpen
        v-else
        class="h-4 w-4"
      />
    </button>
  </div>
</template>
