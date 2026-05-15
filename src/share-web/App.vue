<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch, watchEffect, type Ref } from 'vue';
import { useI18n } from 'vue-i18n';

import {
  fileShareApi,
  getErrorMessage,
  isForbidden,
  isNotFound,
  isUnauthorized,
} from './api';
import Breadcrumbs from './components/Breadcrumbs.vue';
import BulkActionBar from './components/BulkActionBar.vue';
import CreateDirectoryDialog from './components/CreateDirectoryDialog.vue';
import DeleteConfirmDialog from './components/DeleteConfirmDialog.vue';
import EntryTable from './components/EntryTable.vue';
import Flash from './components/Flash.vue';
import ImagePreviewDialog from './components/ImagePreviewDialog.vue';
import LoginDialog from './components/LoginDialog.vue';
import NewTextDialog from './components/NewTextDialog.vue';
import RenameDialog from './components/RenameDialog.vue';
import SearchBar from './components/SearchBar.vue';
import Sidebar from './components/Sidebar.vue';
import ToolbarActions from './components/ToolbarActions.vue';
import TopBar from './components/TopBar.vue';
import UploadDialog from './components/UploadDialog.vue';
import { Icon } from './components/icons';
import {
  loadRecentPaths,
  recordRecentPath,
  type RecentPathEntry,
} from './lib/recent-paths';
import {
  parseHash,
  pushPath,
  replacePath,
  subscribe,
  type UrlState,
} from './lib/url-state';
import { loadViewMode, saveViewMode, type EntryViewMode } from './lib/view-mode';
import {
  loadSortPreference,
  saveSortPreference,
  type EntrySortDirection,
  type EntrySortKey,
} from './lib/sort-preference';
import { FILE_SHARE_WEB_SESSION_HEARTBEAT_INTERVAL_MS } from '../lib/lanShareStatus';
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

type UrlAction = 'push' | 'replace' | 'none';
type ViewIntent = { id: number };
type SearchStateSnapshot = {
  keyword: string;
  activeKeyword: string;
  searchScope: FileShareSearchScope;
  activeSearchScope: FileShareSearchScope;
  searchResults: FileShareNode[];
};

const session = ref<FileShareSession | null>(null);
const tree = ref<FileShareTreeResponse | null>(null);
const shareRoots = ref<FileShareNode[]>([]);
const recentPaths = ref<RecentPathEntry[]>(loadRecentPaths());
const keyword = ref('');
const activeKeyword = ref('');
const searchScope = ref<FileShareSearchScope>('global');
const activeSearchScope = ref<FileShareSearchScope>('global');
const searchResults = ref<FileShareNode[]>([]);

const view = ref<EntryViewMode>(loadViewMode());
const initialSort = loadSortPreference();
const sortKey = ref<EntrySortKey>(initialSort.key);
const sortDirection = ref<EntrySortDirection>(initialSort.direction);
const selectedIds = ref<Set<string>>(new Set());

const pageError = ref('');
const loginError = ref('');
const uploadError = ref('');
const textError = ref('');
const renameError = ref('');
const deleteError = ref('');
const createDirectoryError = ref('');
const flashMessage = ref('');
const guestNoticeOpen = ref(true);

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
let unsubscribeFromUrl: (() => void) | null = null;
let sessionHeartbeatTimer: ReturnType<typeof setInterval> | null = null;
let flashTimer: ReturnType<typeof setTimeout> | null = null;
let unmounted = false;
let latestViewIntentId = 0;
let latestSessionRequestId = 0;
let latestTreeRequestId = 0;
let latestSearchRequestId = 0;

const currentNodeId = computed(() => tree.value?.current.node_id ?? null);
const currentKind = computed<FileShareTreeCurrentKind | null>(() => tree.value?.current.kind ?? null);
const currentName = computed(() => tree.value?.current.name ?? '');
const breadcrumbs = computed(() => tree.value?.breadcrumbs ?? []);
const searchActive = computed(() => activeKeyword.value.length > 0);
const rawEntries = computed(() => (
  searchActive.value
    ? searchResults.value
    : tree.value?.children ?? []
));

