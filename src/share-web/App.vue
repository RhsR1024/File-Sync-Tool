<script setup lang="ts">
import { computed, onMounted, ref, watchEffect } from 'vue';
import { useI18n } from 'vue-i18n';

import { fileShareApi, getErrorMessage, isForbidden, isUnauthorized } from './api';
import CreateDirectoryDialog from './components/CreateDirectoryDialog.vue';
import DeleteConfirmDialog from './components/DeleteConfirmDialog.vue';
import EntryTable from './components/EntryTable.vue';
import ImagePreviewDialog from './components/ImagePreviewDialog.vue';
import LoginDialog from './components/LoginDialog.vue';
import NewTextDialog from './components/NewTextDialog.vue';
import RenameDialog from './components/RenameDialog.vue';
import SearchBar from './components/SearchBar.vue';
import ToolbarActions from './components/ToolbarActions.vue';
import UploadDialog from './components/UploadDialog.vue';
import {
  canRenderAction,
  entryToDisplayEntry,
  isImageEntry,
  joinRelativePath,
  splitPathSegments,
  type FileShareDisplayEntry,
  type FileShareRootSummary,
  type FileShareSearchScope,
  type FileShareSearchResult,
  type FileShareSession,
} from './types';

const { t } = useI18n();

const session = ref<FileShareSession | null>(null);
const roots = ref<FileShareRootSummary[]>([]);
const currentRoot = ref('');
const currentPath = ref('');
const entries = ref<FileShareDisplayEntry[]>([]);
const globalResults = ref<FileShareDisplayEntry[]>([]);
const keyword = ref('');
const searchScope = ref<FileShareSearchScope>('current');

const pageError = ref('');
const loginError = ref('');
const uploadError = ref('');
const textError = ref('');
const renameError = ref('');
const deleteError = ref('');
const flashMessage = ref('');

const loadingSession = ref(true);
const loadingEntries = ref(false);
const searching = ref(false);
const mutating = ref(false);
const loggingIn = ref(false);
const loginOpen = ref(false);
const uploadOpen = ref(false);
const uploadMode = ref<'files' | 'directory'>('files');
const newTextOpen = ref(false);
const createDirectoryOpen = ref(false);
const renameOpen = ref(false);
const deleteOpen = ref(false);
const previewOpen = ref(false);
const previewTitle = ref('');
const previewSrc = ref('');
const createDirectoryError = ref('');
const renameTarget = ref<FileShareDisplayEntry | null>(null);
const deleteTarget = ref<FileShareDisplayEntry | null>(null);

const currentRootPath = computed(() => {
  return roots.value.find((root) => root.alias === currentRoot.value)?.path || '';
});

const breadcrumbs = computed(() => {
  const crumbs = [{ label: currentRoot.value || t('app.rootCrumb'), path: '' }];
  const segments = splitPathSegments(currentPath.value);
  let path = '';
  for (const segment of segments) {
    path = joinRelativePath(path, segment);
    crumbs.push({
      label: segment,
      path,
    });
  }
  return crumbs;
});

const displayedEntries = computed(() => {
  if (searchScope.value === 'global' && keyword.value.trim()) {
    return globalResults.value;
  }

  if (searchScope.value === 'current' && keyword.value.trim()) {
    const needle = keyword.value.trim().toLowerCase();
    return entries.value.filter((entry) => entry.name.toLowerCase().includes(needle));
  }

  return entries.value;
});

const emptyText = computed(() => {
  if (!currentRoot.value) {
    return t('app.noRoots');
  }
  if (searchScope.value === 'global' && keyword.value.trim()) {
    return t('app.noGlobalResults');
  }
  if (searchScope.value === 'current' && keyword.value.trim()) {
    return t('app.noCurrentResults');
  }
  return t('app.emptyDirectory');
});

const canSearchCurrent = computed(() => Boolean(session.value?.permissions.search_current));
const canSearchGlobal = computed(() => Boolean(session.value?.permissions.search_global));
const sessionChipText = computed(() => {
  if (!session.value) {
    return t('app.loggedOut');
  }
  return session.value.is_guest
    ? t('app.guestLabel', { name: session.value.account_name })
    : session.value.account_name;
});

async function bootstrap(preferredRoot?: string, preferredPath?: string) {
  loadingSession.value = true;
  pageError.value = '';

  try {
    session.value = await fileShareApi.getSession();
    loginOpen.value = false;
    loginError.value = '';
    await loadRoots(preferredRoot, preferredPath);
  } catch (error) {
    session.value = null;
    entries.value = [];
    roots.value = [];
    if (isUnauthorized(error)) {
      loginOpen.value = true;
      return;
    }
    if (isForbidden(error)) {
      pageError.value = t('app.forbiddenIp');
      return;
    }
    pageError.value = getErrorMessage(error);
  } finally {
    loadingSession.value = false;
  }
}

