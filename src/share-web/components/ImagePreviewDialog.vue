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
.preview-mask {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(3, 8, 15, 0.82);
  backdrop-filter: blur(8px);
}

.preview-card {
  width: min(100%, 1080px);
  max-height: 100%;
  border-radius: 24px;
  background: rgba(6, 12, 21, 0.96);
  border: 1px solid rgba(148, 163, 184, 0.18);
  overflow: hidden;
}

.preview-top {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  padding: 20px 24px;
}

.preview-top h2 {
  margin: 0;
}

.preview-body {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  overflow: auto;
  max-height: calc(100vh - 120px);
}

.preview-body img {
  max-width: 100%;
  max-height: calc(100vh - 160px);
  object-fit: contain;
  border-radius: 18px;
}

.close-button {
  border: none;
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.12);
  color: #dbe7f3;
  padding: 10px 18px;
}
</style>
