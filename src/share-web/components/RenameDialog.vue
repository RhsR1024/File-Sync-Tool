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
.dialog-card {
  width: min(100%, 440px);
}

.error-text {
  margin: 14px 0 0;
}
</style>
