<script setup lang="ts">
defineOptions({ name: 'ClipboardPanelPage' });
</script>

<template>
  <div class="flex h-screen w-screen flex-col overflow-hidden rounded-2xl bg-white/85 shadow-2xl backdrop-blur-xl">
    <header class="flex items-center justify-between border-b border-slate-200/60 px-4 py-3">
      <span class="text-sm font-semibold text-slate-700">Clipboard</span>
    </header>
    <div class="flex-1 overflow-hidden p-4 text-sm text-slate-400">
      Panel placeholder (M3)
    </div>
  </div>
</template>
