<script setup lang="ts">
import { useI18n } from 'vue-i18n';

defineProps<{
  open: boolean;
  title: string;
  src: string;
}>();

const emit = defineEmits<{
  close: [];
}>();

const { t } = useI18n();
</script>

<template>
  <div v-if="open" class="preview-mask" @click.self="emit('close')">
    <div class="preview-card">
      <div class="preview-top">
        <h2>{{ title }}</h2>
        <button type="button" class="close-button" @click="emit('close')">{{ t('preview.close') }}</button>
      </div>
      <div class="preview-body">
        <img :src="src" :alt="title" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.preview-card {
  width: min(100%, 1080px);
  max-height: 100%;
}

.preview-top {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  padding: 20px 24px;
  border-bottom: 1px solid var(--border);
  background: linear-gradient(180deg, var(--surface) 0%, var(--surface-2) 100%);
}

.preview-top h2 {
  min-width: 0;
  margin: 0;
  overflow: hidden;
  color: var(--text);
  font-size: 18px;
  font-weight: 700;
  letter-spacing: 0;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.preview-body {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  overflow: auto;
  max-height: calc(100vh - 120px);
  background: var(--surface-2);
}

.preview-body img {
  max-width: 100%;
  max-height: calc(100vh - 160px);
  object-fit: contain;
  border: 1px solid var(--border);
  border-radius: var(--r-md);
  background: var(--surface);
}
</style>
