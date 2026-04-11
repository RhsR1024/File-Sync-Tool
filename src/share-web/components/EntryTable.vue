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

interface FileExtStyle {
  label: string;
  color: string;
  bg: string;
}

const extStyleMap: Record<string, { color: string; bg: string }> = {
  exe:  { color: '#9f1239', bg: 'rgba(159,18,57,0.12)' },
  msi:  { color: '#9f1239', bg: 'rgba(159,18,57,0.12)' },
  bat:  { color: '#9f1239', bg: 'rgba(159,18,57,0.12)' },
  sh:   { color: '#9f1239', bg: 'rgba(159,18,57,0.12)' },
  pdf:  { color: '#b91c1c', bg: 'rgba(185,28,28,0.12)' },
  doc:  { color: '#1d4ed8', bg: 'rgba(29,78,216,0.12)' },
  docx: { color: '#1d4ed8', bg: 'rgba(29,78,216,0.12)' },
  xls:  { color: '#15803d', bg: 'rgba(21,128,61,0.12)' },
  xlsx: { color: '#15803d', bg: 'rgba(21,128,61,0.12)' },
  csv:  { color: '#15803d', bg: 'rgba(21,128,61,0.12)' },
  ppt:  { color: '#c2410c', bg: 'rgba(194,65,12,0.12)' },
  pptx: { color: '#c2410c', bg: 'rgba(194,65,12,0.12)' },
  zip:  { color: '#7c3aed', bg: 'rgba(124,58,237,0.12)' },
  rar:  { color: '#7c3aed', bg: 'rgba(124,58,237,0.12)' },
  '7z': { color: '#7c3aed', bg: 'rgba(124,58,237,0.12)' },
  gz:   { color: '#7c3aed', bg: 'rgba(124,58,237,0.12)' },
  tar:  { color: '#7c3aed', bg: 'rgba(124,58,237,0.12)' },
  bz2:  { color: '#7c3aed', bg: 'rgba(124,58,237,0.12)' },
  xz:   { color: '#7c3aed', bg: 'rgba(124,58,237,0.12)' },
  txt:  { color: '#475569', bg: 'rgba(71,85,105,0.10)' },
  log:  { color: '#475569', bg: 'rgba(71,85,105,0.10)' },
  md:   { color: '#475569', bg: 'rgba(71,85,105,0.10)' },
  json: { color: '#0d9488', bg: 'rgba(13,148,136,0.12)' },
  xml:  { color: '#0d9488', bg: 'rgba(13,148,136,0.12)' },
  yml:  { color: '#0d9488', bg: 'rgba(13,148,136,0.12)' },
  yaml: { color: '#0d9488', bg: 'rgba(13,148,136,0.12)' },
  toml: { color: '#0d9488', bg: 'rgba(13,148,136,0.12)' },
  ini:  { color: '#0d9488', bg: 'rgba(13,148,136,0.12)' },
  conf: { color: '#0d9488', bg: 'rgba(13,148,136,0.12)' },
  html: { color: '#ea580c', bg: 'rgba(234,88,12,0.12)' },
  css:  { color: '#2563eb', bg: 'rgba(37,99,235,0.12)' },
  js:   { color: '#ca8a04', bg: 'rgba(202,138,4,0.12)' },
  ts:   { color: '#2563eb', bg: 'rgba(37,99,235,0.12)' },
  vue:  { color: '#059669', bg: 'rgba(5,150,105,0.12)' },
  py:   { color: '#2563eb', bg: 'rgba(37,99,235,0.12)' },
  rs:   { color: '#c2410c', bg: 'rgba(194,65,12,0.12)' },
  go:   { color: '#0891b2', bg: 'rgba(8,145,178,0.12)' },
  java: { color: '#b91c1c', bg: 'rgba(185,28,28,0.12)' },
  jar:  { color: '#b91c1c', bg: 'rgba(185,28,28,0.12)' },
  png:  { color: '#0284c7', bg: 'rgba(2,132,199,0.12)' },
  jpg:  { color: '#0284c7', bg: 'rgba(2,132,199,0.12)' },
  jpeg: { color: '#0284c7', bg: 'rgba(2,132,199,0.12)' },
  gif:  { color: '#0284c7', bg: 'rgba(2,132,199,0.12)' },
  svg:  { color: '#0284c7', bg: 'rgba(2,132,199,0.12)' },
  webp: { color: '#0284c7', bg: 'rgba(2,132,199,0.12)' },
  ico:  { color: '#0284c7', bg: 'rgba(2,132,199,0.12)' },
  mp4:  { color: '#7c3aed', bg: 'rgba(124,58,237,0.12)' },
  mkv:  { color: '#7c3aed', bg: 'rgba(124,58,237,0.12)' },
  avi:  { color: '#7c3aed', bg: 'rgba(124,58,237,0.12)' },
  mov:  { color: '#7c3aed', bg: 'rgba(124,58,237,0.12)' },
  mp3:  { color: '#e11d48', bg: 'rgba(225,29,72,0.12)' },
  wav:  { color: '#e11d48', bg: 'rgba(225,29,72,0.12)' },
  flac: { color: '#e11d48', bg: 'rgba(225,29,72,0.12)' },
  iso:  { color: '#64748b', bg: 'rgba(100,116,139,0.12)' },
  dmg:  { color: '#64748b', bg: 'rgba(100,116,139,0.12)' },
  sql:  { color: '#0d9488', bg: 'rgba(13,148,136,0.12)' },
};

