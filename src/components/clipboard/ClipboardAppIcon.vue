<script setup lang="ts">
import { convertFileSrc } from '@tauri-apps/api/core';
import { computed } from 'vue';

const props = withDefaults(
  defineProps<{
    iconPath?: string | null;
    sourceApp?: string | null;
    size?: 'sm' | 'md';
  }>(),
  {
    iconPath: null,
    sourceApp: null,
    size: 'sm',
  },
);

const iconSrc = computed(() =>
  props.iconPath ? convertFileSrc(props.iconPath) : null,
);
const fallbackLabel = computed(() => {
  const source = props.sourceApp?.trim();
  if (!source) return '?';
  return source.charAt(0).toUpperCase();
});
const wrapperClass = computed(() =>
  props.size === 'md'
    ? 'h-7 w-7 text-[11px]'
    : 'h-5 w-5 text-[10px]',
);
const title = computed(() =>
  props.sourceApp?.trim() || 'Unknown app',
);
</script>

<template>
  <span
    class="inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full border border-slate-200 bg-slate-100 font-semibold uppercase text-slate-600"
    :class="wrapperClass"
    :title="title"
  >
    <img
      v-if="iconSrc"
      :src="iconSrc"
      class="h-full w-full object-cover"
      alt=""
    >
    <span v-else>{{ fallbackLabel }}</span>
  </span>
</template>
