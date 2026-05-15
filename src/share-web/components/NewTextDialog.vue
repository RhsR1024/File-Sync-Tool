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
.dialog-card {
  width: min(100%, 640px);
}

.field textarea {
  min-height: 220px;
}
</style>
