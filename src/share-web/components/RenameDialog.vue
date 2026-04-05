<script setup lang="ts">
import { ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  open: boolean;
  busy?: boolean;
  currentName: string;
  error?: string;
}>();

const emit = defineEmits<{
  close: [];
  submit: [name: string];
}>();

const nextName = ref('');
const { t } = useI18n();

watch(
  () => props.open,
  (open) => {
    if (open) {
      nextName.value = props.currentName;
    }
  },
);

function handleSubmit() {
  emit('submit', nextName.value.trim());
}
</script>

<template>
  <div v-if="open" class="dialog-mask">
    <div class="dialog-card">
      <div class="dialog-header">
        <h2>{{ t('rename.title') }}</h2>
        <p>{{ t('rename.description') }}</p>
      </div>
      <div class="dialog-body">
        <input v-model="nextName" type="text" :disabled="busy" @keyup.enter="handleSubmit" />
        <p v-if="error" class="error-text">{{ error }}</p>
      </div>
      <div class="dialog-footer">
        <button type="button" class="ghost-button" :disabled="busy" @click="emit('close')">
          {{ t('rename.cancel') }}
        </button>
        <button type="button" class="primary-button" :disabled="busy || !nextName.trim()" @click="handleSubmit">
          {{ busy ? t('rename.submitting') : t('rename.submit') }}
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
  width: min(100%, 440px);
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

.dialog-body input {
  width: 100%;
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 16px;
  background: rgba(4, 9, 16, 0.72);
  color: #eff7ff;
  padding: 12px 14px;
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
  margin: 14px 0 0;
  color: #fda4af;
}
</style>
