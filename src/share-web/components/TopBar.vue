<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';

import type { FileShareSession } from '../types';

import { Icon } from './icons';

const props = defineProps<{
  session: FileShareSession | null;
  busy?: boolean;
}>();

const emit = defineEmits<{
  refresh: [];
  'session-action': [];
}>();

const { t } = useI18n();

const userInitial = computed(() => {
  const name = props.session?.username ?? '';
  return name ? name.slice(0, 1).toUpperCase() : '?';
});

const sessionActionLabel = computed(() => {
  if (!props.session) {
    return t('app.switchAccount');
  }
  return props.session.is_guest ? t('app.switchAccount') : t('app.signOut');
});
</script>

<template>
  <header class="topbar">
    <div class="brand">
      <div class="brand-mark" aria-hidden="true">FS</div>
      <div>
        <div class="brand-name">{{ t('app.pageTitle') }}</div>
      </div>
    </div>

    <div class="topbar-spacer" />

    <button
      type="button"
      class="btn ghost"
      :disabled="busy"
      :title="t('toolbar.refresh')"
      @click="emit('refresh')"
    >
      <Icon name="refresh" />
      <span>{{ t('toolbar.refresh') }}</span>
    </button>

    <div v-if="session" class="user-chip">
      <div class="avatar">{{ userInitial }}</div>
      <div class="who">
        <div class="name">{{ session.username }}</div>
      </div>
    </div>

    <button
      type="button"
      class="btn"
      :disabled="busy"
      :title="sessionActionLabel"
      @click="emit('session-action')"
    >
      <Icon name="switch" />
      <span>{{ sessionActionLabel }}</span>
    </button>
  </header>
</template>
