<script setup lang="ts">
import { ArrowUp } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';

defineOptions({ name: 'UpdateRedDot' });

const { t } = useI18n();
</script>

<template>
  <span
    class="inline-flex items-center gap-1 rounded-full border border-rose-400/40 bg-rose-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-rose-200"
    role="status"
    :aria-label="t('sidebar.updateAvailable')"
  >
    <span class="h-1.5 w-1.5 rounded-full bg-rose-400" aria-hidden="true"></span>
    <ArrowUp class="h-2.5 w-2.5" aria-hidden="true" />
  </span>
</template>
