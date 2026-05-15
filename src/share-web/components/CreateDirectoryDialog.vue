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
  submit: [name: string];
}>();

const { t } = useI18n();
const directoryName = ref('');

watch(
  () => props.open,
  (open) => {
    if (open) {
      directoryName.value = '';
    }
  },
);

function handleSubmit() {
  emit('submit', directoryName.value.trim());
}
</script>

<template>
  <div v-if="open" class="dialog-mask">
    <div class="dialog-card">
      <div class="dialog-header">
        <h2>{{ t('createDirectory.title') }}</h2>
        <p>{{ t('createDirectory.description') }}</p>
      </div>
      <div class="dialog-body">
        <label class="field">
          <span>{{ t('createDirectory.name') }}</span>
          <input
            v-model="directoryName"
            type="text"
            :placeholder="t('createDirectory.placeholder')"
            :disabled="busy"
            @keyup.enter="handleSubmit"
          />
        </label>
        <p v-if="error" class="error-text">{{ error }}</p>
      </div>
      <div class="dialog-footer">
        <button type="button" class="ghost-button" :disabled="busy" @click="emit('close')">
          {{ t('createDirectory.cancel') }}
        </button>
        <button type="button" class="primary-button" :disabled="busy || !directoryName.trim()" @click="handleSubmit">
          {{ busy ? t('createDirectory.submitting') : t('createDirectory.submit') }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dialog-card {
  width: min(100%, 440px);
}
</style>
