<script setup lang="ts">
import { Eye } from 'lucide-vue-next';

defineProps<{
  present: boolean;
  redisAvailable: boolean;
  presentLabel: string;
  absentLabel: string;
  unavailableLabel: string;
  detailLabel: string;
  detailAriaLabel: string;
}>();

const emit = defineEmits<{
  (e: 'open-detail'): void;
}>();
</script>

<template>
  <span
    v-if="!redisAvailable"
    class="inline-flex items-center rounded-full border border-amber-200 bg-amber-50 px-2.5 py-1 text-xs font-semibold text-amber-700"
  >
    {{ unavailableLabel }}
  </span>
  <div v-else-if="present" class="inline-flex items-center gap-1.5">
    <span
      class="inline-flex items-center rounded-full border border-indigo-200 bg-indigo-50 px-2.5 py-1 text-xs font-semibold text-indigo-700"
    >
      {{ presentLabel }}
    </span>
    <button
      type="button"
      class="inline-flex h-7 w-7 items-center justify-center rounded-full border border-indigo-200 bg-white text-indigo-600 transition hover:border-indigo-300 hover:bg-indigo-50 hover:text-indigo-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40 focus-visible:ring-offset-1"
      :title="detailLabel"
      :aria-label="detailAriaLabel"
      @click="emit('open-detail')"
    >
      <Eye class="h-3.5 w-3.5" aria-hidden="true" />
    </button>
  </div>
  <span v-else class="text-sm text-slate-400">
    {{ absentLabel }}
  </span>
</template>
