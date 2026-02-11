<script setup lang="ts">
import Sidebar from '@/components/Sidebar.vue';
import { RouterView } from 'vue-router';
</script>

<template>
  <div class="flex h-screen bg-slate-50 font-sans text-slate-900">
    <Sidebar />
    <main class="flex-1 overflow-auto">
      <router-view v-slot="{ Component }">
        <keep-alive include="MainConsole">
          <component :is="Component" />
        </keep-alive>
      </router-view>
    </main>
  </div>
</template>