const displayedEntries = computed(() => {
  const list = rawEntries.value.slice();
  const dir = sortDirection.value === 'asc' ? 1 : -1;
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

  list.sort((a, b) => {
    if (a.is_dir !== b.is_dir) {
      return a.is_dir ? -1 : 1;
    }
    let cmp = 0;
    if (sortKey.value === 'name') {
      cmp = collator.compare(a.name, b.name);
    } else if (sortKey.value === 'size') {
      const aSize = a.is_dir ? -1 : (a.size ?? -1);
      const bSize = b.is_dir ? -1 : (b.size ?? -1);
      cmp = aSize - bSize;
    } else {
      const aTime = a.modified ? Date.parse(a.modified) : 0;
      const bTime = b.modified ? Date.parse(b.modified) : 0;
      cmp = (Number.isFinite(aTime) ? aTime : 0) - (Number.isFinite(bTime) ? bTime : 0);
    }
    if (cmp === 0) {
      cmp = collator.compare(a.name, b.name);
      return cmp;
    }
    return cmp * dir;
  });
  return list;
});

const folderCount = computed(() => displayedEntries.value.filter((entry) => entry.is_dir).length);
const fileCount = computed(() => displayedEntries.value.length - folderCount.value);

const canSearchCurrent = computed(() => (
  currentKind.value !== 'home'
  && Boolean(session.value?.permissions.search_current)
));
const canSearchGlobal = computed(() => Boolean(session.value?.permissions.search_global));

const showGuestNotice = computed(() => (
  Boolean(session.value?.is_guest)
  && guestNoticeOpen.value
));

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

const loginDescription = computed(() => (
  session.value?.is_guest
    ? t('login.switchAccountDescription')
    : t('login.description')
));

const pageTitle = computed(() => {
  if (!tree.value) {
    return t('app.pageTitle');
  }
  if (currentKind.value === 'home') {
    return t('app.sidebarHome');
  }
  return currentName.value || t('app.pageTitle');
});

const pageStatLabel = computed(() => {
  if (currentKind.value === 'home' || displayedEntries.value.length === 0) {
    return '';
  }
  return `${t('app.folderCount', { n: folderCount.value })} · ${t('app.fileCount', { n: fileCount.value })}`;
});

const pageSubText = computed(() => {
  if (searchActive.value) {
    const scopeLabel = activeSearchScope.value === 'global' ? t('search.global') : t('search.current');
    return t('app.searchSummary', { query: activeKeyword.value, scope: scopeLabel });
  }
  if (currentKind.value === 'home') {
    return t('app.homeSubtitle');
  }
  return '';
});

const activeRootNodeId = computed<string | null>(() => {
  if (!tree.value) {
    return null;
  }
  if (currentKind.value === 'share_root' && tree.value.current.node_id) {
    return tree.value.current.node_id;
  }
  const rootCrumb = tree.value.breadcrumbs.find((crumb) => crumb.node_id !== null);
  return rootCrumb?.node_id ?? null;
});

const busy = computed(() => (
  loadingSession.value || loadingEntries.value || searching.value || mutating.value
));

const selectedEntries = computed(() => (
  displayedEntries.value.filter((entry) => selectedIds.value.has(entry.node_id))
));

const canBulkDownload = computed(() => (
  selectedEntries.value.length > 0
  && selectedEntries.value.every((entry) => (
    entry.is_dir
      ? entry.permissions.download_archive
      : entry.permissions.download_file
  ))
));

const canBulkDelete = computed(() => (
  selectedEntries.value.length > 0
  && selectedEntries.value.every((entry) => entry.permissions.delete)
));

function defaultSearchScope(kind: FileShareTreeCurrentKind | null | undefined): FileShareSearchScope {
  return kind === 'home' ? 'global' : 'current';
}

function startViewIntent(): ViewIntent {
  latestViewIntentId += 1;
  return {
    id: latestViewIntentId,
  };
}

function resolveIntent(intent?: ViewIntent): ViewIntent {
  return intent ?? startViewIntent();
}

function isIntentCurrent(intent: ViewIntent): boolean {
  return intent.id === latestViewIntentId;
}

