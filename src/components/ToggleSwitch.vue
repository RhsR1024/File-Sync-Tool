<script setup lang="ts">
import { computed } from 'vue';

export type ToggleSwitchTone = 'amber' | 'blue' | 'cyan' | 'emerald' | 'rose' | 'sky' | 'teal' | 'violet';
export type ToggleSwitchSize = 'sm' | 'xs' | 'compact' | 'md' | 'lg';

const props = withDefaults(defineProps<{
  modelValue: boolean;
  tone?: ToggleSwitchTone;
  size?: ToggleSwitchSize;
  disabled?: boolean;
  id?: string;
  ariaLabel?: string;
  ariaDescribedby?: string;
}>(), {
  tone: 'blue',
  size: 'md',
  disabled: false,
  id: undefined,
  ariaLabel: undefined,
  ariaDescribedby: undefined,
});

const emit = defineEmits<{
  'update:modelValue': [value: boolean];
}>();

const sizeClass = computed(() => `ui-toggle--${props.size}`);
const toneClass = computed(() => `ui-toggle__track--${props.tone}`);

function onChange(event: Event) {
  emit('update:modelValue', (event.target as HTMLInputElement).checked);
}
</script>

<template>
  <span class="ui-toggle" :class="[sizeClass, { 'ui-toggle--disabled': disabled }]">
    <input
      class="ui-toggle__input"
      :id="id"
      type="checkbox"
      role="switch"
      :checked="modelValue"
      :disabled="disabled"
      :aria-checked="modelValue"
      :aria-label="ariaLabel"
      :aria-describedby="ariaDescribedby"
      @change="onChange"
    >
    <span class="ui-toggle__track" :class="toneClass" aria-hidden="true">
      <span class="ui-toggle__thumb"></span>
    </span>
  </span>
</template>

<style scoped>
.ui-toggle {
  --ui-toggle-track: rgb(203 213 225);
  --ui-toggle-thumb: #fff;
  --ui-toggle-inset: 2px;
  --ui-toggle-thumb-size: 20px;
  --ui-toggle-travel: 20px;
  position: relative;
  display: inline-flex;
  flex: 0 0 auto;
  cursor: pointer;
}

.ui-toggle--sm {
  --ui-toggle-inset: 2px;
  --ui-toggle-thumb-size: 16px;
  --ui-toggle-travel: 16px;
}

.ui-toggle--xs {
  --ui-toggle-inset: 3px;
  --ui-toggle-thumb-size: 16px;
  --ui-toggle-travel: 18px;
}

.ui-toggle--compact {
  --ui-toggle-inset: 2px;
  --ui-toggle-thumb-size: 18px;
  --ui-toggle-travel: 18px;
}

.ui-toggle--lg {
  --ui-toggle-inset: 4px;
  --ui-toggle-thumb-size: 20px;
  --ui-toggle-travel: 20px;
}

.ui-toggle__input {
  position: absolute;
  z-index: 1;
  inset: 0;
  width: 100%;
  height: 100%;
  margin: 0;
  cursor: inherit;
  opacity: 0;
  border: 0;
  padding: 0;
}

.ui-toggle__track {
  position: relative;
  display: block;
  width: 44px;
  height: 24px;
  flex-shrink: 0;
  border-radius: 9999px;
  background: var(--ui-toggle-track);
  transition: background-color 180ms ease;
}

.ui-toggle--sm .ui-toggle__track {
  width: 36px;
  height: 20px;
}

.ui-toggle--xs .ui-toggle__track,
.ui-toggle--compact .ui-toggle__track {
  width: 40px;
  height: 22px;
}

.ui-toggle--lg .ui-toggle__track {
  width: 48px;
  height: 28px;
}

.ui-toggle__thumb {
  position: absolute;
  top: 50%;
  left: var(--ui-toggle-inset);
  width: var(--ui-toggle-thumb-size);
  height: var(--ui-toggle-thumb-size);
  border-radius: 9999px;
  background: var(--ui-toggle-thumb);
  box-shadow: 0 1px 3px rgb(15 23 42 / 0.2);
  transform: translateY(-50%);
  transition: transform 180ms ease;
}

.ui-toggle__input:checked + .ui-toggle__track .ui-toggle__thumb {
  transform: translate(var(--ui-toggle-travel), -50%);
}

.ui-toggle__input:focus-visible + .ui-toggle__track {
  outline: 2px solid var(--ui-toggle-focus);
  outline-offset: 2px;
}

.ui-toggle__input:disabled + .ui-toggle__track {
  cursor: not-allowed;
  opacity: 0.45;
}

.ui-toggle--disabled {
  cursor: not-allowed;
}

.ui-toggle__track--amber { --ui-toggle-active: #f59e0b; --ui-toggle-focus: rgb(245 158 11 / 0.65); }
.ui-toggle__track--blue { --ui-toggle-active: #2563eb; --ui-toggle-focus: rgb(37 99 235 / 0.65); }
.ui-toggle__track--cyan { --ui-toggle-active: #0891b2; --ui-toggle-focus: rgb(6 182 212 / 0.65); }
.ui-toggle__track--emerald { --ui-toggle-active: #059669; --ui-toggle-focus: rgb(16 185 129 / 0.65); }
.ui-toggle__track--rose { --ui-toggle-active: #e11d48; --ui-toggle-focus: rgb(244 63 94 / 0.65); }
.ui-toggle__track--sky { --ui-toggle-active: #0284c7; --ui-toggle-focus: rgb(14 165 233 / 0.65); }
.ui-toggle__track--teal { --ui-toggle-active: #0d9488; --ui-toggle-focus: rgb(20 184 166 / 0.65); }
.ui-toggle__track--violet { --ui-toggle-active: #7c3aed; --ui-toggle-focus: rgb(139 92 246 / 0.65); }

.ui-toggle__input:checked + .ui-toggle__track {
  background: var(--ui-toggle-active);
}

@media (prefers-reduced-motion: reduce) {
  .ui-toggle__track,
  .ui-toggle__thumb {
    transition: none;
  }
}
</style>
