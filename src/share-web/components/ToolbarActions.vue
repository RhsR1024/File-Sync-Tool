<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';

import type {
  FileSharePermissionSet,
  FileShareTreeCurrentKind,
} from '../types';

import { Icon } from './icons';

const props = defineProps<{
  currentKind: FileShareTreeCurrentKind | null;
  permissions: FileSharePermissionSet | null;
  hasEntries: boolean;
  busy?: boolean;
}>();

const emit = defineEmits<{
  'upload-files': [];
  'upload-directory': [];
  'create-directory': [];
  'create-text': [];
  'download-all': [];
}>();

const { t } = useI18n();

const showWriteActions = computed(() => props.currentKind !== 'home');

const canUploadFiles = computed(() => showWriteActions.value && Boolean(props.permissions?.upload_file));
const canUploadDirectory = computed(() => showWriteActions.value && Boolean(props.permissions?.upload_directory));
const canCreateDirectory = computed(() => showWriteActions.value && Boolean(props.permissions?.create_directory));
const canCreateText = computed(() => showWriteActions.value && Boolean(props.permissions?.create_text));

const showDownloadAll = computed(() => (
  showWriteActions.value
  && props.hasEntries
  && !canUploadFiles.value
  && !canUploadDirectory.value
  && !canCreateDirectory.value
  && !canCreateText.value
  && Boolean(props.permissions?.download_archive)
));
</script>

<template>
  <div class="page-actions">
    <button
      v-if="canUploadFiles"
      type="button"
      class="btn"
      :disabled="busy"
      @click="emit('upload-files')"
    >
      <Icon name="upload" />
      <span>{{ t('toolbar.uploadFiles') }}</span>
    </button>
    <button
      v-if="canUploadDirectory"
      type="button"
      class="btn"
      :disabled="busy"
      @click="emit('upload-directory')"
    >
      <Icon name="upload" />
      <span>{{ t('toolbar.uploadDirectory') }}</span>
    </button>
    <button
      v-if="canCreateDirectory"
      type="button"
      class="btn"
      :disabled="busy"
      @click="emit('create-directory')"
    >
      <Icon name="newfolder" />
      <span>{{ t('toolbar.createDirectory') }}</span>
    </button>
    <button
      v-if="canCreateText"
      type="button"
      class="btn"
      :disabled="busy"
      @click="emit('create-text')"
    >
      <Icon name="text" />
      <span>{{ t('toolbar.createText') }}</span>
    </button>
    <button
      v-if="showDownloadAll"
      type="button"
      class="btn primary"
      :disabled="busy"
      @click="emit('download-all')"
    >
      <Icon name="download" />
      <span>{{ t('toolbar.downloadAll') }}</span>
    </button>
  </div>
</template>
