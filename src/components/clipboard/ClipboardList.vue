<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { DynamicScroller, DynamicScrollerItem } from 'vue-virtual-scroller';
import { VueDraggable } from 'vue-draggable-plus';
import { convertFileSrc } from '@tauri-apps/api/core';
import { useI18n } from 'vue-i18n';
import { AppWindow, Ellipsis, Trash2, Star } from 'lucide-vue-next';
import 'vue-virtual-scroller/dist/vue-virtual-scroller.css';

import type { ClipboardItem } from '@/lib/clipboardTypes';

interface Props {
  items: ClipboardItem[];
  selectedId: number | null;
  compact?: boolean;
  draggable?: boolean;
  /** When true, render a visible favorite toggle button on each row. */
  showFavoriteButton?: boolean;
  /** When true, render an inline delete button on each row. */
  showDeleteButton?: boolean;
  /** When true, prepend a checkbox and handle click → toggleSelect instead of activate. */
  batchMode?: boolean;
  /** Set of selected ids (only meaningful in batchMode). */
  selectedIds?: Set<number>;
}

const props = defineProps<Props>();

const emit = defineEmits<{
  select: [id: number];
  activate: [id: number];
  favorite: [id: number];
  remove: [id: number];
  reorder: [ids: number[]];
  toggle: [payload: { id: number; shiftKey: boolean }];
  menu: [payload: { item: ClipboardItem; x: number; y: number }];
}>();

const { t } = useI18n();

function emitToggleRequest(id: number, shiftKey: boolean) {
  emit('toggle', { id, shiftKey });
}

function onRowKeydown(e: KeyboardEvent, id: number) {
  if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault();
    emit('select', id);
    if (props.batchMode) emitToggleRequest(id, e.shiftKey);
    else emit('activate', id);
  }
}

function heightOf(it: ClipboardItem): number {
  if (it.kind === 'image') return props.compact ? 148 : 168;
  if (it.kind === 'file') return props.compact ? 80 : 96;
  return props.compact ? 72 : 88;
}

function assetUrl(path: string): string {
  return convertFileSrc(path);
}

function formatTime(tsMs: number): string {
  const d = new Date(tsMs);
  const now = new Date();
  const diffSec = Math.floor((now.getTime() - tsMs) / 1000);
  if (diffSec < 60) return t('clipboard.time.justNow');
  if (diffSec < 3600)
    return t('clipboard.time.minutesAgo', { n: Math.floor(diffSec / 60) });
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  if (sameDay) return `${t('clipboard.time.today')} ${hh}:${mm}`;
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  const isYesterday =
    d.getFullYear() === yesterday.getFullYear() &&
    d.getMonth() === yesterday.getMonth() &&
    d.getDate() === yesterday.getDate();
  if (isYesterday) return `${t('clipboard.time.yesterday')} ${hh}:${mm}`;
  const mo = String(d.getMonth() + 1).padStart(2, '0');
  const dd = String(d.getDate()).padStart(2, '0');
  return `${mo}-${dd} ${hh}:${mm}`;
}

