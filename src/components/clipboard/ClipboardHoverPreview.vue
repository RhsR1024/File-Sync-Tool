<script setup lang="ts">
import { convertFileSrc } from '@tauri-apps/api/core';

import type { ClipboardItem } from '@/lib/clipboardTypes';

interface Props {
  item: ClipboardItem;
  scale: number;
}

const props = defineProps<Props>();
</script>

<template>
  <div
    class="pointer-events-none fixed top-6 right-[436px] z-50 max-h-[80vh] max-w-[60vw] overflow-hidden rounded-xl border border-slate-200/60 bg-white/95 p-3 shadow-2xl"
  >
    <div v-if="props.item.kind === 'image' && props.item.image_path" class="relative">
      <img
        :src="convertFileSrc(props.item.image_path)"
        :style="{
          transform: `scale(${props.scale})`,
          transformOrigin: 'top left',
        }"
        class="max-h-[70vh] transition-transform"
        alt=""
      />
      <span
        class="absolute bottom-1 right-1 rounded bg-white/80 px-1.5 py-0.5 text-[10px] font-medium text-slate-500"
      >
        {{ Math.round(props.scale * 100) }}%
      </span>
    </div>
    <pre
      v-else-if="props.item.kind === 'text' || props.item.kind === 'html'"
      class="max-h-[70vh] overflow-y-auto whitespace-pre-wrap font-mono text-xs text-slate-700"
    >{{ props.item.content_full || props.item.content_preview }}</pre>
    <div v-else class="font-mono text-xs text-slate-600">
      {{ props.item.content_preview }}
    </div>
  </div>
</template>
