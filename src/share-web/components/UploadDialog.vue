<script setup lang="ts">
import { ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  open: boolean;
  mode: 'files' | 'directory';
  busy?: boolean;
  error?: string;
}>();

const emit = defineEmits<{
  close: [];
  submit: [files: File[]];
}>();

const selectedFiles = ref<File[]>([]);
const { t } = useI18n();

watch(
  () => props.open,
  (open) => {
    if (!open) {
      selectedFiles.value = [];
    }
  },
);

function handleSelect(event: Event) {
  const target = event.target as HTMLInputElement;
  selectedFiles.value = Array.from(target.files ?? []);
}

function handleSubmit() {
  emit('submit', selectedFiles.value);
}
</script>

<template>
  <div v-if="open" class="dialog-mask">
    <div class="dialog-card">
      <div class="dialog-header">
        <h2>{{ mode === 'files' ? t('upload.fileTitle') : t('upload.directoryTitle') }}</h2>
        <p>
          {{ mode === 'files' ? t('upload.fileDescription') : t('upload.directoryDescription') }}
        </p>
      </div>

      <div class="dialog-body">
        <input
          v-if="mode === 'files'"
          type="file"
          multiple
          :disabled="busy"
          @change="handleSelect"
        />
        <input
          v-else
          type="file"
          multiple
          webkitdirectory
          directory
          :disabled="busy"
          @change="handleSelect"
        />

        <div class="upload-summary">
          {{ t('upload.summary', { count: selectedFiles.length }) }}
        </div>
        <p v-if="error" class="error-text">{{ error }}</p>
      </div>

      <div class="dialog-footer">
        <button type="button" class="ghost-button" :disabled="busy" @click="emit('close')">
          {{ t('upload.cancel') }}
        </button>
        <button type="button" class="primary-button" :disabled="busy || selectedFiles.length === 0" @click="handleSubmit">
          {{ busy ? t('upload.submitting') : t('upload.submit') }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dialog-card {
  width: min(100%, 520px);
}
</style>
