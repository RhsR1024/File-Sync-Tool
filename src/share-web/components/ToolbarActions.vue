<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';

import type {
  FileShareBreadcrumb,
  FileSharePermissionSet,
  FileShareTreeCurrentKind,
} from '../types';

const props = defineProps<{
  breadcrumbs: FileShareBreadcrumb[];
  currentKind: FileShareTreeCurrentKind | null;
  permissions: FileSharePermissionSet | null;
  sessionText?: string;
  sessionIsGuest?: boolean;
  sessionActionLabel?: string;
  browseOnlyHint?: string;
  busy?: boolean;
}>();

const emit = defineEmits<{
  navigate: [nodeId: string | null];
  'upload-files': [];
  'upload-directory': [];
  'create-directory': [];
  'create-text': [];
  refresh: [];
  'session-action': [];
}>();

const { t } = useI18n();

const showWriteActions = computed(() => props.currentKind !== 'home');

function canUploadFiles() {
  return showWriteActions.value && Boolean(props.permissions?.upload_file);
}

function canUploadDirectory() {
  return showWriteActions.value && Boolean(props.permissions?.upload_directory);
}

function canCreateDirectory() {
  return showWriteActions.value && Boolean(props.permissions?.create_directory);
}

function canCreateText() {
  return showWriteActions.value && Boolean(props.permissions?.create_text);
}

function isCurrentCrumb(index: number): boolean {
  return index === props.breadcrumbs.length - 1;
}
</script>

<template>
  <div class="toolbar">
    <div class="toolbar-row">
      <div class="breadcrumbs">
        <template v-for="(crumb, index) in breadcrumbs" :key="crumb.node_id ?? `__home__-${index}`">
          <button
            type="button"
            class="crumb"
            :class="{ current: isCurrentCrumb(index) }"
            :disabled="busy || isCurrentCrumb(index)"
            @click="emit('navigate', crumb.node_id)"
          >
            {{ crumb.label }}
          </button>
          <span v-if="index < breadcrumbs.length - 1" class="crumb-separator" aria-hidden="true">/</span>
        </template>
      </div>

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
        <div v-if="sessionText" class="session-group">
          <div class="session-chip" :class="{ guest: sessionIsGuest }">
            {{ sessionText }}
          </div>
          <button
            v-if="sessionActionLabel"
            type="button"
            class="ghost-button"
            :disabled="busy"
            @click="emit('session-action')"
          >
            {{ sessionActionLabel }}
          </button>
        </div>
      </div>
    </div>

    <p v-if="browseOnlyHint" class="hint-banner">
      {{ browseOnlyHint }}
    </p>
  </div>
</template>

<style scoped>
.toolbar {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.toolbar-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.breadcrumbs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.toolbar-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}

.session-group {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.primary-button,
.ghost-button,
.session-chip {
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

.session-chip {
  background: rgba(148, 163, 184, 0.12);
  color: #eff7ff;
}

.session-chip.guest {
  background: rgba(34, 197, 94, 0.16);
}

.crumb {
  border: none;
  padding: 0;
  background: transparent;
  color: #90adc9;
  font-size: 14px;
  white-space: nowrap;
}

.crumb.current {
  color: #eff7ff;
  font-weight: 700;
}

.crumb-separator {
  color: #4d6b88;
}

.primary-button:disabled,
.ghost-button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.primary-button:not(:disabled):hover,
.ghost-button:not(:disabled):hover {
  transform: translateY(-1px);
}

.crumb:not(:disabled):hover {
  color: #d7e7f8;
  text-decoration: underline;
}

.crumb:disabled {
  cursor: default;
}

.hint-banner {
  margin: 0;
  border-radius: 18px;
  padding: 12px 14px;
  background: rgba(59, 130, 246, 0.12);
  color: #c6e6ff;
}
</style>
