<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import type {
  FileShareNode,
  FileShareTreeCurrentKind,
} from '../types';
import type { RecentPathEntry } from '../lib/recent-paths';

import { Icon } from './icons';

const props = defineProps<{
  shareRoots: FileShareNode[];
  currentKind: FileShareTreeCurrentKind | null;
  activeRootNodeId: string | null;
  recent: RecentPathEntry[];
  busy?: boolean;
}>();

const emit = defineEmits<{
  navigate: [nodeId: string | null];
}>();

const { t } = useI18n();

function isRootActive(node: FileShareNode): boolean {
  return props.activeRootNodeId === node.node_id;
}
</script>

<template>
  <aside class="sidebar">
    <div class="side-section">
      <button
        type="button"
        class="side-item"
        :class="{ active: currentKind === 'home' }"
        :disabled="busy"
        @click="emit('navigate', null)"
      >
        <span class="ico"><Icon name="home" /></span>
        <span class="label">{{ t('app.sidebarHome') }}</span>
      </button>
    </div>

    <div v-if="shareRoots.length > 0" class="side-section">
      <div class="side-title">{{ t('app.sidebarShared') }}</div>
      <button
        v-for="root in shareRoots"
        :key="root.node_id"
        type="button"
        class="side-item"
        :class="{ active: isRootActive(root) }"
        :disabled="busy"
        @click="emit('navigate', root.node_id)"
      >
        <span class="ico"><Icon name="folder" /></span>
        <span class="label">{{ root.name }}</span>
      </button>
    </div>

    <div v-if="recent.length > 0" class="side-section">
      <div class="side-title">{{ t('app.sidebarRecent') }}</div>
      <button
        v-for="entry in recent"
        :key="entry.node_id"
        type="button"
        class="side-item"
        :disabled="busy"
        @click="emit('navigate', entry.node_id)"
      >
        <span class="ico"><Icon name="clock" /></span>
        <span class="label" :title="entry.label">{{ entry.label }}</span>
      </button>
    </div>
  </aside>
</template>
