<script setup lang="ts">
import { ArrowUp, Check, File, FileArchive, Folder, Loader2, RefreshCw } from 'lucide-vue-next';
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import { remotePackagePatchApi, type RemoteDirEntry, type RemoteSshConfig } from '@/lib/tauri';
import { formatBytes } from '@/lib/remotePackagePatch';

const props = defineProps<{
  config: RemoteSshConfig | null;
  disabled?: boolean;
  modelValue?: string;
}>();

const emit = defineEmits<{
  (event: 'update:modelValue', value: string): void;
  (event: 'error', value: string): void;
}>();

const { t } = useI18n();

const currentPath = ref('/');
const pathDraft = ref('/');
const entries = ref<RemoteDirEntry[]>([]);
const loading = ref(false);
const error = ref('');

const selectedPath = computed(() => props.modelValue ?? '');
const interactive = computed(() => Boolean(props.config) && !props.disabled && !loading.value);

function isPackage(entry: RemoteDirEntry) {
  return entry.kind === 'file' && entry.name.endsWith('.tar.gz');
}

function parentPath(path: string) {
  const trimmed = path.replace(/\/+$/g, '');
  if (!trimmed || trimmed === '/') return '/';
  const index = trimmed.lastIndexOf('/');
  return index <= 0 ? '/' : trimmed.slice(0, index);
}

function formatModified(entry: RemoteDirEntry) {
  if (!entry.modifiedMs) return '-';
  return new Date(entry.modifiedMs).toLocaleString();
}

async function loadPath(path: string) {
  if (!props.config || props.disabled) return;
  loading.value = true;
  error.value = '';
  try {
    const listing = await remotePackagePatchApi.listDir(props.config, path || '/');
    currentPath.value = listing.path || path || '/';
    pathDraft.value = currentPath.value;
    entries.value = listing.entries;
  } catch (err) {
    const message = String(err);
    error.value = message;
    emit('error', message);
  } finally {
    loading.value = false;
  }
}

function openEntry(entry: RemoteDirEntry) {
  if (!interactive.value) return;
  if (entry.kind === 'dir') {
    void loadPath(entry.path);
    return;
  }
  if (isPackage(entry)) {
    emit('update:modelValue', entry.path);
  }
}

function onRowKeydown(event: KeyboardEvent, entry: RemoteDirEntry) {
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    openEntry(entry);
  }
}

watch(
  () => props.config,
  (config) => {
    if (config && !props.disabled) {
      void loadPath(currentPath.value || '/');
    }
  },
  { immediate: true },
);

watch(
  () => props.disabled,
  (disabled) => {
    if (!disabled && props.config) {
      void loadPath(currentPath.value || '/');
    }
  },
);
</script>

