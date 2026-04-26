<script setup lang="ts">
import { X } from 'lucide-vue-next';
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

function clearHotkey() {
  model.value = '';
  recording.value = false;
  emit('change');
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
  <div class="inline-flex items-center gap-1 rounded-lg border border-slate-300 bg-white px-2 py-1.5 transition-colors focus-within:border-slate-500">
    <input
      readonly
      :value="display"
      :placeholder="t('clipboard.settings.hotkeyPlaceholder')"
      :aria-label="t('clipboard.settings.hotkeyLabel')"
      :aria-describedby="'clipboard-hotkey-hint'"
      class="w-32 bg-transparent text-sm font-mono outline-none"
      :class="recording ? 'text-sky-700' : 'text-slate-700'"
      @click="start"
      @blur="cancel"
      @keydown="onKeyDown"
    >
    <button
      v-if="model"
      type="button"
      class="inline-flex h-6 w-6 items-center justify-center rounded text-slate-400 transition-colors hover:bg-slate-100 hover:text-slate-700"
      :aria-label="t('clipboard.settings.hotkeyClear')"
      :title="t('clipboard.settings.hotkeyClear')"
      @click="clearHotkey"
    >
      <X class="h-3.5 w-3.5" />
    </button>
    <span id="clipboard-hotkey-hint" class="sr-only">
      {{ t('clipboard.settings.hotkeyInstruction') }}
    </span>
  </div>
</template>
