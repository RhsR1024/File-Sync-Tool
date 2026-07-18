<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import ClipboardSettingsPanel from '@/components/clipboard/ClipboardSettingsPanel.vue';
import ImageToClipboardCard from '@/components/clipboard/ImageToClipboardCard.vue';

defineOptions({ name: 'ClipboardManagerPage' });

const { t } = useI18n();
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-gradient-to-b from-slate-50 to-white">
    <div class="mx-auto flex w-full max-w-5xl flex-col gap-6 px-6 py-6 pb-10">
      <header class="space-y-2">
        <h1 class="text-2xl font-bold tracking-tight text-slate-950">
          {{ t('clipboard.tool.title') }}
        </h1>
        <p class="text-sm text-slate-500">{{ t('clipboard.tool.description') }}</p>
      </header>

      <ImageToClipboardCard />
      <ClipboardSettingsPanel />
    </div>
  </div>
</template>
