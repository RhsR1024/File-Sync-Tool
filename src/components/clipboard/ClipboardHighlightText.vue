<script setup lang="ts">
import { computed } from 'vue';

import { buildClipboardHighlightParts } from '@/lib/clipboardListPresentation';

const props = withDefaults(
  defineProps<{
    text: string;
    keywords?: string[];
    lines?: number;
  }>(),
  {
    keywords: () => [],
    lines: 0,
  },
);

const parts = computed(() =>
  buildClipboardHighlightParts(props.text, props.keywords),
);

const clampStyle = computed<Record<string, string>>(() => {
  const style: Record<string, string> = {
    whiteSpace: 'pre-wrap',
    wordBreak: 'break-word',
  };

  if (props.lines > 0) {
    style.display = '-webkit-box';
    style.overflow = 'hidden';
    style.WebkitLineClamp = String(props.lines);
    style.WebkitBoxOrient = 'vertical';
  }

  return style;
});
</script>

<template>
  <span class="block min-w-0" :style="clampStyle">
    <template v-for="(part, index) in parts" :key="`${index}-${part.text}`">
      <mark
        v-if="part.match"
        class="rounded bg-amber-200/80 px-[1px] text-inherit"
      >{{ part.text }}</mark>
      <template v-else>{{ part.text }}</template>
    </template>
  </span>
</template>
