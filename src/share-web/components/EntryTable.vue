<script setup lang="ts">
import { ref } from 'vue';
import { useI18n } from 'vue-i18n';

import { fileShareApi } from '../api';
import {
  canPreviewEntry,
  canRenderEntryThumbnail,
  formatFileSize,
  type FileShareNode,
  type FileShareSession,
} from '../types';

const props = defineProps<{
  entries: FileShareNode[];
  session?: FileShareSession | null;
  loading?: boolean;
  emptyText?: string;
  searchActive?: boolean;
}>();

const emit = defineEmits<{
  open: [entry: FileShareNode];
  preview: [entry: FileShareNode];
  download: [entry: FileShareNode];
  rename: [entry: FileShareNode];
  delete: [entry: FileShareNode];
}>();

const { t } = useI18n();
const failedThumbnails = ref<Record<string, boolean>>({});

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
</script>

<template>
  <div class="entry-table">
    <div class="entry-head entry-row">
      <span>{{ t('table.name') }}</span>
      <span>{{ t('table.size') }}</span>
      <span>{{ t('table.modified') }}</span>
      <span>{{ t('table.actions') }}</span>
    </div>

    <div v-if="loading" class="entry-empty">
      {{ t('table.loading') }}
    </div>

    <div v-else-if="entries.length === 0" class="entry-empty">
      {{ emptyText || t('table.empty') }}
    </div>

    <div v-else class="entry-body">
      <div
        v-for="entry in entries"
        :key="entry.node_id"
        class="entry-row"
      >
        <div class="entry-name">
          <button type="button" class="name-button" @click="emit('open', entry)">
            <span
              v-if="canShowThumbnail(entry)"
              class="entry-visual entry-visual--thumb"
            >
              <img
                :src="fileShareApi.previewUrl(entry.node_id)"
                alt=""
                class="entry-thumb"
                loading="lazy"
                @error="markThumbnailFailed(entry.node_id)"
              >
            </span>
            <span
              v-else
              class="entry-visual entry-visual--icon"
              :class="{ folder: entry.is_dir }"
              aria-hidden="true"
            >
              <svg v-if="entry.is_dir" viewBox="0 0 24 24">
                <path
                  d="M3.5 6.5h6l2 2H20a1.5 1.5 0 0 1 1.5 1.5v7.5A2 2 0 0 1 19.5 19h-15A2 2 0 0 1 2.5 17V8.5a2 2 0 0 1 2-2Z"
                  fill="currentColor"
                />
              </svg>
              <svg v-else viewBox="0 0 24 24">
                <path
                  d="M7 3.5h7.5L19.5 8v12A1.5 1.5 0 0 1 18 21.5H7A2.5 2.5 0 0 1 4.5 19V6A2.5 2.5 0 0 1 7 3.5Z"
                  fill="currentColor"
                />
              </svg>
            </span>
            <span class="entry-copy">
              <span class="name-text">{{ entry.name }}</span>
            </span>
          </button>
          <span
            v-if="searchActive && entry.display_path !== entry.name"
            class="entry-hint"
          >
            {{ entry.display_path }}
          </span>
        </div>

        <span>{{ entry.is_dir ? '-' : formatFileSize(entry.size) }}</span>
        <span>{{ entry.modified || '-' }}</span>

        <div class="entry-actions">
          <button
            v-if="canDownload(entry)"
            type="button"
            class="icon-button download"
            :aria-label="t('table.download')"
            :title="t('table.download')"
            @click="emit('download', entry)"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path
                d="M12 4.5v9m0 0 3.5-3.5M12 13.5 8.5 10M6 16.5v1A2.5 2.5 0 0 0 8.5 20h7A2.5 2.5 0 0 0 18 17.5v-1"
                fill="none"
                stroke="currentColor"
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
              />
            </svg>
            <span class="sr-only">{{ t('table.download') }}</span>
          </button>
          <button
            v-if="canPreview(entry)"
            type="button"
            class="icon-button preview"
            :aria-label="t('table.preview')"
            :title="t('table.preview')"
            @click="emit('preview', entry)"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path
                d="M2.5 12s3.5-5.5 9.5-5.5S21.5 12 21.5 12 18 17.5 12 17.5 2.5 12 2.5 12Z"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
              />
              <circle cx="12" cy="12" r="3" fill="currentColor" />
            </svg>
            <span class="sr-only">{{ t('table.preview') }}</span>
          </button>
          <button
            v-if="entry.permissions.rename"
            type="button"
            class="icon-button rename"
            :aria-label="t('table.rename')"
            :title="t('table.rename')"
            @click="emit('rename', entry)"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path
                d="m4.5 16.5 8.8-8.8 4 4-8.8 8.8-4.5.5.5-4.5Zm10.2-10.2 1.6-1.6a1.5 1.5 0 0 1 2.1 0l1 1a1.5 1.5 0 0 1 0 2.1l-1.6 1.6"
                fill="none"
                stroke="currentColor"
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
              />
            </svg>
            <span class="sr-only">{{ t('table.rename') }}</span>
          </button>
          <button
            v-if="entry.permissions.delete"
            type="button"
            class="icon-button delete"
            :aria-label="t('table.delete')"
            :title="t('table.delete')"
            @click="emit('delete', entry)"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path
                d="M5.5 7.5h13m-10 0v10m4-10v10M9.5 4.5h5l1 2h-7l1-2Zm-1 3h7l-.7 11a2 2 0 0 1-2 1.9h-1.6a2 2 0 0 1-2-1.9l-.7-11Z"
                fill="none"
                stroke="currentColor"
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
              />
            </svg>
            <span class="sr-only">{{ t('table.delete') }}</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.entry-table {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.entry-row {
  display: grid;
  grid-template-columns: minmax(0, 2fr) 96px 144px minmax(112px, auto);
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  border-radius: 16px;
  background: rgba(8, 14, 24, 0.62);
  border: 1px solid rgba(148, 163, 184, 0.12);
}

.entry-head {
  padding: 10px 16px;
  background: rgba(255, 255, 255, 0.035);
  color: var(--fs-muted);
  font-size: 12px;
}

.entry-name {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.name-button {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  min-width: 0;
  border: none;
  padding: 0;
  background: transparent;
  color: var(--fs-text);
  text-align: left;
}

.entry-visual {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  flex-shrink: 0;
  border-radius: 12px;
  background: rgba(148, 163, 184, 0.08);
}

.entry-visual--icon {
  color: #eef4fb;
}

.entry-visual--icon.folder {
  color: #f7c85b;
}

.entry-thumb {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  object-fit: cover;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(15, 23, 42, 0.3);
}

.entry-copy {
  min-width: 0;
}

.name-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.entry-hint {
  padding-left: 48px;
  color: var(--fs-muted);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.entry-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 6px;
}

.icon-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border-radius: 10px;
  border: 1px solid transparent;
  background: rgba(15, 23, 42, 0.5);
  color: var(--fs-text);
  transition: border-color 0.18s ease, opacity 0.18s ease, transform 0.18s ease;
}

.icon-button svg {
  width: 18px;
  height: 18px;
}

.icon-button.download {
  border-color: rgba(74, 222, 128, 0.18);
  background: rgba(34, 197, 94, 0.12);
  color: #dcfce7;
}

.icon-button.preview {
  border-color: rgba(56, 189, 248, 0.18);
  background: rgba(56, 189, 248, 0.12);
  color: #dbeafe;
}

.icon-button.rename {
  border-color: rgba(250, 204, 21, 0.18);
  background: rgba(245, 158, 11, 0.12);
  color: #fef3c7;
}

.icon-button.delete {
  border-color: rgba(248, 113, 113, 0.18);
  background: rgba(239, 68, 68, 0.12);
  color: #fee2e2;
}

.icon-button:hover {
  opacity: 0.9;
  transform: translateY(-1px);
}

.entry-empty {
  padding: 56px 24px;
  text-align: center;
  color: #91a7bc;
  border-radius: 20px;
  background: rgba(7, 13, 21, 0.5);
  border: 1px dashed rgba(148, 163, 184, 0.2);
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}

@media (max-width: 880px) {
  .entry-head {
    display: none;
  }

  .entry-row {
    grid-template-columns: 1fr;
    justify-items: start;
    gap: 10px;
  }

  .entry-actions {
    justify-content: flex-start;
  }
}
</style>
