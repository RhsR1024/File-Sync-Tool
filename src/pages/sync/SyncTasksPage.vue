<script setup lang="ts">
import SyncConfigurationEditor from '@/components/sync/SyncConfigurationEditor.vue';

defineOptions({ name: 'SyncTasksPage' });
</script>

<template>
  <SyncConfigurationEditor section="tasks-strategy" />
</template>
