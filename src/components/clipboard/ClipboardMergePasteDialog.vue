<script setup lang="ts">
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

function applyPreset(value: string) {
  emit('update:modelValue', value);
}

function onInput(event: Event) {
  emit('update:modelValue', (event.target as HTMLInputElement).value);
}
</script>

<template>
  <div
    v-if="props.open"
    class="fixed inset-0 z-[75] flex items-center justify-center bg-slate-950/30 px-4"
    @click.self="emit('close')"
  >
    <div class="w-full max-w-md rounded-2xl bg-white p-5 shadow-2xl">
      <div class="flex items-start justify-between gap-4">
        <div>
          <h3 class="text-base font-semibold text-slate-900">
            {{ t('clipboard.merge.title') }}
          </h3>
          <p class="mt-1 text-sm text-slate-500">
            {{ t('clipboard.merge.subtitle', { n: props.selectedCount }) }}
          </p>
        </div>
        <button
          type="button"
          class="rounded-lg px-2 py-1 text-sm text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-700"
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
          />
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
