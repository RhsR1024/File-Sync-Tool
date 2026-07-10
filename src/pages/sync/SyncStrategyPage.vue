<script setup lang="ts">
import SyncConfigurationEditor from '@/components/sync/SyncConfigurationEditor.vue';

defineOptions({ name: 'SyncStrategyPage' });
</script>

<template>
  <SyncConfigurationEditor section="strategy" />
</template>
