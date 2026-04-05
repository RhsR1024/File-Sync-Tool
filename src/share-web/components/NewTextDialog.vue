<script setup lang="ts">
import { ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  open: boolean;
  busy?: boolean;
  error?: string;
}>();

const emit = defineEmits<{
  close: [];
  submit: [payload: { name: string; content: string }];
}>();

const fileName = ref('note.txt');
const content = ref('');
const { t } = useI18n();

watch(
  () => props.open,
  (open) => {
    if (open) {
      fileName.value = 'note.txt';
      content.value = '';
    }
  },
);

function handleSubmit() {
  emit('submit', {
    name: fileName.value.trim(),
    content: content.value,
  });
}
</script>

<template>
  <div v-if="open" class="dialog-mask">
    <div class="dialog-card">
      <div class="dialog-header">
        <h2>{{ t('newText.title') }}</h2>
        <p>{{ t('newText.description') }}</p>
      </div>

      <div class="dialog-body">
        <label class="field">
          <span>{{ t('newText.fileName') }}</span>
          <input v-model="fileName" type="text" :disabled="busy" />
        </label>
        <label class="field">
          <span>{{ t('newText.content') }}</span>
          <textarea v-model="content" rows="10" :disabled="busy"></textarea>
        </label>
        <p v-if="error" class="error-text">{{ error }}</p>
      </div>

      <div class="dialog-footer">
        <button type="button" class="ghost-button" :disabled="busy" @click="emit('close')">
          {{ t('newText.cancel') }}
        </button>
        <button type="button" class="primary-button" :disabled="busy || !fileName.trim()" @click="handleSubmit">
          {{ busy ? t('newText.submitting') : t('newText.submit') }}
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
  width: min(100%, 640px);
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

.dialog-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 8px;
  color: #d3e1ef;
}

.field input,
.field textarea {
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 16px;
  background: rgba(4, 9, 16, 0.72);
  color: #eff7ff;
  padding: 12px 14px;
  resize: vertical;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.primary-button,
.ghost-button {
  border: none;
  border-radius: 999px;
  padding: 10px 18px;
}

.primary-button {
  background: linear-gradient(135deg, #38bdf8, #14b8a6);
  color: #031018;
  font-weight: 700;
}

.ghost-button {
  background: rgba(148, 163, 184, 0.12);
  color: #dbe7f3;
}

.error-text {
  margin: 0;
  color: #fda4af;
}
</style>
