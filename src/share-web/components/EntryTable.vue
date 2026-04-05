<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import {
  formatFileSize,
  isImageEntry,
  type FileShareDisplayEntry,
  type FileSharePermissionSet,
} from '../types';

const props = defineProps<{
  entries: FileShareDisplayEntry[];
  permissions: FileSharePermissionSet | null;
  loading?: boolean;
  emptyText?: string;
  globalSearch?: boolean;
}>();

const emit = defineEmits<{
  open: [entry: FileShareDisplayEntry];
  preview: [entry: FileShareDisplayEntry];
  download: [entry: FileShareDisplayEntry];
  archive: [entry: FileShareDisplayEntry];
  rename: [entry: FileShareDisplayEntry];
  delete: [entry: FileShareDisplayEntry];
}>();

const { t } = useI18n();

function canPreview(entry: FileShareDisplayEntry): boolean {
  return Boolean(props.permissions?.preview_image)
    && !entry.is_dir
    && isImageEntry(entry.name);
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
        :key="`${entry.root_alias}:${entry.relative_path}`"
        class="entry-row"
      >
        <div class="entry-name">
          <button type="button" class="name-button" @click="emit('open', entry)">
            <span class="entry-icon">{{ entry.is_dir ? '📁' : '📄' }}</span>
            <span>{{ entry.name }}</span>
          </button>
          <div v-if="globalSearch" class="entry-hint">
            {{ entry.root_alias }} / {{ entry.relative_path }}
          </div>
        </div>

        <span>{{ entry.is_dir ? '-' : formatFileSize(entry.size) }}</span>
        <span>{{ entry.modified || '-' }}</span>

        <div class="entry-actions">
          <button
            v-if="entry.is_dir && permissions?.download_archive"
            type="button"
            class="action-button"
            @click="emit('archive', entry)"
          >
            ZIP
          </button>
          <button
            v-if="!entry.is_dir && permissions?.download_file"
            type="button"
            class="action-button"
            @click="emit('download', entry)"
          >
            {{ t('table.download') }}
          </button>
          <button
            v-if="canPreview(entry)"
            type="button"
            class="action-button"
            @click="emit('preview', entry)"
          >
            {{ t('table.preview') }}
          </button>
          <button
            v-if="permissions?.rename"
            type="button"
            class="action-button"
            @click="emit('rename', entry)"
          >
            {{ t('table.rename') }}
          </button>
          <button
            v-if="permissions?.delete"
            type="button"
            class="action-button danger"
            @click="emit('delete', entry)"
          >
            {{ t('table.delete') }}
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
  gap: 8px;
}

.entry-row {
  display: grid;
  grid-template-columns: minmax(0, 1.8fr) 110px 150px minmax(220px, auto);
  align-items: center;
  gap: 14px;
  padding: 14px 18px;
  border-radius: 18px;
  background: rgba(7, 13, 21, 0.58);
  border: 1px solid rgba(148, 163, 184, 0.12);
}

.entry-head {
  background: rgba(255, 255, 255, 0.04);
  color: #88a0b8;
  font-size: 12px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.entry-name {
  min-width: 0;
}

.name-button {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  border: none;
  padding: 0;
  background: transparent;
  color: #eff7ff;
  text-align: left;
}

.entry-icon {
  width: 24px;
  text-align: center;
}

.entry-hint {
  margin-top: 4px;
  color: #7e92a7;
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.entry-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
}

.action-button {
  border: none;
  border-radius: 999px;
  padding: 8px 12px;
  background: rgba(148, 163, 184, 0.1);
  color: #d7e6f5;
}

.action-button.danger {
  background: rgba(239, 68, 68, 0.12);
  color: #fecaca;
}

.entry-empty {
  padding: 56px 24px;
  text-align: center;
  color: #91a7bc;
  border-radius: 20px;
  background: rgba(7, 13, 21, 0.5);
  border: 1px dashed rgba(148, 163, 184, 0.2);
}

@media (max-width: 880px) {
  .entry-head {
    display: none;
  }

  .entry-row {
    grid-template-columns: 1fr;
    justify-items: start;
  }

  .entry-actions {
    justify-content: flex-start;
  }
}
</style>
