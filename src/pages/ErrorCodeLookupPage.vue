<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { ChevronLeft, ChevronRight, FileSearch, RefreshCw, Search } from 'lucide-vue-next';

import LoadingSkeleton from '@/components/LoadingSkeleton.vue';
import { pushToast } from '@/composables/useToast';
import {
  errorCodeApi,
  type ErrorCodeEntry,
  type ErrorCodeMetaInfo,
  type ErrorCodeMode,
} from '@/lib/tauri';
import { addLog } from '@/lib/store';
import {
  parseKeyword,
  parseRange,
  parseSingle,
} from '@/pages/errorCodeLookup/validation';

type StatusBanner = { type: 'success' | 'error'; message: string };

type LastQuery = {
  mode: ErrorCodeMode;
  value: string;
  start?: number;
  end?: number;
  code?: number;
  keyword?: string;
};

defineOptions({ name: 'ErrorCodeLookupPage' });

const { t, locale } = useI18n();

const mode = ref<ErrorCodeMode>('single');
const inputValue = ref('');
const inputError = ref<string | null>(null);
const submitting = ref(false);
const syncing = ref(false);
const meta = ref<ErrorCodeMetaInfo>({
  has_cache: false,
  last_synced_at: null,
  file_count: 0,
  row_count: 0,
});
const entries = ref<ErrorCodeEntry[]>([]);
const total = ref(0);
const currentPage = ref(1);
const pageSize = 50;
const expandedKey = ref<string | null>(null);
const lastQuery = ref<LastQuery | null>(null);
const noResultMessage = ref<string | null>(null);
const statusBanner = ref<StatusBanner | null>(null);
const jumpInput = ref('');

const totalPages = computed(() =>
  total.value === 0 ? 0 : Math.ceil(total.value / pageSize),
);

const placeholder = computed(() => t(`errorCodeLookup.placeholders.${mode.value}`));

const lastSyncedDisplay = computed(() => {
  if (!meta.value.last_synced_at) {
    return t('errorCodeLookup.neverSynced');
  }
  return t('errorCodeLookup.lastSyncedAt', {
    time: formatTime(meta.value.last_synced_at),
  });
});

const lastSyncedTooltip = computed(() =>
  t('errorCodeLookup.lastSyncedTooltip', {
    files: meta.value.file_count,
    rows: meta.value.row_count,
  }),
);

watch(mode, () => {
  inputValue.value = '';
  inputError.value = null;
  entries.value = [];
  total.value = 0;
  currentPage.value = 1;
  expandedKey.value = null;
  lastQuery.value = null;
  noResultMessage.value = null;
  jumpInput.value = '';
});

function formatTime(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return iso;
  }
  return date.toLocaleString(locale.value === 'zh' ? 'zh-CN' : 'en-US');
}

function rowKey(entry: ErrorCodeEntry, index: number): string {
  return `${entry.source_file}:${entry.code}:${index}`;
}

function toggleExpand(entry: ErrorCodeEntry, index: number) {
  const key = rowKey(entry, index);
  expandedKey.value = expandedKey.value === key ? null : key;
}

function onRowKeydown(event: KeyboardEvent, entry: ErrorCodeEntry, index: number) {
  // Mirror button semantics for the role="button" row wrapper. Space scrolls
  // by default — prevent that and toggle the detail panel instead.
  if (event.key === 'Enter' || event.key === ' ' || event.key === 'Spacebar') {
    event.preventDefault();
    toggleExpand(entry, index);
  }
}

function camel(snake: string): string {
  return snake.replace(/_([a-z])/g, (_, char: string) => char.toUpperCase());
}

function mapBackendQueryError(raw: string): string {
  const knownKeys = new Set([
    'invalid_single',
    'invalid_range_format',
    'range_reversed',
    'range_too_large',
    'invalid_keyword',
  ]);
  if (knownKeys.has(raw)) {
    return t(`errorCodeLookup.errors.${camel(raw)}`);
  }
  return raw;
}

