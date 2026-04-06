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
      <div class="path-strip" role="navigation" :aria-label="t('toolbar.breadcrumbsLabel')">
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
      </div>

      <div class="action-cluster">
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
  gap: 10px;
}

.toolbar-row {
  display: flex;
  flex-wrap: wrap;
  align-items: stretch;
  justify-content: space-between;
  gap: 10px;
}

.path-strip {
  flex: 1 1 380px;
  min-height: 44px;
  display: flex;
  align-items: center;
  border-radius: 14px;
  border: 1px solid var(--fs-panel-border);
  background: var(--fs-surface);
  padding: 0 12px;
}

.breadcrumbs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 7px;
  min-width: 0;
}

.action-cluster {
  flex: 1 1 420px;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}

.session-group {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.primary-button,
.ghost-button,
.session-chip {
  border: 1px solid transparent;
  border-radius: 999px;
  padding: 9px 14px;
  transition: transform 0.18s ease, background-color 0.18s ease, opacity 0.18s ease, border-color 0.18s ease;
  white-space: nowrap;
}

.primary-button {
  background: linear-gradient(135deg, var(--fs-accent-2), var(--fs-accent));
  color: #031018;
  font-weight: 700;
}

.ghost-button {
  border-color: var(--fs-panel-border);
  background: var(--fs-surface);
  color: var(--fs-text);
}

.session-chip {
  border-color: var(--fs-panel-border);
  background: var(--fs-surface-strong);
  color: var(--fs-text);
}

.session-chip.guest {
  border-color: rgba(34, 197, 94, 0.3);
  background: rgba(34, 197, 94, 0.14);
}

.crumb {
  border: none;
  padding: 0;
  background: transparent;
  color: var(--fs-muted);
  font-size: 14px;
  line-height: 1.2;
  white-space: nowrap;
  max-width: 280px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.crumb.current {
  color: var(--fs-text);
  font-weight: 700;
}

.crumb-separator {
  color: color-mix(in srgb, var(--fs-muted) 70%, transparent);
}

.primary-button:disabled,
.ghost-button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.primary-button:not(:disabled):hover,
.ghost-button:not(:disabled):hover {
  transform: translateY(-1px);
  border-color: color-mix(in srgb, var(--fs-accent) 36%, var(--fs-panel-border));
}

.crumb:not(:disabled):hover {
  color: color-mix(in srgb, var(--fs-text) 90%, var(--fs-muted));
  text-decoration: underline;
}

.crumb:disabled {
  cursor: default;
}

.hint-banner {
  margin: 0;
  border-radius: 14px;
  border: 1px solid rgba(56, 189, 248, 0.2);
  padding: 12px 14px;
  background: rgba(56, 189, 248, 0.1);
  color: #c6e6ff;
}

@media (max-width: 900px) {
  .path-strip,
  .action-cluster {
    flex-basis: 100%;
  }
}
</style>
