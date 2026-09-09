<script setup lang="ts">
import SyncConfigurationEditor from '@/components/sync/SyncConfigurationEditor.vue';
import DeploymentTopologyPanel from '@/components/sync/DeploymentTopologyPanel.vue';

defineOptions({ name: 'SyncDeliveryPage' });
</script>

<template>
  <div class="h-full min-h-0 overflow-y-auto overscroll-y-none">
    <SyncConfigurationEditor section="delivery" />
    <DeploymentTopologyPanel />
  </div>
</template>