<template>
  <section class="overflow-hidden rounded-md border border-slate-200">
    <div class="flex flex-col gap-3 border-b border-slate-200 bg-white p-3 md:flex-row md:items-center">
      <div class="flex min-w-0 flex-1 items-center gap-2">
        <button
          type="button"
          class="rpp-icon-button"
          :disabled="!interactive || currentPath === '/'"
          :title="t('remotePackagePatch.browser.parentDir')"
          :aria-label="t('remotePackagePatch.browser.parentDir')"
          @click="loadPath(parentPath(currentPath))"
        >
          <ArrowUp class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="rpp-icon-button"
          :disabled="!interactive"
          :title="t('remotePackagePatch.browser.refresh')"
          :aria-label="t('remotePackagePatch.browser.refresh')"
          @click="loadPath(currentPath)"
        >
          <RefreshCw class="h-4 w-4" :class="loading ? 'animate-spin' : ''" />
        </button>
        <input
          v-model="pathDraft"
          class="min-w-0 flex-1 rounded-md border border-slate-200 px-3 py-2 font-mono text-xs outline-none focus:border-sky-400 focus:ring-2 focus:ring-sky-100 disabled:cursor-not-allowed disabled:bg-slate-50"
          :disabled="!interactive"
          @keydown.enter.prevent="loadPath(pathDraft)"
        />
      </div>
      <div class="text-xs text-slate-500">
        {{ t('remotePackagePatch.browser.itemCount', { count: entries.length }) }}
      </div>
    </div>

    <div v-if="error" class="border-b border-red-100 bg-red-50 px-3 py-2 text-xs text-red-700">
      {{ error }}
    </div>

    <div class="max-h-[420px] overflow-auto bg-white">
      <table class="w-full table-fixed text-left text-sm">
        <thead class="sticky top-0 z-10 bg-slate-50 text-xs uppercase text-slate-500">
          <tr>
            <th class="w-[42%] px-3 py-2">{{ t('remotePackagePatch.browser.colName') }}</th>
            <th class="w-[14%] px-3 py-2">{{ t('remotePackagePatch.browser.colKind') }}</th>
            <th class="w-[16%] px-3 py-2 text-right">{{ t('remotePackagePatch.browser.colSize') }}</th>
            <th class="w-[28%] px-3 py-2">{{ t('remotePackagePatch.browser.colModified') }}</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-slate-100">
          <tr v-if="!config">
            <td colspan="4" class="px-3 py-8 text-center text-slate-500">
              {{ t('remotePackagePatch.browser.notConnected') }}
            </td>
          </tr>
          <tr v-else-if="loading">
            <td colspan="4" class="px-3 py-8 text-center text-slate-500">
              <Loader2 class="mx-auto mb-2 h-5 w-5 animate-spin" />
              {{ t('remotePackagePatch.browser.loading') }}
            </td>
          </tr>
          <tr v-else-if="entries.length === 0">
            <td colspan="4" class="px-3 py-8 text-center text-slate-500">
              {{ t('remotePackagePatch.browser.empty') }}
            </td>
          </tr>
          <tr
            v-for="entry in entries"
            v-else
            :key="entry.path"
            :tabindex="interactive ? 0 : -1"
            class="transition-colors focus:outline-none"
            :class="[
              interactive ? 'cursor-pointer hover:bg-sky-50 focus:bg-sky-50' : 'cursor-not-allowed opacity-60',
              selectedPath === entry.path ? 'bg-emerald-50' : '',
              isPackage(entry) ? 'text-slate-950' : 'text-slate-700',
            ]"
            @click="openEntry(entry)"
            @keydown="onRowKeydown($event, entry)"
          >
            <td class="px-3 py-2">
              <div class="flex min-w-0 items-center gap-2">
                <Folder v-if="entry.kind === 'dir'" class="h-4 w-4 shrink-0 text-amber-500" />
                <FileArchive v-else-if="isPackage(entry)" class="h-4 w-4 shrink-0 text-sky-600" />
                <File v-else class="h-4 w-4 shrink-0 text-slate-400" />
                <span class="truncate font-medium" :title="entry.name">{{ entry.name }}</span>
                <Check v-if="selectedPath === entry.path" class="h-4 w-4 shrink-0 text-emerald-600" />
              </div>
            </td>
            <td class="px-3 py-2 text-xs text-slate-500">
              {{ t(`remotePackagePatch.browser.kinds.${entry.kind}`) }}
            </td>
            <td class="px-3 py-2 text-right font-mono text-xs text-slate-500">
              {{ entry.kind === 'dir' ? '-' : formatBytes(entry.size) }}
            </td>
            <td class="truncate px-3 py-2 text-xs text-slate-500">{{ formatModified(entry) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>

<style scoped>
@reference "../../style.css";

.rpp-icon-button {
  @apply inline-flex h-9 w-9 shrink-0 cursor-pointer items-center justify-center rounded-md border border-slate-200 bg-white text-slate-600 transition-colors hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-300 disabled:cursor-not-allowed disabled:opacity-50;
}
</style>
