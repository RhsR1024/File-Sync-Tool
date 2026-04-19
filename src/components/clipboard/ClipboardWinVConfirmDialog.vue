<script setup lang="ts">
import { ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

const props = defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  confirm: [];
  cancel: [];
}>();

const { t } = useI18n();
const agreed = ref(false);

watch(
  () => props.open,
  (v) => {
    if (!v) agreed.value = false;
  },
);

function onConfirm() {
  if (!agreed.value) return;
  emit('confirm');
}

function onCancel() {
  agreed.value = false;
  emit('cancel');
}
</script>

<template>
  <div
    v-if="props.open"
    class="fixed inset-0 z-[60] flex items-center justify-center bg-slate-950/40 backdrop-blur-sm"
    @click.self="onCancel"
  >
    <div class="w-full max-w-md rounded-2xl bg-white p-6 shadow-2xl">
      <div class="mb-3 flex items-center gap-2">
        <span class="text-lg">⚠️</span>
        <h3 class="text-base font-semibold text-orange-600">
          {{ t('clipboard.settings.winVConfirmTitle') }}
        </h3>
      </div>
      <p class="mb-2 text-sm text-slate-700">{{ t('clipboard.settings.winVConfirmBody') }}</p>
      <ol class="mb-4 list-inside list-decimal space-y-1 text-xs text-slate-600">
        <li>{{ t('clipboard.settings.winVStep1') }}</li>
        <li>{{ t('clipboard.settings.winVStep2') }}</li>
        <li>{{ t('clipboard.settings.winVStep3') }}</li>
      </ol>
      <label class="mb-5 flex items-start gap-2 text-xs text-slate-700">
        <input type="checkbox" v-model="agreed" class="mt-0.5 h-4 w-4" />
        <span>{{ t('clipboard.settings.winVConfirmAgreeCheckbox') }}</span>
      </label>
      <div class="flex justify-end gap-2">
        <button
          type="button"
          class="rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-600 hover:bg-slate-50"
          @click="onCancel"
        >
          {{ t('clipboard.settings.winVCancel') }}
        </button>
        <button
          type="button"
          class="rounded-lg px-3 py-1.5 text-xs font-medium text-white transition-colors"
          :class="agreed ? 'bg-orange-600 hover:bg-orange-700' : 'bg-slate-300'"
          :disabled="!agreed"
          @click="onConfirm"
        >
          {{ t('clipboard.settings.winVContinue') }}
        </button>
      </div>
    </div>
  </div>
</template>
