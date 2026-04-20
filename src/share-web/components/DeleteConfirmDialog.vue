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
  background: rgba(26, 37, 53, 0.45);
  backdrop-filter: blur(6px);
}

.dialog-card {
  width: min(100%, 460px);
  border-radius: 20px;
  background: var(--fs-panel, rgba(255, 255, 255, 0.96));
  border: 1px solid var(--fs-panel-border, rgba(99, 119, 150, 0.22));
  box-shadow: 0 24px 60px -20px rgba(15, 23, 42, 0.25);
  color: var(--fs-text, #1a2535);
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

.dialog-header h2 {
  font-size: 18px;
  font-weight: 700;
  color: var(--fs-text, #1a2535);
}

.dialog-header p {
  margin-top: 8px;
  color: var(--fs-muted, #5a7194);
  font-size: 13px;
}

.target-name {
  margin: 0;
  font-size: 15px;
  font-weight: 700;
  color: var(--fs-danger, #dc2626);
  word-break: break-all;
  background: rgba(220, 38, 38, 0.08);
  border: 1px solid rgba(220, 38, 38, 0.2);
  border-radius: 10px;
  padding: 10px 14px;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  border-top: 1px solid var(--fs-panel-border, rgba(99, 119, 150, 0.18));
}

.danger-button,
.ghost-button {
  border: none;
  border-radius: 999px;
  padding: 10px 20px;
  font-size: 13px;
  transition: transform 0.12s ease, box-shadow 0.12s ease, background 0.12s ease;
}

.danger-button:disabled,
.ghost-button:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.danger-button {
  background: var(--fs-danger, #dc2626);
  color: #ffffff;
  font-weight: 700;
  box-shadow: 0 6px 16px -6px rgba(220, 38, 38, 0.55);
}

.danger-button:not(:disabled):hover {
  background: #b91c1c;
  box-shadow: 0 8px 20px -6px rgba(185, 28, 28, 0.6);
}

.ghost-button {
  background: var(--fs-surface, rgba(241, 245, 250, 0.9));
  color: var(--fs-text, #1a2535);
  border: 1px solid var(--fs-panel-border, rgba(99, 119, 150, 0.2));
  font-weight: 600;
}

.ghost-button:not(:disabled):hover {
  background: var(--fs-surface-strong, rgba(226, 232, 242, 0.96));
}

.error-text {
  margin: 14px 0 0;
  color: var(--fs-danger, #dc2626);
  font-size: 13px;
}
</style>
