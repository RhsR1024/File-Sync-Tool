<script setup lang="ts">
import { Search, X } from 'lucide-vue-next';
import { ref } from 'vue';
import { useI18n } from 'vue-i18n';

const model = defineModel<string>({ required: true });

withDefaults(
  defineProps<{
    placeholder?: string;
    disabled?: boolean;
  }>(),
  {
    placeholder: '',
    disabled: false,
  },
);

const emit = defineEmits<{
  clear: [];
}>();

const { t } = useI18n();
const inputRef = ref<HTMLInputElement | null>(null);

function clearValue() {
  if (!model.value) return;
  model.value = '';
  emit('clear');
  inputRef.value?.focus();
}

function focus() {
  inputRef.value?.focus();
}

defineExpose({ focus });
</script>

<template>
  <div class="relative">
    <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
    <input
      ref="inputRef"
      v-model="model"
      type="search"
      :placeholder="placeholder"
      :disabled="disabled"
      class="w-full rounded-xl border border-slate-200 bg-white px-10 py-2 text-sm shadow-sm outline-none transition-colors focus:border-slate-400 disabled:cursor-not-allowed disabled:bg-slate-50"
    >
    <button
      v-if="model"
      type="button"
      class="absolute right-2 top-1/2 inline-flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded-full text-slate-400 transition-colors hover:bg-slate-100 hover:text-slate-700"
      :title="t('clipboard.search.clear')"
      @click="clearValue"
    >
      <X class="h-4 w-4" />
    </button>
  </div>
</template>