function computeNoResultMessage(query: LastQuery, count: number): string | null {
  if (count > 0) {
    return null;
  }
  if (query.mode === 'single') {
    return t('errorCodeLookup.empty.singleNotFound', { code: query.code });
  }
  if (query.mode === 'range') {
    return t('errorCodeLookup.empty.rangeNoResult');
  }
  if ((query.keyword ?? '').length === 0) {
    return null;
  }
  return t('errorCodeLookup.empty.keywordNoResult', { keyword: query.keyword });
}

async function loadMeta() {
  try {
    meta.value = await errorCodeApi.getMeta();
  } catch (error) {
    addLog(`[error_code] 获取元数据失败：${String(error)}`, 'error');
  }
}

async function fetchPage(query: LastQuery, page: number) {
  submitting.value = true;
  inputError.value = null;
  expandedKey.value = null;

  try {
    const result = await errorCodeApi.query({
      mode: query.mode,
      value: query.value,
      page,
    });

    entries.value = result.entries;
    total.value = result.total;
    currentPage.value = result.page;
    lastQuery.value = query;
    noResultMessage.value = computeNoResultMessage(query, result.total);
  } catch (error) {
    inputError.value = mapBackendQueryError(String(error));
  } finally {
    submitting.value = false;
  }
}

async function runDefaultPreview() {
  await fetchPage({ mode: 'keyword', value: '', keyword: '' }, 1);
}

async function onSearch() {
  let prepared: LastQuery | null = null;

  if (mode.value === 'single') {
    const result = parseSingle(inputValue.value);
    if (result.ok) {
      prepared = { mode: 'single', value: String(result.code), code: result.code };
    } else {
      const errorKey = 'error' in result ? result.error : 'invalid_single';
      inputError.value = t(`errorCodeLookup.errors.${camel(errorKey)}`);
      return;
    }
  } else if (mode.value === 'range') {
    const result = parseRange(inputValue.value);
    if (result.ok) {
      prepared = {
        mode: 'range',
        value: `${result.start}-${result.end}`,
        start: result.start,
        end: result.end,
      };
    } else {
      const errorKey = 'error' in result ? result.error : 'invalid_range_format';
      inputError.value = t(`errorCodeLookup.errors.${camel(errorKey)}`);
      return;
    }
  } else {
    const result = parseKeyword(inputValue.value);
    if (result.ok) {
      prepared = { mode: 'keyword', value: result.keyword, keyword: result.keyword };
    } else {
      const errorKey = 'error' in result ? result.error : 'invalid_keyword';
      inputError.value = t(`errorCodeLookup.errors.${camel(errorKey)}`);
      return;
    }
  }

  await fetchPage(prepared, 1);
}

async function changePage(delta: number) {
  if (!lastQuery.value) {
    return;
  }

  const nextPage = currentPage.value + delta;
  if (nextPage < 1 || (totalPages.value > 0 && nextPage > totalPages.value)) {
    return;
  }

  await fetchPage(lastQuery.value, nextPage);
}

async function onJump() {
  if (!lastQuery.value) {
    return;
  }

  const trimmed = jumpInput.value.trim();
  if (!/^\d+$/.test(trimmed)) {
    jumpInput.value = '';
    return;
  }

  const target = Math.min(Math.max(Number(trimmed), 1), Math.max(totalPages.value, 1));
  jumpInput.value = '';
  await fetchPage(lastQuery.value, target);
}

async function onSync() {
  syncing.value = true;
  // The inline status banner is reserved for page-level state (e.g., a future
  // persistent service-down notice). Transient sync feedback uses toasts so it
  // does not duplicate with any page-level message that may appear here.
  statusBanner.value = null;

  try {
    const report = await errorCodeApi.sync();
    const successMessage = t('errorCodeLookup.toast.syncSuccess', {
      files: report.file_count,
      rows: report.row_count,
    });
    addLog(`[error_code] ${successMessage}`, 'success');
    pushToast(successMessage, 'success');
    await loadMeta();
    await runDefaultPreview();
  } catch (error) {
    const raw = String(error);
    const [keyPart, detail = ''] = raw.split('|');
    const toastKey = keyPart.startsWith('errorCodeLookup.')
      ? keyPart
      : 'errorCodeLookup.toast.archiveError';
    const statusMatch = detail.match(/http_(\d+)/);
    const message = t(toastKey, { status: statusMatch?.[1] ?? '' });
    addLog(`[error_code] 同步失败：${message} (${detail})`, 'error');
    pushToast(message, 'error');
  } finally {
    syncing.value = false;
  }
}

