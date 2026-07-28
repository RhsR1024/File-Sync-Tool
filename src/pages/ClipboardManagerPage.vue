<script setup lang="ts">
import { Clipboard } from 'lucide-vue-next';
import { useI18n } from 'vue-i18n';

import ClipboardSettingsPanel from '@/components/clipboard/ClipboardSettingsPanel.vue';

defineOptions({ name: 'ClipboardManagerPage' });

const { t } = useI18n();
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-gradient-to-b from-slate-50 to-white">
    <div class="mx-auto flex w-full max-w-5xl flex-col gap-6 px-6 py-6 pb-10">
      <header class="flex items-start gap-3">
        <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-rose-500 to-pink-600 shadow-sm">
          <Clipboard class="h-5 w-5 text-white" aria-hidden="true" />
        </div>
        <div>
          <h1 class="text-2xl font-bold text-slate-900">{{ t('clipboard.tool.title') }}</h1>
          <p class="mt-1 text-sm text-slate-500">{{ t('clipboard.tool.description') }}</p>
        </div>
      </header>

      <ClipboardSettingsPanel />
    </div>
  </div>
</template>
