<script setup lang="ts">
import { ArrowUp, Check, ChevronRight, File, FileArchive, Folder, Loader2, RefreshCw } from 'lucide-vue-next';
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import BrowserDialog from '@/components/remote-package-patch/BrowserDialog.vue';
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

const browserOpen = ref(false);
const currentPath = ref('/');
const pathDraft = ref('/');
const entries = ref<RemoteDirEntry[]>([]);
const pendingSelectedPath = ref('');
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

function openBrowser() {
  if (!props.config || props.disabled) return;
  pendingSelectedPath.value = selectedPath.value;
  browserOpen.value = true;
  void loadPath(currentPath.value || '/');
}

function closeBrowser() {
  browserOpen.value = false;
}

function confirmSelection() {
  if (!pendingSelectedPath.value) return;
  emit('update:modelValue', pendingSelectedPath.value);
  closeBrowser();
}

function openEntry(entry: RemoteDirEntry) {
  if (!interactive.value) return;
  if (entry.kind === 'dir') {
    void loadPath(entry.path);
    return;
  }
  if (isPackage(entry)) {
    pendingSelectedPath.value = entry.path;
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
  () => {
    entries.value = [];
    currentPath.value = '/';
    pathDraft.value = '/';
    if (browserOpen.value && props.config && !props.disabled) void loadPath('/');
  },
);
</script>

<template>
  <button
    type="button"
    class="flex min-h-11 w-full cursor-pointer items-center gap-3 overflow-hidden rounded-lg border border-slate-200 bg-white px-3 py-2 text-left transition-colors hover:border-sky-200 hover:bg-sky-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50 disabled:cursor-not-allowed disabled:bg-slate-50 disabled:text-slate-400"
    :disabled="!config || disabled"
    :aria-label="t('remotePackagePatch.browser.chooseTitle')"
    @click="openBrowser"
  >
    <FileArchive class="h-5 w-5 shrink-0 text-sky-600" />
    <span class="min-w-0 flex-1">
      <span class="block text-xs font-medium text-slate-500">{{ t('remotePackagePatch.browser.selectedLabel') }}</span>
      <span class="mt-0.5 block truncate font-mono text-xs text-slate-800">
        {{ selectedPath || t('remotePackagePatch.browser.choosePlaceholder') }}
      </span>
    </span>
    <ChevronRight class="h-4 w-4 shrink-0 text-slate-400" />
  </button>

  <BrowserDialog
    :open="browserOpen"
    :title="t('remotePackagePatch.browser.chooseTitle')"
    :hint="t('remotePackagePatch.browser.chooseHint')"
    :close-label="t('remotePackagePatch.browser.cancel')"
    wide
    @close="closeBrowser"
  >
    <template #icon>
      <FileArchive class="h-5 w-5" />
    </template>

    <div class="flex flex-col gap-3 border-b border-slate-200 bg-slate-50 p-4 md:flex-row md:items-center">
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
          class="min-w-0 flex-1 rounded-md border border-slate-200 bg-white px-3 py-2 font-mono text-xs outline-none focus:border-sky-400 focus:ring-2 focus:ring-sky-100 disabled:cursor-not-allowed disabled:bg-slate-50"
          :disabled="!interactive"
          :aria-label="t('remotePackagePatch.browser.currentPath')"
          @keydown.enter.prevent="loadPath(pathDraft)"
        />
      </div>
      <div class="shrink-0 text-xs text-slate-500">
        {{ t('remotePackagePatch.browser.itemCount', { count: entries.length }) }}
      </div>
    </div>

    <div v-if="error" class="border-b border-red-100 bg-red-50 px-4 py-2 text-xs text-red-700">
      {{ error }}
    </div>

    <div class="min-h-64 flex-1 overflow-auto bg-white">
      <table class="w-full table-fixed text-left text-sm">
        <thead class="sticky top-0 z-10 bg-slate-50 text-xs uppercase text-slate-500">
          <tr>
            <th class="w-[42%] px-4 py-2">{{ t('remotePackagePatch.browser.colName') }}</th>
            <th class="w-[14%] px-3 py-2">{{ t('remotePackagePatch.browser.colKind') }}</th>
            <th class="w-[16%] px-3 py-2 text-right">{{ t('remotePackagePatch.browser.colSize') }}</th>
            <th class="w-[28%] px-3 py-2">{{ t('remotePackagePatch.browser.colModified') }}</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-slate-100">
          <tr v-if="loading">
            <td colspan="4" class="px-3 py-10 text-center text-slate-500">
              <Loader2 class="mx-auto mb-2 h-5 w-5 animate-spin" />
              {{ t('remotePackagePatch.browser.loading') }}
            </td>
          </tr>
          <tr v-else-if="entries.length === 0">
            <td colspan="4" class="px-3 py-10 text-center text-slate-500">
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
              pendingSelectedPath === entry.path ? 'bg-emerald-50' : '',
              isPackage(entry) ? 'text-slate-950' : 'text-slate-700',
            ]"
            @click="openEntry(entry)"
            @dblclick="isPackage(entry) && pendingSelectedPath === entry.path && confirmSelection()"
            @keydown="onRowKeydown($event, entry)"
          >
            <td class="px-4 py-2.5">
              <div class="flex min-w-0 items-center gap-2">
                <Folder v-if="entry.kind === 'dir'" class="h-4 w-4 shrink-0 text-amber-500" />
                <FileArchive v-else-if="isPackage(entry)" class="h-4 w-4 shrink-0 text-sky-600" />
                <File v-else class="h-4 w-4 shrink-0 text-slate-400" />
                <span class="truncate font-medium" :title="entry.name">{{ entry.name }}</span>
                <Check v-if="pendingSelectedPath === entry.path" class="h-4 w-4 shrink-0 text-emerald-600" />
              </div>
            </td>
            <td class="px-3 py-2 text-xs text-slate-500">{{ t(`remotePackagePatch.browser.kinds.${entry.kind}`) }}</td>
            <td class="px-3 py-2 text-right font-mono text-xs text-slate-500">
              {{ entry.kind === 'dir' ? '-' : formatBytes(entry.size) }}
            </td>
            <td class="truncate px-3 py-2 text-xs text-slate-500">{{ formatModified(entry) }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <footer class="flex flex-col gap-3 border-t border-slate-200 bg-slate-50 px-5 py-4 sm:flex-row sm:items-center">
      <div class="min-w-0 flex-1">
        <div class="text-xs text-slate-500">{{ t('remotePackagePatch.browser.pendingLabel') }}</div>
        <div class="mt-0.5 truncate font-mono text-xs font-medium text-slate-700">
          {{ pendingSelectedPath || t('remotePackagePatch.browser.noneSelected') }}
        </div>
      </div>
      <div class="flex justify-end gap-2">
        <button type="button" class="rpp-secondary" @click="closeBrowser">
          {{ t('remotePackagePatch.browser.cancel') }}
        </button>
        <button type="button" class="rpp-primary" :disabled="!pendingSelectedPath" @click="confirmSelection">
          {{ t('remotePackagePatch.browser.chooseButton') }}
        </button>
      </div>
    </footer>
  </BrowserDialog>
</template>

<style scoped>
@reference "../../style.css";

.rpp-icon-button {
  @apply inline-flex h-9 w-9 shrink-0 cursor-pointer items-center justify-center rounded-md border border-slate-200 bg-white text-slate-600 transition-colors hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-300 disabled:cursor-not-allowed disabled:opacity-50;
}

.rpp-primary {
  @apply inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-sky-600 px-3.5 py-2 text-sm font-semibold text-white transition-colors hover:bg-sky-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:bg-slate-300;
}

.rpp-secondary {
  @apply inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2 text-sm font-semibold text-slate-700 transition-colors hover:border-sky-200 hover:bg-sky-50 hover:text-sky-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50 focus-visible:ring-offset-2;
}
</style>
