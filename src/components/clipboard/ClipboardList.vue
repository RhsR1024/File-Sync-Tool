<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { DynamicScroller, DynamicScrollerItem } from 'vue-virtual-scroller';
import { VueDraggable } from 'vue-draggable-plus';
import { convertFileSrc } from '@tauri-apps/api/core';
import { useI18n } from 'vue-i18n';
import { Ellipsis, Package, Pin, Star, Trash2 } from 'lucide-vue-next';
import 'vue-virtual-scroller/dist/vue-virtual-scroller.css';

import ClipboardAppIcon from '@/components/clipboard/ClipboardAppIcon.vue';
import ClipboardHighlightText from '@/components/clipboard/ClipboardHighlightText.vue';
import {
  formatClipboardTimeLabel,
  resolveClipboardSourceBadge,
} from '@/lib/clipboardListPresentation';
import {
  createDefaultClipboardSettings,
  type ClipboardDisplaySettings,
  type ClipboardItem,
} from '@/lib/clipboardTypes';

interface Props {
  items: ClipboardItem[];
  selectedId: number | null;
  displaySettings?: ClipboardDisplaySettings;
  highlightKeywords?: string[];
  compact?: boolean;
  draggable?: boolean;
  showFavoriteButton?: boolean;
  showPinButton?: boolean;
  showDeleteButton?: boolean;
  batchMode?: boolean;
  selectedIds?: Set<number>;
  indexOffset?: number;
}

const props = defineProps<Props>();

const emit = defineEmits<{
  select: [id: number];
  activate: [id: number];
  favorite: [id: number];
  pin: [id: number];
  remove: [id: number];
  reorder: [ids: number[]];
  toggle: [payload: { id: number; shiftKey: boolean }];
  menu: [payload: { item: ClipboardItem; x: number; y: number }];
  hoverLeave: [];
}>();

const { t } = useI18n();
const defaultDisplaySettings = createDefaultClipboardSettings().display;

const displaySettings = computed<ClipboardDisplaySettings>(
  () => props.displaySettings ?? defaultDisplaySettings,
);
const highlightKeywords = computed(() => props.highlightKeywords ?? []);
const previewLines = computed(() =>
  Math.max(1, Math.min(displaySettings.value.preview_lines ?? 3, 6)),
);
const densityClasses = computed(() => {
  switch (displaySettings.value.density) {
    case 'compact':
      return {
        card: 'gap-0.5 px-2.5 py-1.5',
        text: 'text-xs leading-snug',
        file: 'text-[11px]',
        meta: 'text-[10px]',
        imageCap: 'max-h-20',
        badge: 'text-[9px]',
      };
    case 'spacious':
      return {
        card: 'gap-1.5 px-3.5 py-3',
        text: 'text-[15px] leading-6',
        file: 'text-sm',
        meta: 'text-xs',
        imageCap: 'max-h-32',
        badge: 'text-[11px]',
      };
    case 'standard':
    default:
      return {
        card: 'gap-1 px-3 py-2',
        text: 'text-sm leading-snug',
        file: 'text-xs',
        meta: 'text-[11px]',
        imageCap: 'max-h-24',
        badge: 'text-[10px]',
      };
  }
});

function emitToggleRequest(id: number, shiftKey: boolean) {
  emit('toggle', { id, shiftKey });
}

function onRowKeydown(event: KeyboardEvent, id: number) {
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    emit('select', id);
    if (props.batchMode) emitToggleRequest(id, event.shiftKey);
    else emit('activate', id);
  }
}

function heightOf(item: ClipboardItem): number {
  const densityAdjust = (() => {
    switch (displaySettings.value.density) {
      case 'compact':
        return -8;
      case 'spacious':
        return 16;
      case 'standard':
      default:
        return 0;
    }
  })();

  if (item.kind === 'image') {
    const fixedHeightAdjust = displaySettings.value.image_auto_height ? 0 : 18;
    return (props.compact ? 148 : 168) + densityAdjust + fixedHeightAdjust;
  }
  if (item.kind === 'file') {
    return (props.compact ? 80 : 96) + densityAdjust;
  }

  const lineAdjust = Math.max(0, previewLines.value - 2) * 18;
  return (props.compact ? 72 : 88) + densityAdjust + lineAdjust;
}

function displayIndex(index: number): number {
  return (props.indexOffset ?? 0) + index + 1;
}

function assetUrl(path: string): string {
  return convertFileSrc(path);
}

function formatTime(tsMs: number): string {
  return formatClipboardTimeLabel(
    tsMs,
    displaySettings.value.time_format,
    {
      justNow: t('clipboard.time.justNow'),
      today: t('clipboard.time.today'),
      yesterday: t('clipboard.time.yesterday'),
      minutesAgo: (minutes) => t('clipboard.time.minutesAgo', { n: minutes }),
    },
  );
}

