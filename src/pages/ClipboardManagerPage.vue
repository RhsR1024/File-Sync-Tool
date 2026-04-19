<script setup lang="ts">
import { useI18n } from 'vue-i18n';

defineOptions({ name: 'ClipboardManagerPage' });

const { t } = useI18n();
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-gradient-to-b from-slate-50 to-white">
    <div class="mx-auto flex w-full max-w-6xl flex-col gap-6 px-6 py-6 pb-10">
      <header class="space-y-2">
        <h1 class="text-2xl font-bold tracking-tight text-slate-950">
          {{ t('clipboard.tool.title') }}
        </h1>
        <p class="text-sm text-slate-500">{{ t('clipboard.tool.description') }}</p>
      </header>
      <section class="rounded-2xl border border-slate-200 bg-white p-8 text-center text-sm text-slate-400">
        {{ t('clipboard.tool.placeholder') }}
      </section>
    </div>
  </div>
</template>
