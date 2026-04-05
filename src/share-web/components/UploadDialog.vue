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
  width: min(100%, 520px);
  border-radius: 24px;
  background: rgba(7, 14, 24, 0.96);
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

.dialog-body input {
  border: 1px dashed rgba(56, 189, 248, 0.4);
  border-radius: 18px;
  padding: 18px;
  background: rgba(8, 15, 24, 0.72);
  color: #eff7ff;
}

.upload-summary {
  color: #cde0f3;
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
  background: linear-gradient(135deg, #22c55e, #14b8a6);
  color: #04111b;
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