function invalidateViewLifecycle() {
  latestViewIntentId += 1;
  latestSessionRequestId += 1;
  latestTreeRequestId += 1;
  latestSearchRequestId += 1;
}

function startSessionHeartbeat() {
  if (sessionHeartbeatTimer) {
    clearInterval(sessionHeartbeatTimer);
  }
  sessionHeartbeatTimer = setInterval(() => {
    if (unmounted || !session.value) {
      return;
    }
    void (async () => {
      try {
        session.value = await fileShareApi.getSession();
      } catch {
        /* The next user action will surface auth or network failures. */
      }
    })();
  }, FILE_SHARE_WEB_SESSION_HEARTBEAT_INTERVAL_MS);
}

function beginSessionRequest(intent: ViewIntent) {
  latestSessionRequestId += 1;
  const requestId = latestSessionRequestId;
  loadingSession.value = true;

  return {
    isCurrent() {
      return isIntentCurrent(intent) && requestId === latestSessionRequestId;
    },
    finish() {
      if (requestId === latestSessionRequestId) {
        loadingSession.value = false;
      }
    },
  };
}

function beginTreeRequest(intent: ViewIntent) {
  latestTreeRequestId += 1;
  const requestId = latestTreeRequestId;
  loadingEntries.value = true;

  return {
    isCurrent() {
      return isIntentCurrent(intent) && requestId === latestTreeRequestId;
    },
    finish() {
      if (requestId === latestTreeRequestId) {
        loadingEntries.value = false;
      }
    },
  };
}

function beginSearchRequest(intent: ViewIntent) {
  latestSearchRequestId += 1;
  const requestId = latestSearchRequestId;
  searching.value = true;

  return {
    isCurrent() {
      return isIntentCurrent(intent) && requestId === latestSearchRequestId;
    },
    finish() {
      if (requestId === latestSearchRequestId) {
        searching.value = false;
      }
    },
  };
}

function defaultUrlScope(segments: string[]): FileShareSearchScope {
  return segments.length > 0 ? 'current' : 'global';
}

function homeUrlState(): UrlState {
  return {
    segments: [],
    q: '',
    scope: 'global',
  };
}

function currentUrlSegments(): string[] {
  return breadcrumbs.value
    .filter((crumb) => crumb.node_id !== null)
    .map((crumb) => crumb.label);
}

function buildUrlState(segments: string[] = currentUrlSegments()): UrlState {
  const q = activeKeyword.value;

  return {
    segments,
    q,
    scope: q ? activeSearchScope.value : defaultUrlScope(segments),
  };
}

