<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';

const model = defineModel<string>({ required: true });
const emit = defineEmits<{ change: [] }>();

const { t } = useI18n();
const recording = ref(false);
const display = computed(() =>
  recording.value ? t('clipboard.settings.hotkeyRecording') : model.value,
);

function start() {
  recording.value = true;
}

function cancel() {
  recording.value = false;
}

function onKeyDown(e: KeyboardEvent) {
  if (!recording.value) return;
  e.preventDefault();
  e.stopPropagation();

  if (e.key === 'Escape') {
    recording.value = false;
    return;
  }

  const parts: string[] = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.altKey) parts.push('Alt');
  if (e.shiftKey) parts.push('Shift');
  if (e.metaKey) parts.push('Super');

  // Only accept a completed combination when the non-modifier key is a printable char or F-key.
  const k = e.key;
  const isFn = /^F([1-9]|1[0-2])$/.test(k);
  const isAlnum = k.length === 1;
  if (isAlnum || isFn) {
    parts.push(isFn ? k : k.toUpperCase());
    if (parts.length >= 2) {
      model.value = parts.join('+');
      recording.value = false;
      emit('change');
    }
  }
}
</script>

<template>
  <input
    readonly
    :value="display"
    :placeholder="t('clipboard.settings.hotkeyPlaceholder')"
    class="w-40 rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-sm font-mono outline-none transition-colors focus:border-slate-500"
    :class="recording ? 'border-sky-400 ring-2 ring-sky-200' : ''"
    @click="start"
    @blur="cancel"
    @keydown="onKeyDown"
  />
</template>