function formatCharCount(item: ClipboardItem): string {
  const text = item.content_full ?? item.content_preview ?? '';
  const count = item.char_count ?? [...text].length;
  if (count >= 10000) {
    return t('clipboard.meta.charCountWan', { n: (count / 10000).toFixed(1) });
  }
  return t('clipboard.meta.charCount', { n: count.toLocaleString() });
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

function metaItems(item: ClipboardItem): string[] {
  const items = [formatTime(item.updated_at ?? item.created_at)];
  if (
    displaySettings.value.show_char_count
    && (item.kind === 'text' || item.kind === 'html' || item.kind === 'rtf')
  ) {
    items.push(formatCharCount(item));
  }
  if (displaySettings.value.show_byte_size && item.byte_size > 0) {
    items.push(formatSize(item.byte_size));
  }
  return items;
}

function imageStyle(): Record<string, string> {
  const maxHeight = `${displaySettings.value.image_max_height}px`;
  return displaySettings.value.image_auto_height
    ? { maxHeight }
    : { height: maxHeight, maxHeight };
}

function sourceBadge(item: ClipboardItem) {
  return resolveClipboardSourceBadge(
    displaySettings.value.show_source_app,
    item,
  );
}

function isSelected(id: number): boolean {
  return props.selectedIds?.has(id) ?? false;
}

function onRowClick(event: MouseEvent, id: number) {
  emit('select', id);
  if (props.batchMode) {
    event.stopPropagation();
    emitToggleRequest(id, event.shiftKey);
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
  props.items.map((item, index) => ({
    ...item,
    _height: heightOf(item),
    _idx: index,
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
  emit('reorder', draggableItems.value.map((item) => item.id));
}
</script>

<template>
  <VueDraggable
    v-if="props.draggable"
    v-model="draggableItems"
    class="flex h-full w-full flex-col gap-1.5 overflow-y-auto px-1"
    @end="onReorderEnd"
  >
    <div
      v-for="(item, index) in draggableItems"
      :key="item.id"
      role="button"
      tabindex="0"
      class="group relative flex w-full cursor-move flex-col rounded-lg border text-left shadow-sm transition-all"
      :class="[
        densityClasses.card,
        item.id === props.selectedId
          ? 'border-slate-300 bg-slate-50'
          : 'border-slate-200 bg-white hover:border-slate-300 hover:shadow',
        props.batchMode && isSelected(item.id) && 'ring-2 ring-blue-400',
      ]"
      :style="{ minHeight: `${heightOf(item)}px` }"
      @mouseenter="emit('select', item.id)"
      @mouseleave="emit('hoverLeave')"
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
          <svg
            v-if="isSelected(item.id)"
            viewBox="0 0 16 16"
            class="h-3 w-3"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
          >
            <polyline points="3 8.5 6.5 12 13 4.5" />
          </svg>
        </span>

        <div v-if="item.kind === 'image' && item.image_path" class="flex-1">
          <img
            :src="assetUrl(item.image_path)"
            :style="imageStyle()"
            class="w-full rounded object-contain"
            :class="densityClasses.imageCap"
            loading="lazy"
            alt=""
          />
        </div>
        <div
          v-else-if="item.kind === 'file'"
          class="flex-1 font-mono text-slate-700"
          :class="densityClasses.file"
        >
          <ClipboardHighlightText
            :text="item.content_preview"
            :keywords="highlightKeywords"
            :lines="previewLines"
          />
        </div>
        <div v-else class="flex-1 text-slate-800" :class="densityClasses.text">
          <ClipboardHighlightText
            :text="item.content_preview"
            :keywords="highlightKeywords"
            :lines="previewLines"
          />
        </div>

        <div class="mt-0.5 flex shrink-0 items-center gap-1">
          <Star
            v-if="item.is_favorite"
            class="h-3.5 w-3.5 text-amber-500"
            fill="currentColor"
            aria-hidden="true"
          />
          <Pin
            v-if="item.is_pinned"
            class="h-3.5 w-3.5 text-amber-700"
            aria-hidden="true"
          />
        </div>
      </div>

      <div
        class="flex items-center justify-between gap-2 text-slate-500"
        :class="densityClasses.meta"
      >
        <div class="flex min-w-0 items-center gap-1.5">
          <template v-for="(meta, metaIndex) in metaItems(item)" :key="metaIndex">
            <span v-if="metaIndex > 0" class="text-slate-300">·</span>
            <span class="truncate">{{ meta }}</span>
          </template>
        </div>

        <div class="flex shrink-0 items-center gap-1.5">
          <span
            v-if="sourceBadge(item).kind === 'self'"
            class="inline-flex items-center gap-1 rounded-full bg-slate-100 px-2 py-0.5 text-slate-700"
            :class="densityClasses.badge"
          >
            <Package class="h-3 w-3" />
            {{ t('clipboard.source.self') }}
          </span>
          <span
            v-else-if="sourceBadge(item).kind === 'app'"
            class="flex items-center gap-1 text-slate-500"
          >
            <ClipboardAppIcon
              v-if="sourceBadge(item).showIcon"
              :icon-path="item.source_app_icon"
              :source-app="item.source_app"
            />
            <span
              v-if="sourceBadge(item).showName"
              class="max-w-[96px] truncate"
            >{{ sourceBadge(item).label }}</span>
          </span>
          <span
            class="inline-flex min-w-[20px] items-center justify-center rounded-full bg-slate-100 px-1.5 font-semibold text-slate-500"
            :class="densityClasses.badge"
          >
            {{ displayIndex(index) }}
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
          v-if="props.showPinButton"
          type="button"
          class="rounded-full p-1 text-slate-400 transition-colors hover:bg-amber-50 hover:text-amber-700"
          :title="item.is_pinned ? t('clipboard.actions.unpin') : t('clipboard.actions.pin')"
          @click.stop="emit('pin', item.id)"
        >
          <Pin class="h-3.5 w-3.5" :fill="item.is_pinned ? 'currentColor' : 'none'" />
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
  </VueDraggable>

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
        :size-dependencies="[
          item.content_preview,
          item.kind,
          item.image_path,
          item.is_favorite,
          item.is_pinned,
          displaySettings.density,
          displaySettings.preview_lines,
          displaySettings.image_max_height,
          displaySettings.image_auto_height,
          displaySettings.show_source_app,
        ]"
      >
        <div
          role="button"
          tabindex="0"
          class="group relative mx-1 my-0.5 flex w-[calc(100%-0.5rem)] cursor-pointer flex-col rounded-lg border text-left shadow-sm transition-all"
          :class="[
            densityClasses.card,
            item.id === props.selectedId
              ? 'border-slate-300 bg-slate-50'
              : 'border-slate-200 bg-white hover:border-slate-300 hover:shadow',
            props.batchMode && isSelected(item.id) && 'ring-2 ring-blue-400',
          ]"
          :style="{ minHeight: `${item._height - 6}px` }"
          @mouseenter="emit('select', item.id)"
          @mouseleave="emit('hoverLeave')"
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
              <svg
                v-if="isSelected(item.id)"
                viewBox="0 0 16 16"
                class="h-3 w-3"
                fill="none"
                stroke="currentColor"
                stroke-width="2.5"
              >
                <polyline points="3 8.5 6.5 12 13 4.5" />
              </svg>
            </span>

            <div v-if="item.kind === 'image' && item.image_path" class="flex-1">
              <img
                :src="assetUrl(item.image_path)"
                :style="imageStyle()"
                class="w-full rounded object-contain"
                :class="densityClasses.imageCap"
                loading="lazy"
                alt=""
              />
            </div>
            <div
              v-else-if="item.kind === 'file'"
              class="flex-1 font-mono text-slate-700"
              :class="densityClasses.file"
            >
              <ClipboardHighlightText
                :text="item.content_preview"
                :keywords="highlightKeywords"
                :lines="previewLines"
              />
            </div>
            <div v-else class="flex-1 text-slate-800" :class="densityClasses.text">
              <ClipboardHighlightText
                :text="item.content_preview"
                :keywords="highlightKeywords"
                :lines="previewLines"
              />
            </div>

            <div class="mt-0.5 flex shrink-0 items-center gap-1">
              <Star
                v-if="item.is_favorite"
                class="h-3.5 w-3.5 text-amber-500"
                fill="currentColor"
                aria-hidden="true"
              />
              <Pin
                v-if="item.is_pinned"
                class="h-3.5 w-3.5 text-amber-700"
                aria-hidden="true"
              />
            </div>
          </div>

          <div
            class="flex items-center justify-between gap-2 text-slate-500"
            :class="densityClasses.meta"
          >
            <div class="flex min-w-0 items-center gap-1.5">
              <template v-for="(meta, metaIndex) in metaItems(item)" :key="metaIndex">
                <span v-if="metaIndex > 0" class="text-slate-300">·</span>
                <span class="truncate">{{ meta }}</span>
              </template>
            </div>

            <div class="flex shrink-0 items-center gap-1.5">
              <span
                v-if="sourceBadge(item).kind === 'self'"
                class="inline-flex items-center gap-1 rounded-full bg-slate-100 px-2 py-0.5 text-slate-700"
                :class="densityClasses.badge"
              >
                <Package class="h-3 w-3" />
                {{ t('clipboard.source.self') }}
              </span>
              <span
                v-else-if="sourceBadge(item).kind === 'app'"
                class="flex items-center gap-1 text-slate-500"
              >
                <ClipboardAppIcon
                  v-if="sourceBadge(item).showIcon"
                  :icon-path="item.source_app_icon"
                  :source-app="item.source_app"
                />
                <span
                  v-if="sourceBadge(item).showName"
                  class="max-w-[96px] truncate"
                >{{ sourceBadge(item).label }}</span>
              </span>
              <span
                class="inline-flex min-w-[20px] items-center justify-center rounded-full bg-slate-100 px-1.5 font-semibold text-slate-500"
                :class="densityClasses.badge"
              >
                {{ displayIndex(item._idx) }}
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
              v-if="props.showPinButton"
              type="button"
              class="rounded-full p-1 text-slate-400 transition-colors hover:bg-amber-50 hover:text-amber-700"
              :title="item.is_pinned ? t('clipboard.actions.unpin') : t('clipboard.actions.pin')"
              @click.stop="emit('pin', item.id)"
            >
              <Pin class="h-3.5 w-3.5" :fill="item.is_pinned ? 'currentColor' : 'none'" />
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
