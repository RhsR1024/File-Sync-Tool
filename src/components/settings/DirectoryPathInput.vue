<script setup lang="ts">
import { ref } from 'vue';
import { FolderOpen } from 'lucide-vue-next';

import { openDirectory } from '@/lib/tauri';

defineOptions({ name: 'DirectoryPathInput' });

interface Props {
  modelValue: string;
  placeholder?: string;
  title?: string;
  disabled?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  placeholder: '',
  title: '',
  disabled: false,
});

const emit = defineEmits<{
  'update:modelValue': [value: string];
  'pick-error': [error: string];
}>();

const isPicking = ref(false);

async function pickDirectory() {
  if (props.disabled || isPicking.value) {
    return;
  }

  isPicking.value = true;

  try {
    const selected = await openDirectory();
    if (selected) {
      emit('update:modelValue', selected);
    }
  } catch (error) {
    emit('pick-error', String(error));
  } finally {
    isPicking.value = false;
  }
}
</script>

<template>
  <div class="flex gap-2">
    <input
      :value="modelValue"
      type="text"
      :placeholder="placeholder"
      :disabled="disabled"
      class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all disabled:bg-slate-100 disabled:cursor-not-allowed"
      @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
    />
    <button
      type="button"
      :disabled="disabled || isPicking"
      :title="title"
      class="p-2 text-slate-400 hover:text-blue-600 hover:bg-blue-50 rounded-lg transition-colors border border-transparent hover:border-blue-100 disabled:opacity-60 disabled:cursor-not-allowed disabled:hover:text-slate-400 disabled:hover:bg-transparent disabled:hover:border-transparent"
      @click="pickDirectory"
    >
      <FolderOpen class="w-4 h-4" />
    </button>
  </div>
</template>
