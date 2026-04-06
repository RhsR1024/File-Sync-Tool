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
    <div class="scope-segment" role="group" :aria-label="t('search.scopeLabel')">
      <span class="scope-label">{{ t('search.scopeLabel') }}</span>
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
  gap: 10px;
}

.scope-segment {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  min-height: 42px;
  border-radius: 12px;
  border: 1px solid var(--fs-panel-border);
  background: var(--fs-surface);
  padding: 4px 8px 4px 10px;
}

.scope-label {
  color: var(--fs-muted);
  font-size: 12px;
  line-height: 1;
  white-space: nowrap;
}

.scope-toggle {
  display: inline-flex;
  gap: 4px;
  padding: 2px;
  border-radius: 10px;
  background: rgba(15, 23, 42, 0.78);
}

.scope-button {
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--fs-muted);
  padding: 6px 10px;
  font-size: 13px;
  line-height: 1.2;
}

.scope-button.active {
  border-color: color-mix(in srgb, var(--fs-accent) 30%, transparent);
  background: linear-gradient(
    135deg,
    color-mix(in srgb, var(--fs-accent-2) 22%, transparent),
    color-mix(in srgb, var(--fs-accent) 22%, transparent)
  );
  color: var(--fs-text);
}

.search-box {
  display: flex;
  flex: 1;
  min-width: min(100%, 360px);
  gap: 8px;
}

.search-box input {
  flex: 1;
  min-width: 180px;
  border: 1px solid var(--fs-panel-border);
  border-radius: 12px;
  background: var(--fs-surface-strong);
  color: var(--fs-text);
  padding: 10px 12px;
}

.search-button,
.clear-button {
  border: 1px solid transparent;
  border-radius: 12px;
  padding: 0 14px;
  white-space: nowrap;
}

.search-button {
  background: linear-gradient(135deg, var(--fs-accent-2), var(--fs-accent));
  color: #031018;
  font-weight: 700;
}

.clear-button {
  border-color: var(--fs-panel-border);
  background: var(--fs-surface);
  color: var(--fs-text);
}

.scope-button:disabled,
.search-button:disabled,
.clear-button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

@media (max-width: 860px) {
  .scope-segment {
    width: 100%;
    justify-content: space-between;
  }

  .search-box {
    width: 100%;
  }
}
</style>