function writeUrlState(
  action: UrlAction,
  state: UrlState = buildUrlState(),
  intent?: ViewIntent,
) {
  if (intent && !isIntentCurrent(intent)) {
    return;
  }
  if (action === 'push') {
    pushPath(state);
    return;
  }
  if (action === 'replace') {
    replacePath(state);
  }
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

function clearSearchWithoutUrl() {
  resetSearchState(currentKind.value);
}

function captureSearchState(): SearchStateSnapshot {
  return {
    keyword: keyword.value,
    activeKeyword: activeKeyword.value,
    searchScope: searchScope.value,
    activeSearchScope: activeSearchScope.value,
    searchResults: [...searchResults.value],
  };
}

function restoreSearchState(snapshot: SearchStateSnapshot) {
  keyword.value = snapshot.keyword;
  activeKeyword.value = snapshot.activeKeyword;
  searchScope.value = snapshot.searchScope;
  activeSearchScope.value = snapshot.activeSearchScope;
  searchResults.value = [...snapshot.searchResults];
}

function canRestoreSearch(scope: FileShareSearchScope): boolean {
  if (scope === 'current') {
    return currentKind.value !== 'home' && Boolean(session.value?.permissions.search_current);
  }
  return Boolean(session.value?.permissions.search_global);
}

function isSameUrlState(left: UrlState, right: UrlState): boolean {
  return left.q === right.q
    && left.scope === right.scope
    && left.segments.length === right.segments.length
    && left.segments.every((segment, index) => segment === right.segments[index]);
}

function setFlash(message: string) {
  flashMessage.value = message;
  if (flashTimer) {
    clearTimeout(flashTimer);
  }
  if (!message) {
    return;
  }
  flashTimer = setTimeout(() => {
    flashMessage.value = '';
  }, 1800);
}

async function loadTree(
  nodeId: string | null = null,
  options: {
    preserveSearch?: boolean;
    allowHomeFallback?: boolean;
    urlAction?: UrlAction;
    intent?: ViewIntent;
  } = {},
) {
  const intent = resolveIntent(options.intent);
  if (!isIntentCurrent(intent)) {
    return;
  }
  const preserveSearch = options.preserveSearch ?? false;
  const allowHomeFallback = options.allowHomeFallback ?? true;
  const urlAction = options.urlAction ?? 'replace';
  const request = beginTreeRequest(intent);

  if (request.isCurrent()) {
    pageError.value = '';
  }

  try {
    const response = await fileShareApi.getTree(nodeId);
    if (!request.isCurrent()) {
      return;
    }
    tree.value = response;
    if (response.current.kind === 'home') {
      shareRoots.value = response.children.filter((entry) => entry.is_dir);
    }
    syncSearchScope(response.current.kind, preserveSearch);
  } catch (error) {
    if (!request.isCurrent()) {
      return;
    }
    if (isNotFound(error) && nodeId && allowHomeFallback) {
      resetSearchState('home');
      await loadTree(null, {
        preserveSearch: false,
        allowHomeFallback: false,
        urlAction: 'none',
        intent,
      });
      if (!isIntentCurrent(intent)) {
        return;
      }
      writeUrlState('replace', homeUrlState(), intent);
      pageError.value = t('app.directoryNotFound');
      return;
    }
    throw error;
  } finally {
    request.finish();
  }

  if (preserveSearch && activeKeyword.value) {
    await rerunSearch({
      urlAction: 'none',
      intent,
    });
  }

  if (!isIntentCurrent(intent)) {
    return;
  }

  if (urlAction !== 'none') {
    writeUrlState(urlAction, buildUrlState(), intent);
  }
}

async function loadSession(
  options: { clearViewOnFailure?: boolean; intent?: ViewIntent } = {},
): Promise<boolean> {
  const intent = resolveIntent(options.intent);
  if (!isIntentCurrent(intent)) {
    return false;
  }
  const clearViewOnFailure = options.clearViewOnFailure ?? true;
  const request = beginSessionRequest(intent);

  if (request.isCurrent()) {
    pageError.value = '';
  }

  try {
    const nextSession = await fileShareApi.getSession();
    if (!request.isCurrent()) {
      return false;
    }
    session.value = nextSession;
    loginOpen.value = false;
    loginError.value = '';
    return true;
  } catch (error) {
    if (!request.isCurrent()) {
      return false;
    }
    session.value = null;

    if (clearViewOnFailure) {
      tree.value = null;
      shareRoots.value = [];
      resetSearchState('home');
    }

    if (isUnauthorized(error)) {
      loginOpen.value = true;
      return false;
    }
    if (isForbidden(error)) {
      pageError.value = t('app.forbiddenIp');
      return false;
    }
    pageError.value = getErrorMessage(error);
    return false;
  } finally {
    request.finish();
  }
}

async function ensureShareRootsCached() {
  if (shareRoots.value.length > 0 || currentKind.value === 'home') {
    return;
  }
  try {
    const homeTree = await fileShareApi.getTree(null);
    shareRoots.value = homeTree.children.filter((entry) => entry.is_dir);
  } catch {
    /* Sidebar may stay empty if home tree is unavailable. */
  }
}

async function bootstrap(
  preferredNodeId: string | null = null,
  options: { preserveSearch?: boolean; intent?: ViewIntent } = {},
) {
  const intent = resolveIntent(options.intent);
  const hasSession = await loadSession({
    intent,
  });
  if (!isIntentCurrent(intent)) {
    return;
  }
  if (!hasSession) {
    return;
  }

  try {
    await loadTree(preferredNodeId, {
      preserveSearch: options.preserveSearch ?? false,
      intent,
    });
  } catch (error) {
    if (!isIntentCurrent(intent)) {
      return;
    }
    if (isUnauthorized(error)) {
      loginOpen.value = true;
      return;
    }
    pageError.value = getErrorMessage(error);
  }

  void ensureShareRootsCached();
}

async function executeSearch(
  rawKeyword: string,
  scope: FileShareSearchScope,
  options: {
    urlAction?: UrlAction;
    throwError?: boolean;
    showPageError?: boolean;
    intent?: ViewIntent;
  } = {},
) {
  const intent = resolveIntent(options.intent);
  if (!isIntentCurrent(intent)) {
    return;
  }
  const trimmed = rawKeyword.trim();
  if (!trimmed) {
    clearSearchWithoutUrl();
    return;
  }

  const effectiveScope = scope === 'current' && currentNodeId.value
    ? 'current'
    : 'global';
  const urlAction = options.urlAction ?? 'replace';
  const showPageError = options.showPageError ?? true;
  const request = beginSearchRequest(intent);

  if (request.isCurrent()) {
    pageError.value = '';
  }

  try {
    const response = await fileShareApi.search(
      trimmed,
      effectiveScope === 'current' ? currentNodeId.value : null,
    );
    if (!request.isCurrent()) {
      return;
    }
    keyword.value = trimmed;
    activeKeyword.value = trimmed;
    activeSearchScope.value = effectiveScope;
    searchScope.value = effectiveScope;
    searchResults.value = response.results;
    if (urlAction !== 'none') {
      writeUrlState(urlAction, buildUrlState(), intent);
    }
  } catch (error) {
    if (!request.isCurrent()) {
      return;
    }
    if (showPageError) {
      pageError.value = getErrorMessage(error);
    }
    if (options.throwError) {
      throw error;
    }
  } finally {
    request.finish();
  }
}

async function rerunSearch(
  options: {
    urlAction?: UrlAction;
    throwError?: boolean;
    showPageError?: boolean;
    intent?: ViewIntent;
  } = {},
) {
  if (!activeKeyword.value) {
    searchResults.value = [];
    return;
  }
  await executeSearch(activeKeyword.value, activeSearchScope.value, options);
}

async function handleSearch() {
  const intent = startViewIntent();
  if (!keyword.value.trim()) {
    clearSearch(intent);
    return;
  }
  await executeSearch(keyword.value, searchScope.value, {
    intent,
  });
}

function canonicalStateFromSegments(segments: string[]): UrlState {
  return {
    segments,
    q: activeKeyword.value,
    scope: activeKeyword.value ? activeSearchScope.value : defaultUrlScope(segments),
  };
}

async function fallbackToHomeFromUrlError(message: string, intent: ViewIntent) {
  try {
    resetSearchState('home');
    await loadTree(null, {
      preserveSearch: false,
      allowHomeFallback: false,
      urlAction: 'none',
      intent,
    });
    if (!isIntentCurrent(intent)) {
      return;
    }
    replacePath(homeUrlState());
    pageError.value = message;
  } catch (error) {
    if (!isIntentCurrent(intent)) {
      return;
    }
    if (isUnauthorized(error)) {
      loginOpen.value = true;
      return;
    }
    pageError.value = getErrorMessage(error);
  }
}

async function handleUrlStateError(error: unknown, intent: ViewIntent) {
  if (!isIntentCurrent(intent)) {
    return;
  }
  if (isUnauthorized(error)) {
    loginOpen.value = true;
    return;
  }
  if (isForbidden(error)) {
    await fallbackToHomeFromUrlError(t('app.forbiddenDirectory'), intent);
    return;
  }
  if (isNotFound(error)) {
    await fallbackToHomeFromUrlError(t('app.directoryNotFound'), intent);
    return;
  }
  pageError.value = getErrorMessage(error);
}

async function applyUrlState(
  state: UrlState,
  options: { skipSession?: boolean; intent?: ViewIntent } = {},
) {
  const intent = resolveIntent(options.intent);
  if (!isIntentCurrent(intent)) {
    return;
  }
  if (!options.skipSession) {
    const hasSession = await loadSession({
      clearViewOnFailure: false,
      intent,
    });
    if (!isIntentCurrent(intent)) {
      return;
    }
    if (!hasSession) {
      return;
    }
  }

  if (isIntentCurrent(intent)) {
    pageError.value = '';
  }

  let nodeId: string | null = null;
  let canonicalSegments: string[] = [];

  if (state.segments.length > 0) {
    const resolved = await fileShareApi.resolvePath(state.segments);
    if (!isIntentCurrent(intent)) {
      return;
    }
    nodeId = resolved.node_id;
    canonicalSegments = resolved.canonical_segments;
  }

  await loadTree(nodeId, {
    preserveSearch: false,
    allowHomeFallback: false,
    urlAction: 'none',
    intent,
  });
  if (!isIntentCurrent(intent)) {
    return;
  }

  const resolvedSegments = canonicalSegments.length > 0
    ? canonicalSegments
    : currentUrlSegments();
  const restoredScope: FileShareSearchScope = state.scope === 'current' && nodeId
    ? 'current'
    : 'global';

  if (state.q) {
    keyword.value = state.q;

    if (canRestoreSearch(restoredScope)) {
      await executeSearch(state.q, restoredScope, {
        urlAction: 'none',
        throwError: true,
        showPageError: false,
        intent,
      });
    } else {
      clearSearchWithoutUrl();
    }
  } else {
    clearSearchWithoutUrl();
  }

  if (!isIntentCurrent(intent)) {
    return;
  }

  replacePath(canonicalStateFromSegments(resolvedSegments));

  void ensureShareRootsCached();
}

async function bootstrapFromUrl(state: UrlState) {
  const intent = startViewIntent();
  const hasSession = await loadSession({
    intent,
  });
  if (!isIntentCurrent(intent)) {
    return;
  }
  if (!hasSession) {
    return;
  }

  try {
    await applyUrlState(state, {
      skipSession: true,
      intent,
    });
  } catch (error) {
    await handleUrlStateError(error, intent);
  }
}

async function handleExternalUrlChange(state: UrlState) {
  const intent = startViewIntent();
  try {
    await applyUrlState(state, {
      skipSession: true,
      intent,
    });
  } catch (error) {
    await handleUrlStateError(error, intent);
  }
}

function clearSearch(intent: ViewIntent = startViewIntent()) {
  clearSearchWithoutUrl();
  writeUrlState('replace', buildUrlState(), intent);
}

async function handleLogin(payload: { username: string; password: string }) {
  loggingIn.value = true;
  loginError.value = '';

  try {
    await fileShareApi.login(payload.username, payload.password);
    await bootstrapFromUrl(parseHash());
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
    const intent = startViewIntent();
    const searchSnapshot = captureSearchState();
    clearSearchWithoutUrl();
    try {
      await loadTree(node.node_id, {
        allowHomeFallback: false,
        urlAction: 'push',
        intent,
      });
    } catch (error) {
      if (isIntentCurrent(intent)) {
        restoreSearchState(searchSnapshot);
        if (isUnauthorized(error)) {
          loginOpen.value = true;
        } else {
          pageError.value = getErrorMessage(error);
        }
      }
    }
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
  const intent = startViewIntent();
  const searchSnapshot = captureSearchState();
  clearSearchWithoutUrl();
  try {
    await loadTree(nodeId, {
      allowHomeFallback: false,
      urlAction: 'push',
      intent,
    });
  } catch (error) {
    if (isIntentCurrent(intent)) {
      restoreSearchState(searchSnapshot);
      if (isUnauthorized(error)) {
        loginOpen.value = true;
      } else {
        pageError.value = getErrorMessage(error);
      }
    }
  }
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
    setFlash(uploadMode.value === 'files'
      ? t('app.uploadFilesSuccess')
      : t('app.uploadDirectorySuccess'));
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
    setFlash(t('app.createDirectorySuccess'));
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
    setFlash(t('app.createTextSuccess'));
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
    setFlash(t('app.renameSuccess'));
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
    setFlash(t('app.deleteSuccess'));
    await refreshCurrentView(fallbackNodeId, {
      preserveSearch: searchActive.value && fallbackNodeId !== null,
    });
  } catch (error) {
    await handleMutationError(error, deleteError);
  } finally {
    mutating.value = false;
  }
}

function toggleSelect(nodeId: string) {
  const next = new Set(selectedIds.value);
  if (next.has(nodeId)) {
    next.delete(nodeId);
  } else {
    next.add(nodeId);
  }
  selectedIds.value = next;
}

function selectAll() {
  const visible = displayedEntries.value;
  if (visible.length === 0) {
    return;
  }
  const allSelected = visible.every((entry) => selectedIds.value.has(entry.node_id));
  selectedIds.value = allSelected
    ? new Set()
    : new Set(visible.map((entry) => entry.node_id));
}

function clearSelection() {
  if (selectedIds.value.size === 0) {
    return;
  }
  selectedIds.value = new Set();
}

function bulkDownload() {
  const items = selectedEntries.value;
  if (items.length === 0) {
    return;
  }
  for (const entry of items) {
    triggerDownload(entry);
  }
  setFlash(t('app.bulkDownloadStarted', { n: items.length }));
  clearSelection();
}

async function bulkDelete() {
  const items = selectedEntries.value;
  if (items.length === 0) {
    return;
  }
  const confirmed = typeof window === 'undefined'
    ? false
    : window.confirm(t('app.bulkDeleteConfirm', { n: items.length }));
  if (!confirmed) {
    return;
  }

  mutating.value = true;
  pageError.value = '';
  let succeeded = 0;
  let lastError: unknown = null;

  for (const entry of items) {
    try {
      await fileShareApi.remove(entry.node_id);
      succeeded += 1;
    } catch (error) {
      lastError = error;
      break;
    }
  }

  mutating.value = false;

  if (succeeded > 0) {
    setFlash(t('app.bulkDeleteResult', { n: succeeded }));
  }
  clearSelection();
  await refreshCurrentView(currentNodeId.value, {
    preserveSearch: searchActive.value,
  });

  if (lastError) {
    if (isUnauthorized(lastError)) {
      loginOpen.value = true;
    } else if (isForbidden(lastError)) {
      pageError.value = t('app.permissionChanged');
    } else {
      pageError.value = getErrorMessage(lastError);
    }
  }
}

function handleDownloadAll() {
  if (!tree.value || !currentNodeId.value) {
    return;
  }
  const archiveTarget: FileShareNode = {
    node_id: currentNodeId.value,
    parent_id: null,
    kind: currentKind.value === 'share_root' ? 'share_root' : 'directory',
    name: currentName.value || 'archive',
    root_id: '',
    root_alias: '',
    relative_path: '',
    display_path: currentName.value || '',
    is_dir: true,
    size: null,
    modified: null,
    permissions: {
      browse: true,
      download_file: false,
      download_archive: true,
      upload_file: false,
      upload_directory: false,
      create_directory: false,
      create_text: false,
      rename: false,
      delete: false,
      preview_image: false,
      search_current: false,
      search_global: false,
    },
  };
  triggerDownload(archiveTarget);
  setFlash(t('app.downloadAllStarted'));
}

function setView(next: EntryViewMode) {
  if (view.value === next) {
    return;
  }
  view.value = next;
  saveViewMode(next);
}

function handleSort(key: EntrySortKey) {
  if (sortKey.value === key) {
    sortDirection.value = sortDirection.value === 'asc' ? 'desc' : 'asc';
  } else {
    sortKey.value = key;
    sortDirection.value = key === 'name' ? 'asc' : 'desc';
  }
  saveSortPreference({ key: sortKey.value, direction: sortDirection.value });
}

watch([currentNodeId, activeKeyword, activeSearchScope], () => {
  clearSelection();
});

watch(currentNodeId, (next) => {
  if (!next || currentKind.value === 'home') {
    return;
  }
  const label = currentName.value || (breadcrumbs.value.at(-1)?.label ?? '');
  if (!label) {
    return;
  }
  recentPaths.value = recordRecentPath({ node_id: next, label });
});

onMounted(() => {
  unmounted = false;
  startSessionHeartbeat();
  const initialState = parseHash();

  void (async () => {
    await bootstrapFromUrl(initialState);
    if (unmounted) {
      return;
    }

    const currentHash = typeof window === 'undefined' ? '' : window.location.hash;
    unsubscribeFromUrl = subscribe((state) => {
      void handleExternalUrlChange(state);
    }, {
      initialHash: currentHash,
    });

    if (!session.value) {
      return;
    }

    const currentState = parseHash(currentHash);
    if (!isSameUrlState(currentState, buildUrlState())) {
      void handleExternalUrlChange(currentState);
    }
  })();
});

onUnmounted(() => {
  unmounted = true;
  invalidateViewLifecycle();
  unsubscribeFromUrl?.();
  unsubscribeFromUrl = null;
  if (sessionHeartbeatTimer) {
    clearInterval(sessionHeartbeatTimer);
    sessionHeartbeatTimer = null;
  }
  if (flashTimer) {
    clearTimeout(flashTimer);
    flashTimer = null;
  }
});

watchEffect(() => {
  document.title = t('app.pageTitle');
});
</script>

<template>
  <div class="app">
    <TopBar
      :session="session"
      :busy="busy"
      @refresh="refreshCurrentView()"
      @session-action="handleSessionAction"
    />

    <Sidebar
      :share-roots="shareRoots"
      :current-kind="currentKind"
      :active-root-node-id="activeRootNodeId"
      :recent="recentPaths"
      :busy="busy"
      @navigate="navigate"
    />

    <main class="main">
      <Breadcrumbs :breadcrumbs="breadcrumbs" :busy="busy" @navigate="navigate" />

      <div class="page-head">
        <div>
          <h1 class="page-title">
            {{ pageTitle }}
            <span v-if="pageStatLabel" class="sub">{{ pageStatLabel }}</span>
          </h1>
          <div v-if="pageSubText" class="page-sub">{{ pageSubText }}</div>
        </div>
        <ToolbarActions
          :current-kind="currentKind"
          :permissions="session?.permissions ?? null"
          :has-entries="displayedEntries.length > 0"
          :busy="busy"
          @upload-files="openUpload('files')"
          @upload-directory="openUpload('directory')"
          @create-directory="openCreateDirectoryDialog"
          @create-text="newTextOpen = true"
          @download-all="handleDownloadAll"
        />
      </div>

      <div v-if="showGuestNotice" class="notice">
        <span class="ico"><Icon name="info" /></span>
        <span class="body">{{ t('app.guestModeNotice') }}</span>
        <button type="button" class="close" :aria-label="t('app.dismissNotice')" @click="guestNoticeOpen = false">
          {{ t('app.dismiss') }}
        </button>
      </div>

      <div v-if="pageError" class="notice danger">
        <span class="ico"><Icon name="info" /></span>
        <span class="body">{{ pageError }}</span>
        <button type="button" class="close" :aria-label="t('app.dismissNotice')" @click="pageError = ''">
          {{ t('app.dismiss') }}
        </button>
      </div>

      <SearchBar
        :keyword="keyword"
        :scope="searchScope"
        :view="view"
        :can-search-current="canSearchCurrent"
        :can-search-global="canSearchGlobal"
        :busy="busy"
        @update:keyword="keyword = $event"
        @update:scope="searchScope = $event"
        @update:view="setView"
        @search="handleSearch"
        @clear="clearSearch"
      />

      <EntryTable
        :entries="displayedEntries"
        :session="session"
        :loading="loadingEntries || searching || loadingSession"
        :empty-text="emptyText"
        :search-active="searchActive"
        :view="view"
        :selected-ids="selectedIds"
        :sort-key="sortKey"
        :sort-direction="sortDirection"
        @open="openEntry"
        @preview="openPreview"
        @download="triggerDownload"
        @rename="openRename"
        @delete="openDelete"
        @toggle-select="toggleSelect"
        @select-all="selectAll"
        @sort="handleSort"
      />
    </main>

    <BulkActionBar
      v-if="selectedIds.size > 0"
      :count="selectedIds.size"
      :can-download="canBulkDownload"
      :can-delete="canBulkDelete"
      :busy="busy"
      @download-all="bulkDownload"
      @delete-all="bulkDelete"
      @clear="clearSelection"
    />

    <Flash :message="flashMessage" />

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
