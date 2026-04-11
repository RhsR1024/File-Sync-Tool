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
  background: rgba(180, 195, 215, 0.45);
  backdrop-filter: blur(10px);
}

.dialog-card {
  width: min(100%, 420px);
  border-radius: 24px;
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.98), rgba(248, 251, 255, 0.97));
  border: 1px solid rgba(99, 119, 150, 0.2);
  box-shadow: 0 20px 56px rgba(0, 0, 0, 0.14);
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
  color: var(--fs-muted);
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
  color: var(--fs-text);
}

.field input {
  border: 1px solid rgba(99, 119, 150, 0.22);
  border-radius: 14px;
  background: rgba(241, 245, 250, 0.9);
  color: var(--fs-text);
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
  background: linear-gradient(135deg, #0284c7, #0b9e90);
  color: #fff;
  font-weight: 700;
}

.ghost-button {
  background: rgba(99, 119, 150, 0.1);
  color: var(--fs-text);
}

.error-text {
  margin: 0;
  color: #b91c1c;
}
</style>