onMounted(async () => {
  await loadMeta();
  if (meta.value.has_cache) {
    await runDefaultPreview();
  }
});
</script>

<template>
  <div
    class="flex-1 overflow-y-auto bg-[radial-gradient(circle_at_top_left,_rgba(99,102,241,0.16),_transparent_30%),linear-gradient(180deg,_#f8fbff_0%,_#eef4fb_42%,_#f8fafc_100%)]"
  >
    <div class="mx-auto flex w-full max-w-6xl flex-col gap-6 px-6 py-6 pb-10">
      <section
        class="rounded-[24px] border border-white/70 bg-white/85 px-6 py-5 shadow-[0_18px_60px_rgba(15,23,42,0.08)] backdrop-blur"
      >
        <div class="flex items-start justify-between gap-4">
          <div class="flex min-w-0 items-start gap-3">
            <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-indigo-500 to-violet-600 shadow-sm">
              <FileSearch class="h-5 w-5 text-white" />
            </div>
            <div class="min-w-0">
              <p class="text-xs font-bold uppercase tracking-[0.16em] text-indigo-700">
                {{ t('toolsHub.cards.errorCodeLookup.chip') }}
              </p>
              <h1 class="mt-1 text-2xl font-bold text-slate-900">
                {{ t('errorCodeLookup.title') }}
              </h1>
              <p class="mt-1 text-sm text-slate-500">{{ t('errorCodeLookup.description') }}</p>
            </div>
          </div>

          <div class="flex shrink-0 items-center gap-3">
            <span
              class="text-xs text-slate-500"
              :title="meta.has_cache ? lastSyncedTooltip : ''"
            >
              {{ lastSyncedDisplay }}
            </span>
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-xl border border-indigo-200 bg-indigo-500 px-4 py-2 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-indigo-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40 focus-visible:ring-offset-2 focus-visible:ring-offset-white disabled:cursor-not-allowed disabled:opacity-60"
              :disabled="syncing"
              @click="onSync"
            >
              <RefreshCw class="h-4 w-4 motion-reduce:animate-none" :class="syncing ? 'animate-spin' : ''" aria-hidden="true" />
              <span>
                {{ syncing ? t('errorCodeLookup.syncing') : t('errorCodeLookup.syncButton') }}
              </span>
            </button>
          </div>
        </div>
      </section>

      <section
        v-if="statusBanner"
        class="rounded-2xl border px-4 py-3 text-sm shadow-sm"
        :class="
          statusBanner.type === 'success'
            ? 'border-emerald-200 bg-emerald-50 text-emerald-700'
            : 'border-rose-200 bg-rose-50 text-rose-700'
        "
      >
        {{ statusBanner.message }}
      </section>

      <section
        v-if="!meta.has_cache"
        class="flex flex-col items-center gap-4 rounded-[24px] border border-dashed border-slate-300 bg-white/70 py-16 text-center"
      >
        <FileSearch class="h-12 w-12 text-slate-400" />
        <p class="text-sm text-slate-600">{{ t('errorCodeLookup.empty.notSynced') }}</p>
        <button
          type="button"
          class="rounded-xl bg-indigo-500 px-5 py-2 text-sm font-semibold text-white shadow-sm hover:bg-indigo-600 disabled:opacity-60"
          :disabled="syncing"
          @click="onSync"
        >
          {{ t('errorCodeLookup.syncNowAction') }}
        </button>
      </section>

      <template v-else>
        <section
          class="rounded-[24px] border border-slate-200 bg-white/90 p-5 shadow-[0_14px_40px_rgba(15,23,42,0.06)]"
        >
          <div
            class="flex items-center gap-3 text-sm text-slate-500"
            role="radiogroup"
            :aria-label="t('errorCodeLookup.aria.modeGroup')"
          >
            <span class="font-semibold text-slate-700">
              {{ t('errorCodeLookup.modeLabel') }}
            </span>
            <label
              v-for="queryMode in (['single', 'range', 'keyword'] as ErrorCodeMode[])"
              :key="queryMode"
              class="inline-flex cursor-pointer items-center gap-2"
            >
              <input
                v-model="mode"
                type="radio"
                :value="queryMode"
                class="text-indigo-600 focus:ring-indigo-500"
              />
              <span>{{ t(`errorCodeLookup.modes.${queryMode}`) }}</span>
            </label>
          </div>

          <div class="mt-4 flex gap-3">
            <input
              v-model="inputValue"
              type="text"
              :placeholder="placeholder"
              class="flex-1 rounded-xl border bg-white px-4 py-2 text-sm text-slate-900 shadow-sm focus:outline-none focus:ring-2 focus:ring-indigo-500/30"
              :class="inputError ? 'border-red-400' : 'border-slate-200'"
              @keyup.enter="onSearch"
            />
            <button
              type="button"
              class="inline-flex items-center gap-2 rounded-xl bg-slate-900 px-4 py-2 text-sm font-semibold text-white shadow-sm hover:bg-slate-800 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-500/40 focus-visible:ring-offset-2 focus-visible:ring-offset-white disabled:opacity-60"
              :disabled="submitting"
              @click="onSearch"
            >
              <Search class="h-4 w-4" aria-hidden="true" />
              <span>{{ t('errorCodeLookup.searchButton') }}</span>
            </button>
          </div>

          <p v-if="inputError" class="mt-2 text-xs text-red-500">{{ inputError }}</p>
        </section>

        <Transition name="fst-result-fade" mode="out-in">
        <section
          :key="mode"
          class="rounded-[24px] border border-slate-200 bg-white/95 shadow-[0_14px_40px_rgba(15,23,42,0.06)]"
        >
          <div
            v-if="submitting && entries.length === 0"
            class="px-5 py-8"
          >
            <LoadingSkeleton variant="list-row" :count="5" />
          </div>

          <div
            v-else-if="entries.length === 0 && noResultMessage"
            class="px-5 py-12 text-center text-sm text-slate-500"
          >
            {{ noResultMessage }}
          </div>

          <table v-else class="w-full table-fixed text-sm text-slate-700">
            <thead class="bg-slate-50 text-slate-600">
              <tr>
                <th class="w-[110px] px-4 py-3 text-left">{{ t('errorCodeLookup.columns.code') }}</th>
                <th class="px-4 py-3 text-left">{{ t('errorCodeLookup.columns.messageCn') }}</th>
                <th class="px-4 py-3 text-left">{{ t('errorCodeLookup.columns.messageEn') }}</th>
                <th class="w-[120px] px-4 py-3 text-left">{{ t('errorCodeLookup.columns.module') }}</th>
                <th class="px-4 py-3 text-left">{{ t('errorCodeLookup.columns.solution') }}</th>
                <th class="w-[160px] px-4 py-3 text-left">{{ t('errorCodeLookup.columns.remark') }}</th>
              </tr>
            </thead>

            <tbody>
              <template v-for="(entry, index) in entries" :key="rowKey(entry, index)">
                <tr
                  role="button"
                  tabindex="0"
                  :aria-expanded="expandedKey === rowKey(entry, index)"
                  :aria-label="t('errorCodeLookup.aria.expandRow')"
                  class="cursor-pointer border-t border-slate-100 hover:bg-slate-50 focus-visible:outline-none focus-visible:bg-slate-50 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo-500/40"
                  @click="toggleExpand(entry, index)"
                  @keydown="onRowKeydown($event, entry, index)"
                >
                  <td class="px-4 py-3 font-mono">
                    <span class="rounded bg-slate-100 px-2 py-1 text-xs text-slate-700">
                      {{ entry.code }}
                    </span>
                  </td>
                  <td class="truncate px-4 py-3" :title="entry.message_cn">
                    {{ entry.message_cn || '—' }}
                  </td>
                  <td class="truncate px-4 py-3" :title="entry.message_en">
                    {{ entry.message_en || '—' }}
                  </td>
                  <td class="px-4 py-3">
                    <span v-if="entry.module" class="rounded-full bg-slate-100 px-2 py-0.5 text-xs">
                      {{ entry.module }}
                    </span>
                    <span v-else class="text-slate-400">—</span>
                  </td>
                  <td class="truncate px-4 py-3 text-slate-600" :title="entry.solution">
                    {{ entry.solution || '—' }}
                  </td>
                  <td class="truncate px-4 py-3 text-slate-500" :title="entry.remark">
                    {{ entry.remark || '—' }}
                  </td>
                </tr>

                <tr v-if="expandedKey === rowKey(entry, index)" class="bg-slate-50">
                  <td colspan="6" class="px-6 py-4">
                    <dl class="grid grid-cols-2 gap-x-6 gap-y-2 text-sm text-slate-700">
                      <div>
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.code') }}
                        </dt>
                        <dd class="font-mono">{{ entry.code }}</dd>
                      </div>
                      <div>
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.module') }}
                        </dt>
                        <dd>{{ entry.module || '—' }}</dd>
                      </div>
                      <div class="col-span-2">
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.messageCn') }}
                        </dt>
                        <dd class="break-words whitespace-pre-wrap">
                          {{ entry.message_cn || '—' }}
                        </dd>
                      </div>
                      <div class="col-span-2">
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.messageEn') }}
                        </dt>
                        <dd class="break-words whitespace-pre-wrap">
                          {{ entry.message_en || '—' }}
                        </dd>
                      </div>
                      <div class="col-span-2">
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.solution') }}
                        </dt>
                        <dd class="break-words whitespace-pre-wrap">
                          {{ entry.solution || '—' }}
                        </dd>
                      </div>
                      <div class="col-span-2">
                        <dt class="text-xs uppercase tracking-wide text-slate-400">
                          {{ t('errorCodeLookup.columns.remark') }}
                        </dt>
                        <dd class="break-words whitespace-pre-wrap">
                          {{ entry.remark || '—' }}
                        </dd>
                      </div>
                    </dl>
                  </td>
                </tr>
              </template>
            </tbody>
          </table>

          <div
            v-if="totalPages > 1"
            class="flex items-center justify-between gap-3 border-t border-slate-100 px-5 py-3 text-sm text-slate-600"
          >
            <button
              type="button"
              class="inline-flex items-center gap-1 rounded-lg border border-slate-200 px-3 py-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40 disabled:opacity-50"
              :disabled="currentPage <= 1 || submitting"
              @click="changePage(-1)"
            >
              <ChevronLeft class="h-4 w-4" aria-hidden="true" />
              {{ t('errorCodeLookup.pagination.prev') }}
            </button>

            <span>
              {{ t('errorCodeLookup.pagination.pageOf', { page: currentPage, total: totalPages }) }}
            </span>

            <div class="flex items-center gap-2">
              <span>{{ t('errorCodeLookup.pagination.jumpTo') }}</span>
              <input
                v-model="jumpInput"
                class="w-16 rounded-lg border border-slate-200 px-2 py-1 text-center focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40"
                type="text"
                inputmode="numeric"
                :aria-label="t('errorCodeLookup.aria.jumpInput')"
                @keyup.enter="onJump"
              />
              <button
                type="button"
                class="inline-flex items-center gap-1 rounded-lg border border-slate-200 px-3 py-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40 disabled:opacity-50"
                :disabled="currentPage >= totalPages || submitting"
                @click="changePage(1)"
              >
                {{ t('errorCodeLookup.pagination.next') }}
                <ChevronRight class="h-4 w-4" aria-hidden="true" />
              </button>
            </div>
          </div>
        </section>
        </Transition>
      </template>
    </div>
  </div>
</template>

<style scoped>
/* 120ms fade on mode change so the results section doesn't snap when the user
   switches between single / range / keyword. Reduced motion drops the
   transform. */
.fst-result-fade-enter-from {
  opacity: 0;
}
.fst-result-fade-enter-active {
  transition: opacity 120ms ease-out;
}
.fst-result-fade-enter-to {
  opacity: 1;
}
.fst-result-fade-leave-from {
  opacity: 1;
}
.fst-result-fade-leave-active {
  transition: opacity 120ms ease-in;
}
.fst-result-fade-leave-to {
  opacity: 0;
}
@media (prefers-reduced-motion: reduce) {
  .fst-result-fade-enter-active,
  .fst-result-fade-leave-active {
    transition: none;
  }
}
</style>