async function loadRoots(preferredRoot?: string, preferredPath?: string) {
  roots.value = await fileShareApi.listRoots();
  if (roots.value.length === 0) {
    currentRoot.value = '';
    currentPath.value = '';
    entries.value = [];
    return;
  }

  const nextRoot = preferredRoot && roots.value.some((root) => root.alias === preferredRoot)
    ? preferredRoot
    : currentRoot.value && roots.value.some((root) => root.alias === currentRoot.value)
      ? currentRoot.value
      : roots.value[0].alias;

  currentRoot.value = nextRoot;
  currentPath.value = preferredPath ?? currentPath.value;
  await loadEntries();
}

async function loadEntries() {
  if (!currentRoot.value) {
    return;
  }
  loadingEntries.value = true;
  pageError.value = '';

  try {
    const response = await fileShareApi.listEntries(currentRoot.value, currentPath.value);
    currentRoot.value = response.root_alias;
    currentPath.value = response.path;
    entries.value = response.entries.map((entry) => entryToDisplayEntry(entry, response.root_alias));
    if (searchScope.value === 'global' && keyword.value.trim()) {
      await runGlobalSearch();
    }
  } catch (error) {
    pageError.value = getErrorMessage(error);
  } finally {
    loadingEntries.value = false;
  }
}

async function runGlobalSearch() {
  if (!currentRoot.value || !keyword.value.trim()) {
    globalResults.value = [];
    return;
  }
  searching.value = true;
  pageError.value = '';
  try {
    const results = await fileShareApi.search(keyword.value, 'global');
    globalResults.value = results.map((entry: FileShareSearchResult) => ({
      ...entry,
      root_alias: entry.root_alias,
    }));
  } catch (error) {
    pageError.value = getErrorMessage(error);
  } finally {
    searching.value = false;
  }
}

async function handleSearch() {
  if (searchScope.value === 'current') {
    return;
  }
  await runGlobalSearch();
}

function clearSearch() {
  keyword.value = '';
  globalResults.value = [];
}

async function handleLogin(payload: { accountId: string; password: string }) {
  loggingIn.value = true;
  loginError.value = '';
  try {
    await fileShareApi.login(payload.accountId, payload.password);
    await bootstrap();
  } catch (error) {
    loginError.value = getErrorMessage(error);
  } finally {
    loggingIn.value = false;
  }
}

async function handleLogout() {
  mutating.value = true;
  pageError.value = '';
  try {
    await fileShareApi.logout();
    await bootstrap(currentRoot.value, currentPath.value);
  } catch (error) {
    pageError.value = getErrorMessage(error);
  } finally {
    mutating.value = false;
  }
}

async function openDirectory(entry: FileShareDisplayEntry) {
  if (!entry.is_dir) {
    if (isImageEntry(entry.name) && session.value?.permissions.preview_image) {
      openPreview(entry);
      return;
    }
    if (session.value?.permissions.download_file) {
      triggerDownload(entry.root_alias, entry.relative_path, false);
    }
    return;
  }

  if (searchScope.value === 'global' && keyword.value.trim()) {
    currentRoot.value = entry.root_alias;
    currentPath.value = entry.relative_path;
    searchScope.value = 'current';
    clearSearch();
    await loadEntries();
    return;
  }

  currentPath.value = entry.relative_path;
  await loadEntries();
}

async function navigateTo(path: string) {
  currentPath.value = path;
  await loadEntries();
}

async function handleRootChange(root: string) {
  currentRoot.value = root;
  currentPath.value = '';
  clearSearch();
  await loadEntries();
}

function openUpload(mode: 'files' | 'directory') {
  uploadMode.value = mode;
  uploadError.value = '';
  uploadOpen.value = true;
}

async function submitUpload(files: File[]) {
  if (!currentRoot.value) {
    return;
  }
  mutating.value = true;
  uploadError.value = '';
  try {
    if (uploadMode.value === 'files') {
      await fileShareApi.uploadFiles(currentRoot.value, currentPath.value, files);
    } else {
      await fileShareApi.uploadDirectory(currentRoot.value, currentPath.value, files);
    }
    uploadOpen.value = false;
    flashMessage.value = uploadMode.value === 'files'
      ? t('app.uploadFilesSuccess')
      : t('app.uploadDirectorySuccess');
    await loadEntries();
  } catch (error) {
    uploadError.value = getErrorMessage(error);
  } finally {
    mutating.value = false;
  }
}

function openCreateDirectoryDialog() {
  if (!currentRoot.value) {
    return;
  }
  createDirectoryError.value = '';
  createDirectoryOpen.value = true;
}

