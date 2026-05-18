<script setup lang="ts">
import { useI18n } from 'vue-i18n';

defineOptions({ name: 'UpdateRedDot' });

const { t } = useI18n();
</script>

<template>
  <span
    class="inline-flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-rose-300"
    role="status"
    :aria-label="t('sidebar.updateAvailable')"
  >
    <span class="h-1.5 w-1.5 rounded-full bg-rose-400 shadow-[0_0_0_3px_rgba(251,113,133,0.16)]" aria-hidden="true"></span>
    <span aria-hidden="true">NEW</span>
  </span>
</template>
