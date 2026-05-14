<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import type { FileShareBreadcrumb } from '../types';

import { Icon } from './icons';

const props = defineProps<{
  breadcrumbs: FileShareBreadcrumb[];
  busy?: boolean;
}>();

const emit = defineEmits<{
  navigate: [nodeId: string | null];
}>();

const { t } = useI18n();

function isLast(index: number): boolean {
  return index === props.breadcrumbs.length - 1;
}
</script>

<template>
  <nav class="crumbs" :aria-label="t('toolbar.breadcrumbsLabel')">
    <template v-for="(crumb, index) in breadcrumbs" :key="crumb.node_id ?? `__home__-${index}`">
      <span v-if="index > 0" class="sep" aria-hidden="true">/</span>
      <span v-if="isLast(index)" class="last">
        <template v-if="index === 0">
          <Icon name="home" />
        </template>
        {{ crumb.label }}
      </span>
      <button
        v-else
        type="button"
        :disabled="busy"
        @click="emit('navigate', crumb.node_id)"
      >
        <template v-if="index === 0">
          <Icon name="home" />
        </template>
        {{ crumb.label }}
      </button>
    </template>
  </nav>
</template>

<style scoped>
.crumbs :deep(svg) {
  width: 13px;
  height: 13px;
  margin-right: 4px;
  vertical-align: -1px;
}
</style>
