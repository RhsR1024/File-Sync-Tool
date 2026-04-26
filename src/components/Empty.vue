<script setup lang="ts">
import type { Component } from 'vue';

withDefaults(defineProps<{
  icon?: Component;
  title?: string;
  description?: string;
  dashed?: boolean;
  /** When provided, renders a CTA button below the description. */
  actionLabel?: string;
  /** Visual treatment of the CTA button. */
  actionTone?: 'primary' | 'subtle';
}>(), {
  dashed: true,
  actionTone: 'primary',
});

defineEmits<{ (e: 'action'): void }>();
</script>

<template>
  <div
    class="flex flex-col items-center justify-center gap-2 py-10 px-4 rounded-lg text-slate-400"
    :class="dashed ? 'border-2 border-dashed border-slate-200 bg-white' : ''"
    aria-live="polite"
  >
    <component v-if="icon" :is="icon" class="w-10 h-10 opacity-20" />
    <span v-if="title" class="text-sm font-medium text-slate-500">{{ title }}</span>
    <span v-if="description" class="text-xs text-slate-400 text-center max-w-xs">{{ description }}</span>
    <slot />
    <button
      v-if="actionLabel"
      type="button"
      class="mt-2 inline-flex items-center justify-center rounded-xl px-4 py-2 text-sm font-semibold transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-white"
      :class="actionTone === 'primary'
        ? 'bg-indigo-600 text-white hover:bg-indigo-700'
        : 'border border-slate-200 bg-white text-slate-700 hover:border-slate-300 hover:bg-slate-50'"
      @click="$emit('action')"
    >
      {{ actionLabel }}
    </button>
  </div>
</template>
