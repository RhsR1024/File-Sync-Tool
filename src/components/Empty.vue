<script setup lang="ts">
import type { Component } from 'vue';

withDefaults(defineProps<{
  icon?: Component;
  title?: string;
  description?: string;
  dashed?: boolean;
}>(), {
  dashed: true,
});
</script>

<template>
  <div
    class="flex flex-col items-center justify-center gap-2 py-10 px-4 rounded-lg text-slate-400"
    :class="dashed ? 'border-2 border-dashed border-slate-200 bg-white' : ''"
  >
    <component v-if="icon" :is="icon" class="w-10 h-10 opacity-20" />
    <span v-if="title" class="text-sm font-medium text-slate-500">{{ title }}</span>
    <span v-if="description" class="text-xs text-slate-400 text-center max-w-xs">{{ description }}</span>
    <slot />
  </div>
</template>
