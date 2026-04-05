<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import type { FileSharePermissionSet, FileShareRootSummary } from '../types';

interface BreadcrumbItem {
  label: string;
  path: string;
}

const props = defineProps<{
  roots: FileShareRootSummary[];
  currentRoot: string;
  breadcrumbs: BreadcrumbItem[];
  permissions: FileSharePermissionSet | null;
  busy?: boolean;
}>();

const emit = defineEmits<{
  'select-root': [root: string];
  navigate: [path: string];
  'upload-files': [];
  'upload-directory': [];
  'create-directory': [];
  'create-text': [];
  refresh: [];
}>();

const { t } = useI18n();

function canUploadFiles() {
  return Boolean(props.permissions?.upload_file);
}

function canUploadDirectory() {
  return Boolean(props.permissions?.upload_directory);
}

function canCreateDirectory() {
  return Boolean(props.permissions?.create_directory);
}

function canCreateText() {
  return Boolean(props.permissions?.create_text);
}
</script>

<template>
  <div class="toolbar">
    <div class="toolbar-top">
      <label class="toolbar-select">
        <span>{{ t('toolbar.sharedRoot') }}</span>
        <select
          :value="currentRoot"
          :disabled="busy || roots.length === 0"
          @change="emit('select-root', ($event.target as HTMLSelectElement).value)"
        >
          <option
            v-for="root in roots"
            :key="root.alias"
            :value="root.alias"
          >
            {{ root.alias }}
          </option>
        </select>
      </label>

      <div class="toolbar-actions">
        <button type="button" class="ghost-button" :disabled="busy" @click="emit('refresh')">
          {{ t('toolbar.refresh') }}
        </button>
        <button
          v-if="canUploadFiles()"
          type="button"
          class="primary-button"
          :disabled="busy"
          @click="emit('upload-files')"
        >
          {{ t('toolbar.uploadFiles') }}
        </button>
        <button
          v-if="canUploadDirectory()"
          type="button"
          class="ghost-button"
          :disabled="busy"
          @click="emit('upload-directory')"
        >
          {{ t('toolbar.uploadDirectory') }}
        </button>
        <button
          v-if="canCreateDirectory()"
          type="button"
          class="ghost-button"
          :disabled="busy"
          @click="emit('create-directory')"
        >
          {{ t('toolbar.createDirectory') }}
        </button>
        <button
          v-if="canCreateText()"
          type="button"
          class="ghost-button"
          :disabled="busy"
          @click="emit('create-text')"
        >
          {{ t('toolbar.createText') }}
        </button>
      </div>
    </div>

    <div class="breadcrumbs">
      <button
        v-for="crumb in breadcrumbs"
        :key="crumb.path || '__root__'"
        type="button"
        class="crumb"
        @click="emit('navigate', crumb.path)"
      >
        {{ crumb.label }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.toolbar {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.toolbar-top {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.toolbar-select {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 220px;
  color: #9ab0c6;
  font-size: 13px;
}

.toolbar-select select {
  border: 1px solid rgba(154, 176, 198, 0.22);
  border-radius: 12px;
  background: rgba(11, 20, 32, 0.88);
  color: #eff7ff;
  padding: 10px 14px;
}

.toolbar-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.primary-button,
.ghost-button,
.crumb {
  border: none;
  border-radius: 999px;
  padding: 10px 16px;
  transition: transform 0.18s ease, background-color 0.18s ease, opacity 0.18s ease;
}

.primary-button {
  background: linear-gradient(135deg, #14b8a6, #22c55e);
  color: #04111b;
  font-weight: 700;
}

.ghost-button {
  background: rgba(148, 163, 184, 0.12);
  color: #e7f0fa;
}

.primary-button:disabled,
.ghost-button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.primary-button:not(:disabled):hover,
.ghost-button:not(:disabled):hover,
.crumb:hover {
  transform: translateY(-1px);
}

.breadcrumbs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.crumb {
  background: rgba(255, 255, 255, 0.06);
  color: #c7d8ea;
  padding: 8px 12px;
}
</style>
