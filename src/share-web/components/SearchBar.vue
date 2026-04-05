<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import type { FileShareSearchScope } from '../types';

defineProps<{
  keyword: string;
  scope: FileShareSearchScope;
  canSearchCurrent: boolean;
  canSearchGlobal: boolean;
  busy?: boolean;
}>();

const emit = defineEmits<{
  'update:keyword': [value: string];
  'update:scope': [value: FileShareSearchScope];
  search: [];
  clear: [];
}>();

const { t } = useI18n();
</script>

<template>
  <div class="search-shell">
    <div class="scope-toggle">
      <button
        v-if="canSearchCurrent"
        type="button"
        class="scope-button"
        :class="{ active: scope === 'current' }"
        @click="emit('update:scope', 'current')"
      >
        {{ t('search.current') }}
      </button>
      <button
        v-if="canSearchGlobal"
        type="button"
        class="scope-button"
        :class="{ active: scope === 'global' }"
        @click="emit('update:scope', 'global')"
      >
        {{ t('search.global') }}
      </button>
    </div>

    <div class="search-box">
      <input
        :value="keyword"
        type="search"
        :placeholder="t('search.placeholder')"
        :disabled="busy"
        @input="emit('update:keyword', ($event.target as HTMLInputElement).value)"
        @keyup.enter="emit('search')"
      />
      <button type="button" class="search-button" :disabled="busy" @click="emit('search')">
        {{ t('search.submit') }}
      </button>
      <button type="button" class="clear-button" :disabled="busy" @click="emit('clear')">
        {{ t('search.clear') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.search-shell {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
}

.scope-toggle {
  display: inline-flex;
  gap: 8px;
}

.scope-button {
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.08);
  color: #b8cbe0;
  padding: 8px 14px;
}

.scope-button.active {
  border-color: rgba(34, 211, 238, 0.42);
  background: rgba(34, 211, 238, 0.14);
  color: #eff9ff;
}

.search-box {
  display: flex;
  flex: 1;
  min-width: min(100%, 320px);
  gap: 8px;
}

.search-box input {
  flex: 1;
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 14px;
  background: rgba(8, 15, 24, 0.86);
  color: #eff7ff;
  padding: 12px 14px;
}

.search-button,
.clear-button {
  border: none;
  border-radius: 14px;
  padding: 0 16px;
}

.search-button {
  background: linear-gradient(135deg, #38bdf8, #14b8a6);
  color: #031018;
  font-weight: 700;
}

.clear-button {
  background: rgba(148, 163, 184, 0.12);
  color: #d3e1ef;
}
</style>
