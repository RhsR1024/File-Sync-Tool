<script setup lang="ts">
import { computed, onMounted, ref, watchEffect, type Ref } from 'vue';
import { useI18n } from 'vue-i18n';

import {
  fileShareApi,
  getErrorMessage,
  isForbidden,
  isNotFound,
  isUnauthorized,
} from './api';
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
  canPreviewEntry,
  shouldPromptForAccountSwitch,
  type FileShareNode,
  type FileShareSearchScope,
  type FileShareSession,
  type FileShareTreeCurrentKind,
  type FileShareTreeResponse,
} from './types';

const { t } = useI18n();

const session = ref<FileShareSession | null>(null);
const tree = ref<FileShareTreeResponse | null>(null);
const keyword = ref('');
const activeKeyword = ref('');
const searchScope = ref<FileShareSearchScope>('global');
const activeSearchScope = ref<FileShareSearchScope>('global');
const searchResults = ref<FileShareNode[]>([]);

const pageError = ref('');
const loginError = ref('');
const uploadError = ref('');
const textError = ref('');
const renameError = ref('');
const deleteError = ref('');
const createDirectoryError = ref('');
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
const renameTarget = ref<FileShareNode | null>(null);
const deleteTarget = ref<FileShareNode | null>(null);

const currentNodeId = computed(() => tree.value?.current.node_id ?? null);
const currentKind = computed<FileShareTreeCurrentKind | null>(() => tree.value?.current.kind ?? null);
const breadcrumbs = computed(() => tree.value?.breadcrumbs ?? []);
const searchActive = computed(() => activeKeyword.value.length > 0);
const displayedEntries = computed(() => (
  searchActive.value
    ? searchResults.value
    : tree.value?.children ?? []
));

const canSearchCurrent = computed(() => (
  currentKind.value !== 'home'
  && Boolean(session.value?.permissions.search_current)
));
const canSearchGlobal = computed(() => Boolean(session.value?.permissions.search_global));
const browseOnlyHint = computed(() => {
  const permissions = session.value?.permissions;
  if (!permissions) {
    return '';
  }

  return !permissions.upload_file
    && !permissions.upload_directory
    && !permissions.create_directory
    && !permissions.create_text
    && !permissions.rename
    && !permissions.delete
    ? t('app.browseOnlyHint')
    : '';
});
const emptyText = computed(() => {
  if (searchActive.value) {
    return activeSearchScope.value === 'global'
      ? t('app.noGlobalResults')
      : t('app.noCurrentResults');
  }
  return currentKind.value === 'home'
    ? t('app.noRoots')
    : t('app.emptyDirectory');
});
const sessionChipText = computed(() => {
  if (!session.value) {
    return t('app.loggedOut');
  }
  return session.value.is_guest
    ? t('app.guestLabel', { name: session.value.username })
    : session.value.username;
});
const sessionActionLabel = computed(() => {
  if (!session.value) {
    return '';
  }
  return session.value.is_guest
    ? t('app.switchAccount')
    : t('app.signOut');
});
const loginDescription = computed(() => (
  session.value?.is_guest
    ? t('login.switchAccountDescription')
    : t('login.description')
));

function defaultSearchScope(kind: FileShareTreeCurrentKind | null | undefined): FileShareSearchScope {
  return kind === 'home' ? 'global' : 'current';
}

function resetSearchState(kind: FileShareTreeCurrentKind | null | undefined) {
  const nextScope = defaultSearchScope(kind);
  keyword.value = '';
  activeKeyword.value = '';
  searchScope.value = nextScope;
  activeSearchScope.value = nextScope;
  searchResults.value = [];
}

function syncSearchScope(kind: FileShareTreeCurrentKind, preserveSearch: boolean) {
  if (!preserveSearch || !activeKeyword.value) {
    searchScope.value = defaultSearchScope(kind);
    return;
  }

  let nextScope = activeSearchScope.value;
  if (kind === 'home') {
    nextScope = 'global';
  }

  if (nextScope === 'current' && !session.value?.permissions.search_current) {
    nextScope = canSearchGlobal.value ? 'global' : defaultSearchScope(kind);
  }
  if (nextScope === 'global' && !session.value?.permissions.search_global) {
    nextScope = defaultSearchScope(kind);
  }

  activeSearchScope.value = nextScope;
  searchScope.value = nextScope;
}

