<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import { Icon } from './icons';

defineProps<{
  count: number;
  canDownload: boolean;
  canDelete: boolean;
  busy?: boolean;
}>();

const emit = defineEmits<{
  'download-all': [];
  'delete-all': [];
  clear: [];
}>();

const { t } = useI18n();
</script>

<template>
  <div class="bulkbar" role="dialog" aria-live="polite">
    <div class="count">
      <span class="pill">{{ count }}</span>
      <span>{{ t('app.selectedCount', { count }) }}</span>
    </div>
    <div class="divider" aria-hidden="true" />
    <button
      v-if="canDownload"
      type="button"
      class="primary"
      :disabled="busy"
      @click="emit('download-all')"
    >
      <Icon name="download" />
      <span>{{ t('app.bulkDownload') }}</span>
    </button>
    <button
      v-if="canDelete"
      type="button"
      class="danger"
      :disabled="busy"
      @click="emit('delete-all')"
    >
      <Icon name="trash" />
      <span>{{ t('app.bulkDelete') }}</span>
    </button>
    <div class="divider" aria-hidden="true" />
    <button type="button" :disabled="busy" @click="emit('clear')">
      <Icon name="close" />
      <span>{{ t('app.bulkClear') }}</span>
    </button>
  </div>
</template>
