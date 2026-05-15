<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import type { FileShareSearchScope } from '../types';
import type { EntryViewMode } from '../lib/view-mode';

import { Icon } from './icons';

defineProps<{
  keyword: string;
  scope: FileShareSearchScope;
  view: EntryViewMode;
  canSearchCurrent: boolean;
  canSearchGlobal: boolean;
  busy?: boolean;
}>();

const emit = defineEmits<{
  'update:keyword': [value: string];
  'update:scope': [value: FileShareSearchScope];
  'update:view': [value: EntryViewMode];
  search: [];
  clear: [];
}>();

const { t } = useI18n();

function handleInput(event: Event) {
  const target = event.target as HTMLInputElement;
  emit('update:keyword', target.value);
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter') {
    emit('search');
  } else if (event.key === 'Escape') {
    emit('clear');
  }
}
</script>

<template>
  <div class="toolbar" :class="{ 'has-search': keyword.trim().length > 0 }">
    <label class="search">
      <Icon name="search" />
      <input
        type="search"
        :value="keyword"
        :placeholder="t('search.placeholder')"
        :disabled="busy"
        :aria-label="t('search.placeholder')"
        @input="handleInput"
        @keydown="handleKeydown"
      />
      <kbd>⌘K</kbd>
    </label>

    <div
      v-if="keyword.trim().length > 0"
      class="scope-toggle"
      role="tablist"
      :aria-label="t('search.scopeLabel')"
    >
      <button
        type="button"
        :class="{ active: scope === 'current' }"
        :disabled="!canSearchCurrent || busy"
        :title="canSearchCurrent ? t('search.current') : t('search.scopeCurrentUnavailable')"
        @click="emit('update:scope', 'current'); emit('search')"
      >
        {{ t('search.current') }}
      </button>
      <button
        type="button"
        :class="{ active: scope === 'global' }"
        :disabled="!canSearchGlobal || busy"
        @click="emit('update:scope', 'global'); emit('search')"
      >
        {{ t('search.global') }}
      </button>
    </div>

    <div class="view-toggle" role="tablist" :aria-label="t('app.viewLabel')">
      <button
        type="button"
        :class="{ active: view === 'list' }"
        :title="t('app.viewList')"
        :aria-label="t('app.viewList')"
        @click="emit('update:view', 'list')"
      >
        <Icon name="list" />
      </button>
      <button
        type="button"
        :class="{ active: view === 'grid' }"
        :title="t('app.viewGrid')"
        :aria-label="t('app.viewGrid')"
        @click="emit('update:view', 'grid')"
      >
        <Icon name="grid" />
      </button>
    </div>
  </div>
</template>
