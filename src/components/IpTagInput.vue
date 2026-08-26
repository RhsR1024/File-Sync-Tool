<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { isValidIp } from '../lib/applianceSshGroups';

defineOptions({
  name: 'IpTagInput',
});

const props = withDefaults(
  defineProps<{
    modelValue: string[];
    disabled?: boolean;
    placeholder?: string;
    /** Reject additions beyond this count and emit `limit-exceeded`. */
    maxTags?: number;
    /** Optional <datalist> id rendered by the parent for input suggestions. */
    datalistId?: string;
    ariaLabel?: string;
  }>(),
  {
    disabled: false,
    placeholder: '',
    maxTags: undefined,
    datalistId: undefined,
    ariaLabel: '',
  },
);

const emit = defineEmits<{
  (e: 'update:modelValue', value: string[]): void;
  (e: 'update:pending', value: string): void;
  (e: 'limit-exceeded'): void;
}>();

const { t } = useI18n();

const pending = ref('');
const inputRef = ref<HTMLInputElement | null>(null);

watch(pending, value => emit('update:pending', value));

const SEPARATORS = /[\s,，、;；\n\r]+/;

const addTags = (raw: string) => {
  const parts = raw.split(SEPARATORS).map(s => s.trim()).filter(Boolean);
  const next = [...props.modelValue];
  let exceeded = false;
  for (const tag of parts) {
    if (next.includes(tag)) continue;
    if (props.maxTags !== undefined && next.length >= props.maxTags) {
      exceeded = true;
      continue;
    }
    next.push(tag);
  }
  if (next.length !== props.modelValue.length) {
    emit('update:modelValue', next);
  }
  if (exceeded) {
    emit('limit-exceeded');
  }
};

const removeTag = (tag: string) => {
  const idx = props.modelValue.indexOf(tag);
  if (idx > -1) {
    const next = [...props.modelValue];
    next.splice(idx, 1);
    emit('update:modelValue', next);
  }
};

// Clicking a tag's text moves it back into the input so a single character can
// be edited (e.g. 192.115.2.30 → 192.115.2.130) instead of deleting it whole.
const editTag = (tag: string) => {
  if (pending.value.trim()) {
    addTags(pending.value);
  }
  removeTag(tag);
  pending.value = tag;
  nextTick(() => inputRef.value?.focus());
};

const handleKeydown = (e: KeyboardEvent) => {
  const triggerKeys = ['Enter', 'Tab', ' '];
  const raw = pending.value.trim();
  if (triggerKeys.includes(e.key)) {
    if (raw) {
      e.preventDefault();
      addTags(raw);
      pending.value = '';
    }
  } else if (e.key === 'Backspace' && !raw && props.modelValue.length > 0) {
    // Move the last tag back into the input for editing rather than deleting it.
    const next = [...props.modelValue];
    const last = next.pop();
    if (last !== undefined) {
      emit('update:modelValue', next);
      pending.value = last;
    }
  }
};

const handleInputChange = () => {
  // Confirm tag when user types a separator character inline
  const raw = pending.value;
  if (SEPARATORS.test(raw)) {
    addTags(raw);
    pending.value = '';
  }
};

const handlePaste = (e: ClipboardEvent) => {
  e.preventDefault();
  const text = e.clipboardData?.getData('text') ?? '';
  addTags(text);
  pending.value = '';
};

const handleBlur = () => {
  if (pending.value.trim()) {
    addTags(pending.value);
    pending.value = '';
  }
};

const focus = () => {
  inputRef.value?.focus();
};

// Discard any pending text, confirm the given tag, and refocus — the recent
// history chips use this to fill the input in one click.
const applyTag = (tag: string) => {
  pending.value = '';
  addTags(tag);
  nextTick(() => inputRef.value?.focus());
};

const removeTagValue = (tag: string) => {
  if (pending.value.trim() === tag) {
    pending.value = '';
  }
  removeTag(tag);
  nextTick(() => inputRef.value?.focus());
};

defineExpose({ focus, applyTag, removeTag: removeTagValue });
</script>

<template>
  <div
    role="listbox"
    :aria-label="ariaLabel"
    class="min-h-[2.375rem] w-full flex flex-wrap gap-1.5 px-2.5 py-1.5 border border-slate-200 rounded-lg transition-colors cursor-text"
    :class="disabled ? 'bg-slate-50 cursor-not-allowed' : 'bg-white focus-within:border-blue-400 focus-within:ring-2 focus-within:ring-blue-400/20'"
    @click="inputRef?.focus()"
  >
    <span
      v-for="tag in modelValue"
      :key="tag"
      class="inline-flex items-center gap-1 text-xs font-mono px-2 py-0.5 rounded-md"
      :class="isValidIp(tag)
        ? 'bg-blue-100 text-blue-800'
        : 'bg-red-100 text-red-700 border border-red-200'"
      :title="isValidIp(tag) ? undefined : t('tools.applianceSsh.invalidIp', { ip: tag })"
    >
      <button
        type="button"
        :disabled="disabled"
        class="disabled:cursor-not-allowed leading-none font-mono"
        :title="t('tools.applianceSsh.editTag')"
        @click.stop="editTag(tag)"
      >{{ tag }}</button>
      <button
        type="button"
        :disabled="disabled"
        class="disabled:cursor-not-allowed leading-none"
        :class="isValidIp(tag) ? 'text-blue-500 hover:text-blue-700' : 'text-red-400 hover:text-red-600'"
        @click.stop="removeTag(tag)"
      >×</button>
    </span>
    <input
      ref="inputRef"
      v-model="pending"
      type="text"
      :list="datalistId"
      :placeholder="modelValue.length === 0 ? placeholder : ''"
      :disabled="disabled"
      class="flex-1 min-w-[120px] text-sm bg-transparent outline-none disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 py-0.5"
      @keydown="handleKeydown"
      @input="handleInputChange"
      @paste="handlePaste"
      @blur="handleBlur"
    />
  </div>
</template>
