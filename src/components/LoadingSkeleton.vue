<script setup lang="ts">
import { computed } from 'vue';

defineOptions({ name: 'LoadingSkeleton' });

type SkeletonVariant = 'text-line' | 'card' | 'list-row' | 'custom';

const props = withDefaults(
  defineProps<{
    variant?: SkeletonVariant;
    /** For text-line: number of stacked lines. Defaults to 1. */
    lines?: number;
    /** For list-row: number of rows. Defaults to 3. */
    count?: number;
  }>(),
  {
    variant: 'text-line',
    lines: 1,
    count: 3,
  },
);

const lineCount = computed(() => Math.max(1, props.lines ?? 1));
const rowCount = computed(() => Math.max(1, props.count ?? 3));

// Reduced-motion users still see the gray block but no pulse, so the shape
// keeps signalling "content is loading" without repeating animation.
const baseClass =
  'bg-slate-200/70 animate-pulse motion-reduce:animate-none rounded-md';
</script>

<template>
  <div
    v-if="variant === 'text-line'"
    class="flex flex-col gap-2"
    role="status"
    aria-live="polite"
    aria-busy="true"
  >
    <span
      v-for="(_, index) in lineCount"
      :key="index"
      :class="baseClass"
      class="h-3 w-full last:w-3/4"
    />
  </div>

  <div
    v-else-if="variant === 'card'"
    role="status"
    aria-live="polite"
    aria-busy="true"
    :class="baseClass"
    class="h-32 w-full"
  />

  <div
    v-else-if="variant === 'list-row'"
    class="flex flex-col gap-3"
    role="status"
    aria-live="polite"
    aria-busy="true"
  >
    <div
      v-for="(_, index) in rowCount"
      :key="index"
      class="flex items-center gap-3"
    >
      <span :class="baseClass" class="h-9 w-9 rounded-full" />
      <div class="flex flex-1 flex-col gap-2">
        <span :class="baseClass" class="h-3 w-1/2" />
        <span :class="baseClass" class="h-3 w-3/4" />
      </div>
    </div>
  </div>

  <div
    v-else
    role="status"
    aria-live="polite"
    aria-busy="true"
    :class="baseClass"
  />
</template>
