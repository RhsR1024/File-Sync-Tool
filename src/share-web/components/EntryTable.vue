<script setup lang="ts">
import { computed, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import { fileShareApi } from '../api';
import { entryListHint } from '../lib/entry-display';
import { getFileGlyphStyle, isCjk } from '../lib/file-glyph';
import type { EntrySortDirection, EntrySortKey } from '../lib/sort-preference';
import { timeAgo } from '../lib/time-ago';
import type { EntryViewMode } from '../lib/view-mode';
import {
  canPreviewEntry,
  canRenderEntryThumbnail,
  formatFileSize,
  type FileShareNode,
  type FileShareSession,
} from '../types';

import { Icon } from './icons';

const props = defineProps<{
  entries: FileShareNode[];
  session?: FileShareSession | null;
  loading?: boolean;
  emptyText?: string;
  searchActive?: boolean;
  view?: EntryViewMode;
  selectedIds: Set<string>;
  sortKey: EntrySortKey;
  sortDirection: EntrySortDirection;
}>();

const emit = defineEmits<{
  open: [entry: FileShareNode];
  preview: [entry: FileShareNode];
  download: [entry: FileShareNode];
  rename: [entry: FileShareNode];
  delete: [entry: FileShareNode];
  'toggle-select': [nodeId: string];
  'select-all': [];
  sort: [key: EntrySortKey];
}>();

const { t } = useI18n();
const failedThumbnails = ref<Record<string, boolean>>({});

const view = computed<EntryViewMode>(() => props.view ?? 'list');

const folderCount = computed(() => props.entries.filter((entry) => entry.is_dir).length);
const fileCount = computed(() => props.entries.length - folderCount.value);

const isAllSelected = computed(() => (
  props.entries.length > 0
  && props.entries.every((entry) => props.selectedIds.has(entry.node_id))
));

const isSomeSelected = computed(() => (
  props.entries.some((entry) => props.selectedIds.has(entry.node_id))
));

const headerCheckState = computed<'unchecked' | 'partial' | 'checked'>(() => {
  if (isAllSelected.value) {
    return 'checked';
  }
  if (isSomeSelected.value) {
    return 'partial';
  }
  return 'unchecked';
});

const sortLabel = computed(() => {
  if (props.sortKey === 'name') {
    return t('app.sortByName');
  }
  if (props.sortKey === 'size') {
    return t('app.sortBySize');
  }
  return t('app.sortByModified');
});

const sortDirectionLabel = computed(() => (
  props.sortDirection === 'asc' ? t('app.sortAscending') : t('app.sortDescending')
));

const sortSummary = computed(() => (
  t('app.sortSummary', { label: sortLabel.value, dir: sortDirectionLabel.value })
));

function sortHeaderAria(label: string): string {
  return t('app.sortByLabel', { label });
}

function sortAriaValue(key: EntrySortKey): 'ascending' | 'descending' | 'none' {
  if (props.sortKey !== key) {
    return 'none';
  }
  return props.sortDirection === 'asc' ? 'ascending' : 'descending';
}

const timeAgoFormatter = computed(() => ({
  justNow: t('app.timeAgoJustNow'),
  minutes: (n: number) => t('app.timeAgoMinutes', { n }),
  hours: (n: number) => t('app.timeAgoHours', { n }),
  days: (n: number) => t('app.timeAgoDays', { n }),
  months: (n: number) => t('app.timeAgoMonths', { n }),
  years: (n: number) => t('app.timeAgoYears', { n }),
}));

function canPreview(entry: FileShareNode): boolean {
  return canPreviewEntry(props.session ?? null, entry);
}

function canShowThumbnail(entry: FileShareNode): boolean {
  return !failedThumbnails.value[entry.node_id]
    && canRenderEntryThumbnail(props.session ?? null, entry);
}

function markThumbnailFailed(nodeId: string) {
  failedThumbnails.value = {
    ...failedThumbnails.value,
    [nodeId]: true,
  };
}

function canDownload(entry: FileShareNode): boolean {
  return entry.is_dir
    ? entry.permissions.download_archive
    : entry.permissions.download_file;
}

function relativeTime(value: string | null | undefined): string {
  return timeAgo(value, timeAgoFormatter.value);
}

function hintLine(entry: FileShareNode): string {
  return entryListHint(entry, Boolean(props.searchActive));
}

function isHintCjk(value: string): boolean {
  return Boolean(value) && isCjk(value);
}

function handleTileClick(entry: FileShareNode, event: MouseEvent) {
  if (event.shiftKey) {
    emit('toggle-select', entry.node_id);
    return;
  }
  emit('open', entry);
}
</script>

<template>
  <div class="list-card" :class="{ 'grid-mode': view === 'grid' }">
    <template v-if="view === 'list'">
      <div class="list-meta">
        <span>
          <span class="count">{{ entries.length }}</span> {{ t('app.itemUnit') }}
        </span>
        <span class="sep" aria-hidden="true">·</span>
        <span>
          {{ t('app.folderCount', { n: folderCount }) }} · {{ t('app.fileCount', { n: fileCount }) }}
        </span>
        <div class="sort">
          <Icon :name="sortDirection === 'asc' ? 'sortAsc' : 'sortDesc'" />
          <span>{{ sortSummary }}</span>
        </div>
      </div>

      <div class="list-head" v-if="entries.length > 0">
        <button
          type="button"
          class="check"
          :class="{ checked: headerCheckState === 'checked' }"
          :aria-label="t('table.selectAll')"
          :aria-checked="headerCheckState === 'checked' ? 'true' : (headerCheckState === 'partial' ? 'mixed' : 'false')"
          @click="emit('select-all')"
        >
          <Icon name="check" />
        </button>
        <button
          type="button"
          class="col-sort"
          :class="{ active: sortKey === 'name' }"
          :aria-sort="sortAriaValue('name')"
          :aria-label="sortHeaderAria(t('table.name'))"
          @click="emit('sort', 'name')"
        >
          <span>{{ t('table.name') }}</span>
          <Icon
            v-if="sortKey === 'name'"
            :name="sortDirection === 'asc' ? 'chevronUp' : 'chevronDown'"
          />
        </button>
        <button
          type="button"
          class="col-sort"
          :class="{ active: sortKey === 'size' }"
          :aria-sort="sortAriaValue('size')"
          :aria-label="sortHeaderAria(t('table.size'))"
          @click="emit('sort', 'size')"
        >
          <span>{{ t('table.size') }}</span>
          <Icon
            v-if="sortKey === 'size'"
            :name="sortDirection === 'asc' ? 'chevronUp' : 'chevronDown'"
          />
        </button>
        <button
          type="button"
          class="col-sort"
          :class="{ active: sortKey === 'modified' }"
          :aria-sort="sortAriaValue('modified')"
          :aria-label="sortHeaderAria(t('table.modified'))"
          @click="emit('sort', 'modified')"
        >
          <span>{{ t('table.modified') }}</span>
          <Icon
            v-if="sortKey === 'modified'"
            :name="sortDirection === 'asc' ? 'chevronUp' : 'chevronDown'"
          />
        </button>
        <span class="col-actions">{{ t('table.actions') }}</span>
      </div>

      <div v-if="loading && entries.length === 0" class="empty">
        <div class="title">{{ t('table.loading') }}</div>
      </div>

      <div v-else-if="entries.length === 0" class="empty">
        <div class="icon"><Icon name="search" /></div>
        <div class="title">{{ emptyText || t('table.empty') }}</div>
      </div>

      <div
        v-for="entry in entries"
        v-else
        :key="entry.node_id"
        class="row"
        :class="{ selected: selectedIds.has(entry.node_id) }"
      >
        <button
          type="button"
          class="check"
          :class="{ checked: selectedIds.has(entry.node_id) }"
          :aria-label="t('table.select')"
          :aria-checked="selectedIds.has(entry.node_id) ? 'true' : 'false'"
          @click.stop="emit('toggle-select', entry.node_id)"
        >
          <Icon name="check" />
        </button>

        <button
          v-if="entry.is_dir"
          type="button"
          class="name-cell interactive"
          @click="emit('open', entry)"
        >
          <span
            v-if="canShowThumbnail(entry)"
            class="glyph"
            aria-hidden="true"
          >
            <img
              :src="fileShareApi.previewUrl(entry.node_id)"
              alt=""
              loading="lazy"
              @error="markThumbnailFailed(entry.node_id)"
            >
          </span>
          <span
            v-else-if="entry.is_dir"
            class="glyph folder"
            aria-hidden="true"
          >
            <Icon name="folder" />
          </span>
          <span
            v-else
            class="glyph ext"
            :style="{
              color: getFileGlyphStyle(entry.name).color,
              background: getFileGlyphStyle(entry.name).bg,
              borderColor: getFileGlyphStyle(entry.name).border,
            }"
            aria-hidden="true"
          >
            {{ getFileGlyphStyle(entry.name).label }}
          </span>
          <span class="name-text">
            <span class="n">{{ entry.name }}</span>
            <span
              v-if="hintLine(entry)"
              class="hint"
              :class="{ cn: isHintCjk(hintLine(entry)) }"
            >{{ hintLine(entry) }}</span>
          </span>
        </button>
        <div v-else class="name-cell">
          <span
            v-if="canShowThumbnail(entry)"
            class="glyph"
            aria-hidden="true"
          >
            <img
              :src="fileShareApi.previewUrl(entry.node_id)"
              alt=""
              loading="lazy"
              @error="markThumbnailFailed(entry.node_id)"
            >
          </span>
          <span
            v-else
            class="glyph ext"
            :style="{
              color: getFileGlyphStyle(entry.name).color,
              background: getFileGlyphStyle(entry.name).bg,
              borderColor: getFileGlyphStyle(entry.name).border,
            }"
            aria-hidden="true"
          >
            {{ getFileGlyphStyle(entry.name).label }}
          </span>
          <span class="name-text">
            <span class="n">{{ entry.name }}</span>
            <span
              v-if="hintLine(entry)"
              class="hint"
              :class="{ cn: isHintCjk(hintLine(entry)) }"
            >{{ hintLine(entry) }}</span>
          </span>
        </div>

        <span class="cell-size">
          {{ entry.is_dir ? '-' : formatFileSize(entry.size) }}
        </span>

        <span class="cell-modified">
          {{ entry.modified || '-' }}
          <span v-if="entry.modified" class="ago">{{ relativeTime(entry.modified) }}</span>
        </span>

        <div class="cell-actions">
          <button
            v-if="canPreview(entry)"
            type="button"
            class="row-action"
            :title="t('table.preview')"
            :aria-label="t('table.preview')"
            @click="emit('preview', entry)"
          >
            <Icon name="preview" />
          </button>
          <button
            v-if="canDownload(entry)"
            type="button"
            class="row-action primary"
            :title="t('table.download')"
            :aria-label="t('table.download')"
            @click="emit('download', entry)"
          >
            <Icon name="download" />
          </button>
          <button
            v-if="entry.permissions.rename"
            type="button"
            class="row-action"
            :title="t('table.rename')"
            :aria-label="t('table.rename')"
            @click="emit('rename', entry)"
          >
            <Icon name="edit" />
          </button>
          <button
            v-if="entry.permissions.delete"
            type="button"
            class="row-action danger"
            :title="t('table.delete')"
            :aria-label="t('table.delete')"
            @click="emit('delete', entry)"
          >
            <Icon name="trash" />
          </button>
        </div>
      </div>
    </template>

    <template v-else>
      <div v-if="loading && entries.length === 0" class="empty">
        <div class="title">{{ t('table.loading') }}</div>
      </div>

      <div v-else-if="entries.length === 0" class="empty">
        <div class="icon"><Icon name="search" /></div>
        <div class="title">{{ emptyText || t('table.empty') }}</div>
      </div>

      <div v-else class="grid-card">
        <button
          v-for="entry in entries"
          :key="entry.node_id"
          type="button"
          class="tile"
          :class="{ selected: selectedIds.has(entry.node_id) }"
          @click="handleTileClick(entry, $event)"
        >
          <span
            v-if="canShowThumbnail(entry)"
            class="tile-glyph"
            aria-hidden="true"
          >
            <img
              :src="fileShareApi.previewUrl(entry.node_id)"
              alt=""
              loading="lazy"
              @error="markThumbnailFailed(entry.node_id)"
            >
          </span>
          <span
            v-else-if="entry.is_dir"
            class="tile-glyph folder"
            aria-hidden="true"
          >
            <Icon name="folder" />
          </span>
          <span
            v-else
            class="tile-glyph"
            :style="{
              color: getFileGlyphStyle(entry.name).color,
              background: getFileGlyphStyle(entry.name).bg,
              borderColor: getFileGlyphStyle(entry.name).border,
            }"
            aria-hidden="true"
          >
            {{ getFileGlyphStyle(entry.name).label }}
          </span>

          <span class="tile-name">{{ entry.name }}</span>

          <span
            v-if="searchActive && entry.display_path !== entry.name"
            class="tile-hint"
            :class="{ cn: isHintCjk(entry.display_path) }"
          >{{ entry.display_path }}</span>

          <span class="tile-meta">
            <span>{{ entry.is_dir ? '-' : formatFileSize(entry.size) }}</span>
            <span>{{ relativeTime(entry.modified) }}</span>
          </span>

          <button
            v-if="canDownload(entry)"
            type="button"
            class="row-action primary"
            :title="t('table.download')"
            :aria-label="t('table.download')"
            @click.stop="emit('download', entry)"
          >
            <Icon name="download" />
          </button>
        </button>
      </div>
    </template>
  </div>
</template>