function getFileExtStyle(name: string): FileExtStyle {
  const lower = name.toLowerCase();
  // Handle compound extensions like .tar.gz, .tar.bz2, .tar.xz
  const compoundMatch = lower.match(/\.(tar\.(?:gz|bz2|xz))$/);
  if (compoundMatch) {
    const ext = compoundMatch[1].replace('.', '');
    const style = extStyleMap[ext] ?? extStyleMap['tar'];
    return { label: 'TGZ', color: style!.color, bg: style!.bg };
  }

  const dotIndex = lower.lastIndexOf('.');
  if (dotIndex < 0 || dotIndex === lower.length - 1) {
    return { label: 'F', color: '#64748b', bg: 'rgba(100,116,139,0.10)' };
  }

  const ext = lower.slice(dotIndex + 1);
  const style = extStyleMap[ext];
  const label = ext.length <= 4 ? ext.toUpperCase() : ext.slice(0, 3).toUpperCase();

  if (style) {
    return { label, color: style.color, bg: style.bg };
  }
  return { label, color: '#64748b', bg: 'rgba(100,116,139,0.10)' };
}

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
              v-else-if="entry.is_dir"
              class="entry-visual entry-visual--icon folder"
              aria-hidden="true"
            >
              <svg viewBox="0 0 24 24">
                <path
                  d="M3.5 6.5h6l2 2H20a1.5 1.5 0 0 1 1.5 1.5v7.5A2 2 0 0 1 19.5 19h-15A2 2 0 0 1 2.5 17V8.5a2 2 0 0 1 2-2Z"
                  fill="currentColor"
                />
              </svg>
            </span>
            <span
              v-else
              class="entry-visual entry-visual--ext"
              :style="{ color: getFileExtStyle(entry.name).color, background: getFileExtStyle(entry.name).bg }"
              aria-hidden="true"
            >
              {{ getFileExtStyle(entry.name).label }}
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
  background: rgba(255, 255, 255, 0.72);
  border: 1px solid rgba(99, 119, 150, 0.14);
}

.entry-head {
  padding: 10px 16px;
  background: rgba(0, 0, 0, 0.03);
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
  background: rgba(99, 119, 150, 0.1);
}

.entry-visual--icon.folder {
  color: #d97706;
}

.entry-visual--ext {
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.02em;
  line-height: 1;
  user-select: none;
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
  background: rgba(241, 245, 249, 0.9);
  color: var(--fs-text);
  transition: border-color 0.18s ease, opacity 0.18s ease, transform 0.18s ease;
}

.icon-button svg {
  width: 18px;
  height: 18px;
}

.icon-button.download {
  border-color: rgba(21, 128, 61, 0.2);
  background: rgba(21, 128, 61, 0.08);
  color: #14532d;
}

.icon-button.preview {
  border-color: rgba(2, 132, 199, 0.2);
  background: rgba(2, 132, 199, 0.08);
  color: #0c4a6e;
}

.icon-button.rename {
  border-color: rgba(161, 98, 7, 0.2);
  background: rgba(161, 98, 7, 0.08);
  color: #78350f;
}

.icon-button.delete {
  border-color: rgba(185, 28, 28, 0.2);
  background: rgba(185, 28, 28, 0.08);
  color: #7f1d1d;
}

.icon-button:hover {
  opacity: 0.9;
  transform: translateY(-1px);
}

.entry-empty {
  padding: 56px 24px;
  text-align: center;
  color: var(--fs-muted);
  border-radius: 20px;
  background: rgba(248, 250, 252, 0.8);
  border: 1px dashed rgba(99, 119, 150, 0.24);
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