async function submitCreateDirectory(name: string) {
  if (!currentRoot.value) {
    return;
  }
  if (!name.trim()) {
    return;
  }
  mutating.value = true;
  createDirectoryError.value = '';
  try {
    await fileShareApi.createDirectory(currentRoot.value, currentPath.value, name.trim());
    createDirectoryOpen.value = false;
    flashMessage.value = t('app.createDirectorySuccess');
    await loadEntries();
  } catch (error) {
    createDirectoryError.value = getErrorMessage(error);
  } finally {
    mutating.value = false;
  }
}

async function createText(payload: { name: string; content: string }) {
  if (!currentRoot.value) {
    return;
  }
  mutating.value = true;
  textError.value = '';
  try {
    await fileShareApi.createText(currentRoot.value, currentPath.value, payload.name, payload.content);
    newTextOpen.value = false;
    flashMessage.value = t('app.createTextSuccess');
    await loadEntries();
  } catch (error) {
    textError.value = getErrorMessage(error);
  } finally {
    mutating.value = false;
  }
}

function openRename(entry: FileShareDisplayEntry) {
  renameTarget.value = entry;
  renameError.value = '';
  renameOpen.value = true;
}

async function submitRename(name: string) {
  if (!renameTarget.value) {
    return;
  }
  mutating.value = true;
  renameError.value = '';
  try {
    await fileShareApi.rename(renameTarget.value.root_alias, renameTarget.value.relative_path, name);
    renameOpen.value = false;
    renameTarget.value = null;
    flashMessage.value = t('app.renameSuccess');
    await refreshAfterMutation();
  } catch (error) {
    renameError.value = getErrorMessage(error);
  } finally {
    mutating.value = false;
  }
}

function openDelete(entry: FileShareDisplayEntry) {
  deleteTarget.value = entry;
  deleteError.value = '';
  deleteOpen.value = true;
}

async function submitDelete() {
  if (!deleteTarget.value) {
    return;
  }
  mutating.value = true;
  deleteError.value = '';
  try {
    await fileShareApi.remove(deleteTarget.value.root_alias, deleteTarget.value.relative_path);
    deleteOpen.value = false;
    deleteTarget.value = null;
    flashMessage.value = t('app.deleteSuccess');
    await refreshAfterMutation();
  } catch (error) {
    deleteError.value = getErrorMessage(error);
  } finally {
    mutating.value = false;
  }
}

function openPreview(entry: FileShareDisplayEntry) {
  previewTitle.value = entry.name;
  previewSrc.value = fileShareApi.previewUrl(entry.root_alias, entry.relative_path);
  previewOpen.value = true;
}

function triggerDownload(root: string, path: string, archive: boolean) {
  const href = archive
    ? fileShareApi.downloadArchiveUrl(root, path)
    : fileShareApi.downloadFileUrl(root, path);
  const link = document.createElement('a');
  link.href = href;
  link.target = '_blank';
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
}

async function refreshAfterMutation() {
  if (searchScope.value === 'global' && keyword.value.trim()) {
    await runGlobalSearch();
  } else {
    await loadEntries();
  }
}

onMounted(async () => {
  await bootstrap();
});

watchEffect(() => {
  document.title = t('app.pageTitle');
});
</script>

