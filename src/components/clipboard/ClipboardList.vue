<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { DynamicScroller, DynamicScrollerItem } from 'vue-virtual-scroller';
import { VueDraggable } from 'vue-draggable-plus';
import { convertFileSrc } from '@tauri-apps/api/core';
import { useI18n } from 'vue-i18n';
import 'vue-virtual-scroller/dist/vue-virtual-scroller.css';

import type { ClipboardItem } from '@/lib/clipboardTypes';

interface Props {
  items: ClipboardItem[];
  selectedId: number | null;
  compact?: boolean;
  draggable?: boolean;
  /** When true, render a visible favorite toggle button on each row. */
  showFavoriteButton?: boolean;
}

const props = defineProps<Props>();

const emit = defineEmits<{
  select: [id: number];
  activate: [id: number];
  favorite: [id: number];
  remove: [id: number];
  reorder: [ids: number[]];
}>();

const { t } = useI18n();

function onRowKeydown(e: KeyboardEvent, id: number) {
  if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault();
    emit('activate', id);
  }
}

function heightOf(it: ClipboardItem): number {
  if (it.kind === 'image') return props.compact ? 120 : 140;
  if (it.kind === 'file') return props.compact ? 64 : 80;
  return props.compact ? 52 : 64;
}

function assetUrl(path: string): string {
  return convertFileSrc(path);
}

function formatTime(tsMs: number): string {
  const d = new Date(tsMs);
  const now = new Date();
  const diffSec = Math.floor((now.getTime() - tsMs) / 1000);
  if (diffSec < 60) return '刚刚';
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)} 分钟前`;
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  if (sameDay) return `今天 ${hh}:${mm}`;
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  const isYesterday =
    d.getFullYear() === yesterday.getFullYear() &&
    d.getMonth() === yesterday.getMonth() &&
    d.getDate() === yesterday.getDate();
  if (isYesterday) return `昨天 ${hh}:${mm}`;
  const mo = String(d.getMonth() + 1).padStart(2, '0');
  const dd = String(d.getDate()).padStart(2, '0');
  return `${mo}-${dd} ${hh}:${mm}`;
}

const itemsWithHeight = computed(() =>
  props.items.map((it) => ({
    ...it,
    _height: heightOf(it),
  })),
);

// Local mutable copy for drag operations; synced from props.
const draggableItems = ref<ClipboardItem[]>([...props.items]);
watch(
  () => props.items,
  (list) => {
    draggableItems.value = [...list];
  },
  { deep: false },
);

function onReorderEnd() {
  emit('reorder', draggableItems.value.map((it) => it.id));
}
</script>

<template>
  <VueDraggable
    v-if="props.draggable"
    v-model="draggableItems"
    class="flex h-full w-full flex-col gap-1 overflow-y-auto"
    @end="onReorderEnd"
  >
    <div
      v-for="it in draggableItems"
      :key="it.id"
      role="button"
      tabindex="0"
      class="flex w-full cursor-move items-start gap-2 rounded-lg px-3 py-2 text-left transition-colors"
      :class="it.id === props.selectedId
        ? 'bg-slate-100 ring-1 ring-slate-300'
        : 'hover:bg-slate-50'"
      :style="{ minHeight: `${heightOf(it)}px` }"
      @mouseenter="emit('select', it.id)"
      @click="emit('activate', it.id)"
      @keydown="onRowKeydown($event, it.id)"
    >
      <span class="inline-flex shrink-0 rounded bg-slate-200/60 px-1.5 py-0.5 text-[10px] uppercase tracking-[0.08em] text-slate-600">
        {{ it.kind }}
      </span>

      <div v-if="it.kind === 'image' && it.image_path" class="flex flex-1 items-center gap-3">
        <img
          :src="assetUrl(it.image_path)"
          class="h-20 w-28 shrink-0 rounded object-cover"
          loading="lazy"
          alt=""
        />
        <span class="text-xs text-slate-500">
          {{ it.image_width ?? '?' }} × {{ it.image_height ?? '?' }}
        </span>
      </div>
      <div v-else-if="it.kind === 'file'" class="flex-1 truncate font-mono text-xs text-slate-600">
        {{ it.content_preview }}
      </div>
      <div v-else class="flex-1 truncate text-sm text-slate-700">
        {{ it.content_preview }}
      </div>
      <span class="shrink-0 self-center text-[10px] tabular-nums text-slate-400">
        {{ formatTime(it.updated_at ?? it.created_at) }}
      </span>
      <button
        v-if="props.showFavoriteButton"
        type="button"
        class="shrink-0 self-center rounded-full p-1 text-base leading-none transition-colors"
        :class="it.is_favorite ? 'text-amber-500 hover:bg-amber-50' : 'text-slate-300 hover:bg-slate-100 hover:text-amber-400'"
        :title="it.is_favorite ? t('clipboard.actions.unfavorite') : t('clipboard.actions.favorite')"
        @click.stop="emit('favorite', it.id)"
      >
        {{ it.is_favorite ? '★' : '☆' }}
      </button>
      <span
        v-else-if="it.is_favorite"
        class="shrink-0 self-center text-xs text-amber-500"
      >★</span>
    </div>
  </VueDraggable>
  <DynamicScroller
    v-else
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
        <div
          role="button"
          tabindex="0"
          class="flex w-full cursor-pointer items-start gap-2 rounded-lg px-3 py-2 text-left transition-colors"
          :class="item.id === props.selectedId
            ? 'bg-slate-100 ring-1 ring-slate-300'
            : 'hover:bg-slate-50'"
          :style="{ minHeight: `${item._height}px` }"
          @mouseenter="emit('select', item.id)"
          @click="emit('activate', item.id)"
          @keydown="onRowKeydown($event, item.id)"
        >
          <span class="inline-flex shrink-0 rounded bg-slate-200/60 px-1.5 py-0.5 text-[10px] uppercase tracking-[0.08em] text-slate-600">
            {{ item.kind }}
          </span>

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
          <span class="shrink-0 self-center text-[10px] tabular-nums text-slate-400">
            {{ formatTime(item.updated_at ?? item.created_at) }}
          </span>
          <button
            v-if="props.showFavoriteButton"
            type="button"
            class="shrink-0 self-center rounded-full p-1 text-base leading-none transition-colors"
            :class="item.is_favorite ? 'text-amber-500 hover:bg-amber-50' : 'text-slate-300 hover:bg-slate-100 hover:text-amber-400'"
            :title="item.is_favorite ? t('clipboard.actions.unfavorite') : t('clipboard.actions.favorite')"
            @click.stop="emit('favorite', item.id)"
          >
            {{ item.is_favorite ? '★' : '☆' }}
          </button>
          <span
            v-else-if="item.is_favorite"
            class="shrink-0 self-center text-xs text-amber-500"
          >★</span>
        </div>
      </DynamicScrollerItem>
    </template>
  </DynamicScroller>
</template>
