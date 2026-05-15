<script setup lang="ts">
import { computed } from 'vue';
import { Pin } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';

import ClipboardList from '@/components/clipboard/ClipboardList.vue';
import { resolveClipboardPinnedSectionHeight } from '@/lib/clipboardListLayout';
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
  batchMode?: boolean;
  selectedIds?: Set<number>;
  showFavoriteButton?: boolean;
  showDeleteButton?: boolean;
  showPinButton?: boolean;
  indexOffset?: number;
}

const props = defineProps<Props>();

const emit = defineEmits<{
  select: [id: number];
  activate: [id: number];
  favorite: [id: number];
  pin: [id: number];
  remove: [id: number];
  toggle: [payload: { id: number; shiftKey: boolean }];
  menu: [payload: { item: ClipboardItem; x: number; y: number }];
  hoverLeave: [];
}>();

const { t } = useI18n();
const defaultDisplaySettings = createDefaultClipboardSettings().display;

const displaySettings = computed(
  () => props.displaySettings ?? defaultDisplaySettings,
);
const highlightKeywords = computed(() => props.highlightKeywords ?? []);
const listHeight = computed(() => {
  const height = resolveClipboardPinnedSectionHeight(
    props.items,
    displaySettings.value,
    { compact: props.compact },
  );
  return `${height}px`;
});
</script>

<template>
  <section v-if="props.items.length" class="flex flex-col gap-2">
    <div class="flex items-center justify-between px-1">
      <div class="flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.18em] text-amber-700">
        <Pin class="h-3.5 w-3.5" />
        <span>{{ t('clipboard.pinnedSection.title') }}</span>
      </div>
      <span class="text-xs text-amber-600/80">{{ props.items.length }}</span>
    </div>

    <div class="overflow-hidden rounded-2xl border border-amber-200 bg-amber-50/70" :style="{ height: listHeight }">
      <ClipboardList
        :items="props.items"
        :selected-id="props.selectedId"
        :display-settings="displaySettings"
        :highlight-keywords="highlightKeywords"
        :compact="props.compact"
        :batch-mode="props.batchMode"
        :selected-ids="props.selectedIds"
        :show-favorite-button="props.showFavoriteButton"
        :show-delete-button="props.showDeleteButton"
        :show-pin-button="props.showPinButton"
        :index-offset="props.indexOffset"
        @select="emit('select', $event)"
        @activate="emit('activate', $event)"
        @favorite="emit('favorite', $event)"
        @pin="emit('pin', $event)"
        @remove="emit('remove', $event)"
        @toggle="emit('toggle', $event)"
        @menu="emit('menu', $event)"
        @hover-leave="emit('hoverLeave')"
      />
    </div>
  </section>
</template>
