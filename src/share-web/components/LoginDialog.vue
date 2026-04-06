<script setup lang="ts">
import { ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  open: boolean;
  busy?: boolean;
  error?: string;
  description?: string;
}>();

const emit = defineEmits<{
  close: [];
  submit: [payload: { username: string; password: string }];
}>();

const username = ref('');
const password = ref('');
const { t } = useI18n();

watch(
  () => props.open,
  (open) => {
    if (!open) {
      password.value = '';
    }
  },
);

function handleSubmit() {
  emit('submit', {
    username: username.value.trim(),
    password: password.value,
  });
}
</script>

<template>
  <div v-if="open" class="dialog-mask">
    <div class="dialog-card">
      <div class="dialog-header">
        <div>
          <h2>{{ t('login.title') }}</h2>
          <p>{{ description || t('login.description') }}</p>
        </div>
      </div>

      <div class="dialog-body">
        <label class="field">
          <span>{{ t('login.username') }}</span>
          <input v-model="username" type="text" :placeholder="t('login.usernamePlaceholder')" :disabled="busy" />
        </label>
        <label class="field">
          <span>{{ t('login.password') }}</span>
          <input v-model="password" type="password" :placeholder="t('login.passwordPlaceholder')" :disabled="busy" @keyup.enter="handleSubmit" />
        </label>
        <p v-if="error" class="error-text">{{ error }}</p>
      </div>

      <div class="dialog-footer">
        <button type="button" class="ghost-button" :disabled="busy" @click="emit('close')">
          {{ t('login.close') }}
        </button>
        <button type="button" class="primary-button" :disabled="busy || !username.trim() || !password" @click="handleSubmit">
          {{ busy ? t('login.submitting') : t('login.submit') }}
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
  width: min(100%, 420px);
  border-radius: 24px;
  background: linear-gradient(180deg, rgba(9, 18, 30, 0.98), rgba(9, 17, 28, 0.96));
  border: 1px solid rgba(148, 163, 184, 0.18);
  box-shadow: 0 28px 60px rgba(0, 0, 0, 0.42);
}

.dialog-header,
.dialog-body,
.dialog-footer {
  padding: 22px 24px;
}

.dialog-header h2 {
  margin: 0 0 8px;
}

.dialog-header p {
  margin: 0;
  color: #95abc0;
}

.dialog-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 8px;
  color: #c5d7e8;
}

.field input {
  border: 1px solid rgba(148, 163, 184, 0.2);
  border-radius: 14px;
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
  margin: 0;
  color: #fda4af;
}
</style>