async function loadTree(
  nodeId: string | null = null,
  options: { preserveSearch?: boolean; allowHomeFallback?: boolean } = {},
) {
  const preserveSearch = options.preserveSearch ?? false;
  const allowHomeFallback = options.allowHomeFallback ?? true;

  loadingEntries.value = true;
  pageError.value = '';

  try {
    const response = await fileShareApi.getTree(nodeId);
    tree.value = response;
    syncSearchScope(response.current.kind, preserveSearch);
  } catch (error) {
    if (isNotFound(error) && nodeId && allowHomeFallback) {
      resetSearchState('home');
      await loadTree(null, {
        preserveSearch: false,
        allowHomeFallback: false,
      });
      pageError.value = t('app.directoryNotFound');
      return;
    }
    throw error;
  } finally {
    loadingEntries.value = false;
  }

  if (preserveSearch && activeKeyword.value) {
    await rerunSearch();
  }
}

async function bootstrap(
  preferredNodeId: string | null = null,
  options: { preserveSearch?: boolean } = {},
) {
  loadingSession.value = true;
  pageError.value = '';

  try {
    session.value = await fileShareApi.getSession();
    loginOpen.value = false;
    loginError.value = '';
    await loadTree(preferredNodeId, {
      preserveSearch: options.preserveSearch ?? false,
    });
  } catch (error) {
    session.value = null;
    tree.value = null;
    searchResults.value = [];
    activeKeyword.value = '';

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

async function executeSearch(rawKeyword: string, scope: FileShareSearchScope) {
  const trimmed = rawKeyword.trim();
  if (!trimmed) {
    resetSearchState(currentKind.value);
    return;
  }

  const effectiveScope = scope === 'current' && currentNodeId.value
    ? 'current'
    : 'global';

  searching.value = true;
  pageError.value = '';

  try {
    const response = await fileShareApi.search(
      trimmed,
      effectiveScope === 'current' ? currentNodeId.value : null,
    );
    activeKeyword.value = trimmed;
    activeSearchScope.value = effectiveScope;
    searchScope.value = effectiveScope;
    searchResults.value = response.results;
  } catch (error) {
    pageError.value = getErrorMessage(error);
  } finally {
    searching.value = false;
  }
}

async function rerunSearch() {
  if (!activeKeyword.value) {
    searchResults.value = [];
    return;
  }
  await executeSearch(activeKeyword.value, activeSearchScope.value);
}

async function handleSearch() {
  await executeSearch(keyword.value, searchScope.value);
}

function clearSearch() {
  resetSearchState(currentKind.value);
}

async function handleLogin(payload: { username: string; password: string }) {
  loggingIn.value = true;
  loginError.value = '';

  try {
    await fileShareApi.login(payload.username, payload.password);
    await bootstrap(currentNodeId.value, {
      preserveSearch: searchActive.value,
    });
  } catch (error) {
    loginError.value = getErrorMessage(error);
  } finally {
    loggingIn.value = false;
  }
}

async function handleSessionAction() {
  if (shouldPromptForAccountSwitch(session.value)) {
    loginError.value = '';
    pageError.value = '';
    loginOpen.value = true;
    return;
  }

  mutating.value = true;
  pageError.value = '';

  try {
    await fileShareApi.logout();
    await bootstrap(currentNodeId.value, {
      preserveSearch: searchActive.value,
    });
  } catch (error) {
    pageError.value = getErrorMessage(error);
  } finally {
    mutating.value = false;
  }
}

function triggerDownload(node: FileShareNode) {
  const href = node.is_dir
    ? fileShareApi.downloadArchiveUrl(node.node_id)
    : fileShareApi.downloadFileUrl(node.node_id);
  const link = document.createElement('a');
  link.href = href;
  link.rel = 'noopener';
  link.download = node.is_dir ? `${node.name}.zip` : node.name;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
}

function openPreview(node: FileShareNode) {
  previewTitle.value = node.name;
  previewSrc.value = fileShareApi.previewUrl(node.node_id);
  previewOpen.value = true;
}

async function openEntry(node: FileShareNode) {
  if (node.is_dir) {
    clearSearch();
    await loadTree(node.node_id);
    return;
  }

  if (canPreviewEntry(session.value, node)) {
    openPreview(node);
    return;
  }
  if (node.permissions.download_file) {
    triggerDownload(node);
  }
}

async function navigate(nodeId: string | null) {
  clearSearch();
  await loadTree(nodeId);
}

function currentParentNodeId(): string | null {
  return currentKind.value === 'home' ? null : currentNodeId.value;
}

function openUpload(mode: 'files' | 'directory') {
  if (!currentParentNodeId()) {
    return;
  }
  uploadMode.value = mode;
  uploadError.value = '';
  uploadOpen.value = true;
}

async function refreshCurrentView(
  preferredNodeId: string | null = currentNodeId.value,
  options: { preserveSearch?: boolean } = {},
) {
  await bootstrap(preferredNodeId, {
    preserveSearch: options.preserveSearch ?? searchActive.value,
  });
}

async function handleMutationError(error: unknown, targetError: Ref<string>) {
  if (isForbidden(error)) {
    await refreshCurrentView(currentNodeId.value, {
      preserveSearch: searchActive.value,
    });
    const message = t('app.permissionChanged');
    pageError.value = message;
    targetError.value = message;
    return;
  }

  if (isUnauthorized(error)) {
    await refreshCurrentView(currentNodeId.value, {
      preserveSearch: searchActive.value,
    });
    targetError.value = t('login.description');
    return;
  }

  targetError.value = getErrorMessage(error);
}

async function submitUpload(files: File[]) {
  const parentNodeId = currentParentNodeId();
  if (!parentNodeId) {
    return;
  }

  mutating.value = true;
  uploadError.value = '';

  try {
    if (uploadMode.value === 'files') {
      await fileShareApi.uploadFiles(parentNodeId, files);
    } else {
      await fileShareApi.uploadDirectory(parentNodeId, files);
    }
    uploadOpen.value = false;
    flashMessage.value = uploadMode.value === 'files'
      ? t('app.uploadFilesSuccess')
      : t('app.uploadDirectorySuccess');
    await refreshCurrentView();
  } catch (error) {
    await handleMutationError(error, uploadError);
  } finally {
    mutating.value = false;
  }
}

function openCreateDirectoryDialog() {
  if (!currentParentNodeId()) {
    return;
  }
  createDirectoryError.value = '';
  createDirectoryOpen.value = true;
}

async function submitCreateDirectory(name: string) {
  const parentNodeId = currentParentNodeId();
  if (!parentNodeId || !name.trim()) {
    return;
  }

  mutating.value = true;
  createDirectoryError.value = '';

  try {
    await fileShareApi.createDirectory(parentNodeId, name.trim());
    createDirectoryOpen.value = false;
    flashMessage.value = t('app.createDirectorySuccess');
    await refreshCurrentView();
  } catch (error) {
    await handleMutationError(error, createDirectoryError);
  } finally {
    mutating.value = false;
  }
}

async function createText(payload: { name: string; content: string }) {
  const parentNodeId = currentParentNodeId();
  if (!parentNodeId || !payload.name.trim()) {
    return;
  }

  mutating.value = true;
  textError.value = '';

  try {
    await fileShareApi.createText(parentNodeId, payload.name, payload.content);
    newTextOpen.value = false;
    flashMessage.value = t('app.createTextSuccess');
    await refreshCurrentView();
  } catch (error) {
    await handleMutationError(error, textError);
  } finally {
    mutating.value = false;
  }
}

function openRename(node: FileShareNode) {
  renameTarget.value = node;
  renameError.value = '';
  renameOpen.value = true;
}

async function submitRename(name: string) {
  if (!renameTarget.value || !name.trim()) {
    return;
  }

  mutating.value = true;
  renameError.value = '';

  try {
    await fileShareApi.rename(renameTarget.value.node_id, name.trim());
    renameOpen.value = false;
    renameTarget.value = null;
    flashMessage.value = t('app.renameSuccess');
    await refreshCurrentView(currentNodeId.value, {
      preserveSearch: searchActive.value,
    });
  } catch (error) {
    await handleMutationError(error, renameError);
  } finally {
    mutating.value = false;
  }
}

function openDelete(node: FileShareNode) {
  deleteTarget.value = node;
  deleteError.value = '';
  deleteOpen.value = true;
}

function currentViewDependsOn(node: FileShareNode | null): boolean {
  if (!node) {
    return false;
  }
  return breadcrumbs.value.some((crumb) => crumb.node_id === node.node_id);
}

async function submitDelete() {
  if (!deleteTarget.value) {
    return;
  }

  const target = deleteTarget.value;
  const fallbackNodeId = currentViewDependsOn(target) ? null : currentNodeId.value;

  mutating.value = true;
  deleteError.value = '';

  try {
    await fileShareApi.remove(target.node_id);
    deleteOpen.value = false;
    deleteTarget.value = null;
    flashMessage.value = t('app.deleteSuccess');
    await refreshCurrentView(fallbackNodeId, {
      preserveSearch: searchActive.value && fallbackNodeId !== null,
    });
  } catch (error) {
    await handleMutationError(error, deleteError);
  } finally {
    mutating.value = false;
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
      <section class="panel">
        <ToolbarActions
          :breadcrumbs="breadcrumbs"
          :current-kind="currentKind"
          :permissions="session?.permissions ?? null"
          :session-text="session ? sessionChipText : ''"
          :session-is-guest="Boolean(session?.is_guest)"
          :session-action-label="sessionActionLabel"
          :browse-only-hint="browseOnlyHint"
          :busy="loadingEntries || mutating || loadingSession || searching"
          @navigate="navigate"
          @upload-files="openUpload('files')"
          @upload-directory="openUpload('directory')"
          @create-directory="openCreateDirectoryDialog"
          @create-text="newTextOpen = true"
          @refresh="refreshCurrentView()"
          @session-action="handleSessionAction"
        />

        <SearchBar
          :keyword="keyword"
          :scope="searchScope"
          :can-search-current="canSearchCurrent"
          :can-search-global="canSearchGlobal"
          :busy="searching || loadingEntries || mutating || loadingSession"
          @update:keyword="keyword = $event"
          @update:scope="searchScope = $event"
          @search="handleSearch"
          @clear="clearSearch"
        />

        <p v-if="flashMessage" class="flash-banner">{{ flashMessage }}</p>
        <p v-if="pageError" class="error-banner">{{ pageError }}</p>

        <EntryTable
          :entries="displayedEntries"
          :session="session"
          :loading="loadingEntries || searching || loadingSession"
          :empty-text="emptyText"
          :search-active="searchActive"
          @open="openEntry"
          @preview="openPreview"
          @download="triggerDownload"
          @rename="openRename"
          @delete="openDelete"
        />
      </section>
    </main>

    <LoginDialog
      :open="loginOpen"
      :busy="loggingIn"
      :error="loginError"
      :description="loginDescription"
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
      :current-name="renameTarget?.name ?? ''"
      :error="renameError"
      @close="renameOpen = false"
      @submit="submitRename"
    />

    <DeleteConfirmDialog
      :open="deleteOpen"
      :busy="mutating"
      :target-name="deleteTarget?.display_path ?? ''"
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
  color: var(--fs-text);
}

.backdrop {
  position: fixed;
  inset: 0;
  background:
    radial-gradient(circle at 18% 18%, rgba(147, 197, 253, 0.22), transparent 0 30%),
    radial-gradient(circle at 82% 14%, rgba(110, 231, 183, 0.16), transparent 0 32%);
  pointer-events: none;
}

.page {
  position: relative;
  max-width: 1280px;
  margin: 0 auto;
  padding: 24px 18px 40px;
}

.panel {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 20px;
  border-radius: 24px;
  border: 1px solid var(--fs-panel-border);
  background: var(--fs-panel);
  backdrop-filter: blur(16px);
  box-shadow: 0 12px 48px rgba(0, 0, 0, 0.1);
}

.flash-banner,
.error-banner {
  margin: 0;
  border-radius: 16px;
  padding: 12px 14px;
}

.flash-banner {
  border: 1px solid rgba(21, 128, 61, 0.2);
  background: rgba(21, 128, 61, 0.07);
  color: #14532d;
}

.error-banner {
  border: 1px solid rgba(185, 28, 28, 0.22);
  background: rgba(185, 28, 28, 0.07);
  color: #7f1d1d;
}

@media (max-width: 880px) {
  .page {
    padding: 18px 14px 32px;
  }

  .panel {
    padding: 18px;
    border-radius: 22px;
  }
}
</style>
