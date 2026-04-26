<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  open: boolean;
  modelValue: string;
  selectedCount: number;
  pending?: boolean;
}>();

const emit = defineEmits<{
  close: [];
  confirm: [];
  'update:modelValue': [value: string];
}>();

const { t } = useI18n();
const dialogRef = ref<HTMLElement | null>(null);
const dialogTitleId = computed(() => 'clipboard-merge-dialog-title');

function applyPreset(value: string) {
  emit('update:modelValue', value);
}

function onInput(event: Event) {
  emit('update:modelValue', (event.target as HTMLInputElement).value);
}

function focusFirstElement() {
  const first = dialogRef.value?.querySelector<HTMLElement>('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])');
  first?.focus();
}

function onWindowKeydown(event: KeyboardEvent) {
  if (!props.open) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    emit('close');
    return;
  }
  if (event.key !== 'Tab' || !dialogRef.value) return;

  const focusable = Array.from(
    dialogRef.value.querySelectorAll<HTMLElement>('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'),
  ).filter((node) => !node.hasAttribute('disabled'));
  if (focusable.length === 0) return;

  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  const active = document.activeElement as HTMLElement | null;
  if (event.shiftKey && active === first) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && active === last) {
    event.preventDefault();
    first.focus();
  }
}

watch(
  () => props.open,
  async (open) => {
    window.removeEventListener('keydown', onWindowKeydown);
    if (!open) return;
    await nextTick();
    focusFirstElement();
    window.addEventListener('keydown', onWindowKeydown);
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onWindowKeydown);
});
</script>

<template>
  <div
    v-if="props.open"
    class="fixed inset-0 z-[75] flex items-center justify-center bg-slate-950/30 px-4"
    @click.self="emit('close')"
  >
    <div
      ref="dialogRef"
      class="w-full max-w-md rounded-2xl bg-white p-5 shadow-2xl"
      role="dialog"
      aria-modal="true"
      :aria-labelledby="dialogTitleId"
    >
      <div class="flex items-start justify-between gap-4">
        <div>
          <h3 :id="dialogTitleId" class="text-base font-semibold text-slate-900">
            {{ t('clipboard.merge.title') }}
          </h3>
          <p class="mt-1 text-sm text-slate-500">
            {{ t('clipboard.merge.subtitle', { n: props.selectedCount }) }}
          </p>
        </div>
        <button
          type="button"
          class="rounded-lg px-2 py-1 text-sm text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-700"
          :aria-label="t('clipboard.actions.close')"
          @click="emit('close')"
        >
          {{ t('clipboard.actions.close') }}
        </button>
      </div>

      <div class="mt-4 space-y-3">
        <label class="block">
          <span class="mb-1.5 block text-sm font-medium text-slate-700">
            {{ t('clipboard.merge.separatorLabel') }}
          </span>
          <input
            :value="props.modelValue"
            type="text"
            class="w-full rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-sm text-slate-800 outline-none focus:border-slate-400 focus:bg-white"
            :placeholder="t('clipboard.merge.separatorPlaceholder')"
            @input="onInput"
            @keydown.enter.prevent="emit('confirm')"
          >
        </label>

        <div class="flex flex-wrap gap-2">
          <button
            type="button"
            class="rounded-full border border-slate-200 px-3 py-1 text-xs text-slate-600 transition-colors hover:bg-slate-100"
            @click="applyPreset('\\n')"
          >
            {{ t('clipboard.merge.presets.newline') }}
          </button>
          <button
            type="button"
            class="rounded-full border border-slate-200 px-3 py-1 text-xs text-slate-600 transition-colors hover:bg-slate-100"
            @click="applyPreset('\\n\\n')"
          >
            {{ t('clipboard.merge.presets.blankLine') }}
          </button>
          <button
            type="button"
            class="rounded-full border border-slate-200 px-3 py-1 text-xs text-slate-600 transition-colors hover:bg-slate-100"
            @click="applyPreset(', ')"
          >
            {{ t('clipboard.merge.presets.commaSpace') }}
          </button>
        </div>

        <p class="text-xs text-slate-500">
          {{ t('clipboard.merge.escapeHint') }}
        </p>
      </div>

      <div class="mt-5 flex justify-end gap-2">
        <button
          type="button"
          class="rounded-lg border border-slate-200 px-3 py-1.5 text-sm text-slate-600 transition-colors hover:bg-slate-100"
          @click="emit('close')"
        >
          {{ t('clipboard.confirm.cancel') }}
        </button>
        <button
          type="button"
          class="rounded-lg bg-slate-900 px-3 py-1.5 text-sm text-white transition-colors hover:bg-slate-700 disabled:cursor-not-allowed disabled:bg-slate-300"
          :disabled="props.pending"
          @click="emit('confirm')"
        >
          {{ props.pending ? t('clipboard.merge.pending') : t('clipboard.merge.confirm') }}
        </button>
      </div>
    </div>
  </div>
</template>