<template>
  <div class="app-shell">
    <div class="backdrop"></div>

    <main class="page">
      <section class="hero-card">
        <div>
          <p class="eyebrow">{{ t('app.eyebrow') }}</p>
          <h1>{{ t('app.title') }}</h1>
          <p class="hero-text">
            {{ t('app.currentRoot') }}:
            <strong>{{ currentRoot || t('app.unselected') }}</strong>
            <span v-if="currentRootPath"> · {{ currentRootPath }}</span>
          </p>
        </div>

        <div class="hero-actions">
          <div class="session-chip" :class="{ guest: session?.is_guest }">
            {{ sessionChipText }}
          </div>
          <button
            v-if="session"
            type="button"
            class="hero-button"
            :disabled="mutating || loadingSession"
            @click="handleLogout"
          >
            {{ session.is_guest ? t('app.switchAccount') : t('app.signOut') }}
          </button>
        </div>
      </section>

      <section class="panel">
        <ToolbarActions
          :roots="roots"
          :current-root="currentRoot"
          :breadcrumbs="breadcrumbs"
          :permissions="session?.permissions ?? null"
          :busy="loadingEntries || mutating || loadingSession"
          @select-root="handleRootChange"
          @navigate="navigateTo"
          @upload-files="openUpload('files')"
          @upload-directory="openUpload('directory')"
          @create-directory="openCreateDirectoryDialog"
          @create-text="newTextOpen = true"
          @refresh="loadEntries"
        />

        <SearchBar
          :keyword="keyword"
          :scope="searchScope"
          :can-search-current="canSearchCurrent"
          :can-search-global="canSearchGlobal"
          :busy="searching || loadingEntries || mutating"
          @update:keyword="keyword = $event"
          @update:scope="searchScope = $event"
          @search="handleSearch"
          @clear="clearSearch"
        />

        <p v-if="flashMessage" class="flash-banner">{{ flashMessage }}</p>
        <p v-if="pageError" class="error-banner">{{ pageError }}</p>

        <EntryTable
          :entries="displayedEntries"
          :permissions="session?.permissions ?? null"
          :loading="loadingEntries || searching || loadingSession"
          :empty-text="emptyText"
          :global-search="searchScope === 'global' && keyword.trim().length > 0"
          @open="openDirectory"
          @preview="openPreview"
          @download="triggerDownload($event.root_alias, $event.relative_path, false)"
          @archive="triggerDownload($event.root_alias, $event.relative_path, true)"
          @rename="openRename"
          @delete="openDelete"
        />

        <div v-if="session && !canRenderAction(session.permissions, 'upload') && !session.permissions.create_directory && !session.permissions.create_text" class="hint-box">
          {{ t('app.browseOnlyHint') }}
        </div>
      </section>
    </main>

    <LoginDialog
      :open="loginOpen"
      :busy="loggingIn"
      :error="loginError"
      @close="loginOpen = false"
      @submit="handleLogin"
    />

    <UploadDialog
      :open="uploadOpen"
      :mode="uploadMode"
      :busy="mutating"
      :error="uploadError"
      @close="uploadOpen = false"
      @submit="submitUpload"
    />

    <CreateDirectoryDialog
      :open="createDirectoryOpen"
      :busy="mutating"
      :error="createDirectoryError"
      @close="createDirectoryOpen = false"
      @submit="submitCreateDirectory"
    />

    <NewTextDialog
      :open="newTextOpen"
      :busy="mutating"
      :error="textError"
      @close="newTextOpen = false"
      @submit="createText"
    />

    <RenameDialog
      :open="renameOpen"
      :busy="mutating"
      :current-name="renameTarget?.name || ''"
      :error="renameError"
      @close="renameOpen = false"
      @submit="submitRename"
    />

    <DeleteConfirmDialog
      :open="deleteOpen"
      :busy="mutating"
      :target-name="deleteTarget?.name || ''"
      :error="deleteError"
      @close="deleteOpen = false"
      @submit="submitDelete"
    />

    <ImagePreviewDialog
      :open="previewOpen"
      :title="previewTitle"
      :src="previewSrc"
      @close="previewOpen = false"
    />
  </div>
</template>

<style scoped>
.app-shell {
  position: relative;
  min-height: 100vh;
  overflow: hidden;
  color: #eaf3fb;
}

.backdrop {
  position: fixed;
  inset: 0;
  background:
    radial-gradient(circle at 18% 18%, rgba(45, 113, 186, 0.22), transparent 0 30%),
    radial-gradient(circle at 82% 14%, rgba(20, 184, 166, 0.18), transparent 0 32%);
  pointer-events: none;
}

.page {
  position: relative;
  max-width: 1240px;
  margin: 0 auto;
  padding: 36px 20px 48px;
}

.hero-card,
.panel {
  border-radius: 28px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(8, 14, 24, 0.72);
  backdrop-filter: blur(16px);
  box-shadow: 0 20px 80px rgba(0, 0, 0, 0.26);
}

.hero-card {
  display: flex;
  justify-content: space-between;
  gap: 20px;
  padding: 32px;
  margin-bottom: 20px;
}

.eyebrow {
  margin: 0 0 10px;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: #79d6cf;
  font-size: 12px;
}

.hero-card h1 {
  margin: 0;
  font-size: clamp(32px, 5vw, 48px);
}

.hero-text {
  margin: 16px 0 0;
  color: #9cb2c7;
  line-height: 1.6;
}

.hero-actions {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}

.session-chip,
.hero-button {
  border-radius: 999px;
  padding: 10px 16px;
}

.session-chip {
  background: rgba(148, 163, 184, 0.12);
  color: #eff7ff;
}

.session-chip.guest {
  background: rgba(34, 197, 94, 0.16);
}

.hero-button {
  border: none;
  background: rgba(56, 189, 248, 0.14);
  color: #dff7ff;
}

.panel {
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 24px;
}

.flash-banner,
.error-banner,
.hint-box {
  margin: 0;
  border-radius: 18px;
  padding: 14px 16px;
}

.flash-banner {
  background: rgba(34, 197, 94, 0.12);
  color: #baf7d0;
}

.error-banner {
  background: rgba(239, 68, 68, 0.14);
  color: #fecaca;
}

.hint-box {
  background: rgba(59, 130, 246, 0.12);
  color: #c6e6ff;
}

@media (max-width: 880px) {
  .page {
    padding: 20px 14px 32px;
  }

  .hero-card {
    flex-direction: column;
    padding: 24px;
  }

  .hero-actions {
    flex-wrap: wrap;
  }
}
</style>
