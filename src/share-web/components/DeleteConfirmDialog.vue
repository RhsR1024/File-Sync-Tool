<script setup lang="ts">
import { useI18n } from 'vue-i18n';

defineProps<{
  open: boolean;
  busy?: boolean;
  targetName: string;
  error?: string;
}>();

const emit = defineEmits<{
  close: [];
  submit: [];
}>();

const { t } = useI18n();
</script>

<template>
  <div v-if="open" class="dialog-mask">
    <div class="dialog-card">
      <div class="dialog-header">
        <h2>{{ t('deleteConfirm.title') }}</h2>
        <p>{{ t('deleteConfirm.description') }}</p>
      </div>
      <div class="dialog-body">
        <p class="target-name">{{ targetName }}</p>
        <p v-if="error" class="error-text">{{ error }}</p>
      </div>
      <div class="dialog-footer">
        <button type="button" class="ghost-button" :disabled="busy" @click="emit('close')">
          {{ t('deleteConfirm.cancel') }}
        </button>
        <button type="button" class="danger-button" :disabled="busy" @click="emit('submit')">
          {{ busy ? t('deleteConfirm.submitting') : t('deleteConfirm.submit') }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dialog-mask {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(3, 8, 15, 0.72);
  backdrop-filter: blur(10px);
}

.dialog-card {
  width: min(100%, 460px);
  border-radius: 24px;
  background: rgba(7, 14, 24, 0.97);
  border: 1px solid rgba(148, 163, 184, 0.18);
}

.dialog-header,
.dialog-body,
.dialog-footer {
  padding: 22px 24px;
}

.dialog-header h2,
.dialog-header p {
  margin: 0;
}

.dialog-header p {
  margin-top: 8px;
  color: #95abc0;
}

.target-name {
  margin: 0;
  font-size: 18px;
  font-weight: 700;
  color: #fff5f5;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.danger-button,
.ghost-button {
  border: none;
  border-radius: 999px;
  padding: 10px 18px;
}

.danger-button {
  background: linear-gradient(135deg, #fb7185, #ef4444);
  color: white;
  font-weight: 700;
}

.ghost-button {
  background: rgba(148, 163, 184, 0.12);
  color: #dbe7f3;
}

.error-text {
  margin: 14px 0 0;
  color: #fda4af;
}
</style>