function formatCharCount(it: ClipboardItem): string {
  // Prefer content_full length; fall back to preview (truncated).
  const text = it.content_full ?? it.content_preview ?? '';
  const n = [...text].length; // code-point count; close enough for display
  if (n >= 10000) return t('clipboard.meta.charCountWan', { n: (n / 10000).toFixed(1) });
  return t('clipboard.meta.charCount', { n: n.toLocaleString() });
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

function metaItems(it: ClipboardItem): string[] {
  const list: string[] = [formatTime(it.updated_at ?? it.created_at)];
  if (it.kind === 'text' || it.kind === 'html') list.push(formatCharCount(it));
  if (it.byte_size > 0) list.push(formatSize(it.byte_size));
  return list;
}

function isSelected(id: number): boolean {
  return props.selectedIds?.has(id) ?? false;
}

function onRowClick(e: MouseEvent, id: number) {
  emit('select', id);
  if (props.batchMode) {
    e.stopPropagation();
    emitToggleRequest(id, e.shiftKey);
    return;
  }
  emit('activate', id);
}

function emitMenuRequest(item: ClipboardItem, x: number, y: number) {
  emit('menu', { item, x, y });
}

function onRowContextMenu(event: MouseEvent, item: ClipboardItem) {
  if (props.batchMode) return;
  event.preventDefault();
  emit('select', item.id);
  emitMenuRequest(item, event.clientX, event.clientY);
}

function onMenuButtonClick(event: MouseEvent, item: ClipboardItem) {
  const target = event.currentTarget as HTMLElement | null;
  if (!target) return;
  const rect = target.getBoundingClientRect();
  emit('select', item.id);
  emitMenuRequest(item, rect.right - 12, rect.bottom + 6);
}

const itemsWithHeight = computed(() =>
  props.items.map((it, idx) => ({
    ...it,
    _height: heightOf(it),
    _idx: idx,
  })),
);

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
  <!-- Draggable mode (favorites) -->
  <VueDraggable
    v-if="props.draggable"
    v-model="draggableItems"
    class="flex h-full w-full flex-col gap-1.5 overflow-y-auto px-1"
    @end="onReorderEnd"
  >
    <div
      v-for="(it, idx) in draggableItems"
      :key="it.id"
      role="button"
      tabindex="0"
      class="group relative flex w-full cursor-move flex-col gap-1 rounded-lg border px-3 py-2 text-left shadow-sm transition-all"
      :class="[
        it.id === props.selectedId
          ? 'border-slate-300 bg-slate-50'
          : 'border-slate-200 bg-white hover:border-slate-300 hover:shadow',
        props.batchMode && isSelected(it.id) && 'ring-2 ring-blue-400',
      ]"
      :style="{ minHeight: `${heightOf(it)}px` }"
      @mouseenter="emit('select', it.id)"
      @click="onRowClick($event, it.id)"
      @keydown="onRowKeydown($event, it.id)"
      @contextmenu="onRowContextMenu($event, it)"
    >
      <div class="flex items-start gap-2">
        <span
          v-if="props.batchMode"
          class="mt-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded border"
          :class="isSelected(it.id)
            ? 'border-blue-500 bg-blue-500 text-white'
            : 'border-slate-300 bg-white'"
          aria-hidden
        >
          <svg v-if="isSelected(it.id)" viewBox="0 0 16 16" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2.5">
            <polyline points="3 8.5 6.5 12 13 4.5" />
          </svg>
        </span>

        <div v-if="it.kind === 'image' && it.image_path" class="flex-1">
          <img
            :src="assetUrl(it.image_path)"
            class="max-h-24 w-full rounded object-contain"
            loading="lazy"
            alt=""
          />
        </div>
        <div v-else-if="it.kind === 'file'" class="flex-1 truncate font-mono text-xs text-slate-700">
          {{ it.content_preview }}
        </div>
        <div v-else class="flex-1 break-all text-sm leading-snug text-slate-800 line-clamp-2">
          {{ it.content_preview }}
        </div>

        <span
          v-if="it.is_favorite"
          class="shrink-0 text-[13px] text-amber-500"
          aria-hidden
        >★</span>
      </div>

      <div class="flex items-center justify-between gap-2 text-[11px] text-slate-500">
        <div class="flex min-w-0 items-center gap-1.5">
          <template v-for="(m, i) in metaItems(it)" :key="i">
            <span v-if="i > 0" class="text-slate-300">·</span>
            <span class="truncate">{{ m }}</span>
          </template>
        </div>
        <div class="flex shrink-0 items-center gap-1.5">
          <span v-if="it.source_app" class="flex items-center gap-1 text-slate-500">
            <AppWindow class="h-3 w-3 text-slate-400" />
            <span class="max-w-[96px] truncate">{{ it.source_app }}</span>
          </span>
          <span class="inline-flex min-w-[20px] items-center justify-center rounded-full bg-slate-100 px-1.5 text-[10px] font-semibold text-slate-500">
            {{ idx + 1 }}
          </span>
        </div>
      </div>

      <div
        v-if="!props.batchMode"
        class="pointer-events-none absolute right-1.5 top-1.5 flex items-center gap-1 opacity-0 transition-opacity group-hover:pointer-events-auto group-hover:opacity-100"
      >
        <button
          v-if="props.showFavoriteButton"
          type="button"
          class="rounded-full p-1 text-slate-400 transition-colors hover:bg-amber-50 hover:text-amber-500"
          :title="it.is_favorite ? t('clipboard.actions.unfavorite') : t('clipboard.actions.favorite')"
          @click.stop="emit('favorite', it.id)"
        >
          <Star class="h-3.5 w-3.5" :fill="it.is_favorite ? 'currentColor' : 'none'" />
        </button>
        <button
          v-if="props.showDeleteButton"
          type="button"
          class="rounded-full p-1 text-slate-400 transition-colors hover:bg-red-50 hover:text-red-500"
          :title="t('clipboard.actions.delete')"
          @click.stop="emit('remove', it.id)"
        >
          <Trash2 class="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          class="rounded-full p-1 text-slate-400 transition-colors hover:bg-slate-100 hover:text-slate-700"
          :title="t('clipboard.actions.moreActions')"
          @click.stop="onMenuButtonClick($event, it)"
        >
          <Ellipsis class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  </VueDraggable>

  <!-- Virtual list mode -->
  <DynamicScroller
    v-else
    :items="itemsWithHeight"
    :min-item-size="72"
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
          class="group relative mx-1 my-0.5 flex w-[calc(100%-0.5rem)] cursor-pointer flex-col gap-1 rounded-lg border px-3 py-2 text-left shadow-sm transition-all"
          :class="[
            item.id === props.selectedId
              ? 'border-slate-300 bg-slate-50'
              : 'border-slate-200 bg-white hover:border-slate-300 hover:shadow',
            props.batchMode && isSelected(item.id) && 'ring-2 ring-blue-400',
          ]"
          :style="{ minHeight: `${item._height - 6}px` }"
          @mouseenter="emit('select', item.id)"
          @click="onRowClick($event, item.id)"
          @keydown="onRowKeydown($event, item.id)"
          @contextmenu="onRowContextMenu($event, item)"
        >
          <div class="flex items-start gap-2">
            <span
              v-if="props.batchMode"
              class="mt-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded border"
              :class="isSelected(item.id)
                ? 'border-blue-500 bg-blue-500 text-white'
                : 'border-slate-300 bg-white'"
              aria-hidden
            >
              <svg v-if="isSelected(item.id)" viewBox="0 0 16 16" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2.5">
                <polyline points="3 8.5 6.5 12 13 4.5" />
              </svg>
            </span>

            <div v-if="item.kind === 'image' && item.image_path" class="flex-1">
              <img
                :src="assetUrl(item.image_path)"
                class="max-h-24 w-full rounded object-contain"
                loading="lazy"
                alt=""
              />
            </div>
            <div v-else-if="item.kind === 'file'" class="flex-1 truncate font-mono text-xs text-slate-700">
              {{ item.content_preview }}
            </div>
            <div v-else class="flex-1 break-all text-sm leading-snug text-slate-800 line-clamp-2">
              {{ item.content_preview }}
            </div>

            <span
              v-if="item.is_favorite"
              class="shrink-0 text-[13px] text-amber-500"
              aria-hidden
            >★</span>
          </div>

          <div class="flex items-center justify-between gap-2 text-[11px] text-slate-500">
            <div class="flex min-w-0 items-center gap-1.5">
              <template v-for="(m, i) in metaItems(item)" :key="i">
                <span v-if="i > 0" class="text-slate-300">·</span>
                <span class="truncate">{{ m }}</span>
              </template>
            </div>
            <div class="flex shrink-0 items-center gap-1.5">
              <span v-if="item.source_app" class="flex items-center gap-1 text-slate-500">
                <AppWindow class="h-3 w-3 text-slate-400" />
                <span class="max-w-[96px] truncate">{{ item.source_app }}</span>
              </span>
              <span class="inline-flex min-w-[20px] items-center justify-center rounded-full bg-slate-100 px-1.5 text-[10px] font-semibold text-slate-500">
                {{ item._idx + 1 }}
              </span>
            </div>
          </div>

          <div
            v-if="!props.batchMode"
            class="pointer-events-none absolute right-1.5 top-1.5 flex items-center gap-1 opacity-0 transition-opacity group-hover:pointer-events-auto group-hover:opacity-100"
          >
            <button
              v-if="props.showFavoriteButton"
              type="button"
              class="rounded-full p-1 text-slate-400 transition-colors hover:bg-amber-50 hover:text-amber-500"
              :title="item.is_favorite ? t('clipboard.actions.unfavorite') : t('clipboard.actions.favorite')"
              @click.stop="emit('favorite', item.id)"
            >
              <Star class="h-3.5 w-3.5" :fill="item.is_favorite ? 'currentColor' : 'none'" />
            </button>
            <button
              v-if="props.showDeleteButton"
              type="button"
              class="rounded-full p-1 text-slate-400 transition-colors hover:bg-red-50 hover:text-red-500"
              :title="t('clipboard.actions.delete')"
              @click.stop="emit('remove', item.id)"
            >
              <Trash2 class="h-3.5 w-3.5" />
            </button>
            <button
              type="button"
              class="rounded-full p-1 text-slate-400 transition-colors hover:bg-slate-100 hover:text-slate-700"
              :title="t('clipboard.actions.moreActions')"
              @click.stop="onMenuButtonClick($event, item)"
            >
              <Ellipsis class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </DynamicScrollerItem>
    </template>
  </DynamicScroller>
</template>
