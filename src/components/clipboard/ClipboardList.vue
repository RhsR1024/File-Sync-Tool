<script setup lang="ts">
import { computed } from 'vue';
import { DynamicScroller, DynamicScrollerItem } from 'vue-virtual-scroller';
import { convertFileSrc } from '@tauri-apps/api/core';
import 'vue-virtual-scroller/dist/vue-virtual-scroller.css';

import type { ClipboardItem } from '@/lib/clipboardTypes';

interface Props {
  items: ClipboardItem[];
  selectedId: number | null;
  compact?: boolean;
}

const props = defineProps<Props>();

const emit = defineEmits<{
  select: [id: number];
  activate: [id: number];
  favorite: [id: number];
  remove: [id: number];
}>();

function heightOf(it: ClipboardItem): number {
  if (it.kind === 'image') return props.compact ? 120 : 140;
  if (it.kind === 'file') return props.compact ? 64 : 80;
  return props.compact ? 52 : 64;
}

function assetUrl(path: string): string {
  return convertFileSrc(path);
}

const itemsWithHeight = computed(() =>
  props.items.map((it) => ({
    ...it,
    _height: heightOf(it),
  })),
);
</script>

<template>
  <DynamicScroller
    :items="itemsWithHeight"
    :min-item-size="52"
    key-field="id"
    class="h-full w-full"
  >
    <template #default="{ item, active }">
      <DynamicScrollerItem
        :item="item"
        :active="active"
        :size-dependencies="[item.content_preview, item.kind, item.image_path]"
      >
        <button
          type="button"
          class="flex w-full items-start gap-2 rounded-lg px-3 py-2 text-left transition-colors"
          :class="item.id === props.selectedId
            ? 'bg-slate-100 ring-1 ring-slate-300'
            : 'hover:bg-slate-50'"
          :style="{ minHeight: `${item._height}px` }"
          @mouseenter="emit('select', item.id)"
          @click="emit('activate', item.id)"
        >
          <span class="inline-flex shrink-0 rounded bg-slate-200/60 px-1.5 py-0.5 text-[10px] uppercase tracking-[0.08em] text-slate-600">
            {{ item.kind }}
          </span>
          <span v-if="item.is_favorite" class="shrink-0 text-xs text-amber-500">★</span>

          <div v-if="item.kind === 'image' && item.image_path" class="flex flex-1 items-center gap-3">
            <img
              :src="assetUrl(item.image_path)"
              class="h-20 w-28 shrink-0 rounded object-cover"
              loading="lazy"
              alt=""
            />
            <span class="text-xs text-slate-500">
              {{ item.image_width ?? '?' }} × {{ item.image_height ?? '?' }}
            </span>
          </div>
          <div v-else-if="item.kind === 'file'" class="flex-1 truncate font-mono text-xs text-slate-600">
            {{ item.content_preview }}
          </div>
          <div v-else class="flex-1 truncate text-sm text-slate-700">
            {{ item.content_preview }}
          </div>
        </button>
      </DynamicScrollerItem>
    </template>
  </DynamicScroller>
</template>
