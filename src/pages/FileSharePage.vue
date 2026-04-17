<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  Copy,
  ExternalLink,
  KeyRound,
  Play,
  Plus,
  Power,
  QrCode,
  RefreshCw,
  Save,
  Share2,
  Trash2,
  ChevronDown,
  ChevronUp,
} from 'lucide-vue-next';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import QRCode from 'qrcode';
import {
  fileShareGetStatus,
  fileShareLoadSettings,
  fileSharePickDirectory,
  fileShareSaveSettings,
  fileShareStartSaved,
  fileShareStop,
  getConfig,
  saveConfig,
  type AppConfig,
  type FileShareDeleteMode,
  type FileShareIpFilterMode,
  type FileSharePermissionPreset,
  type FileSharePermissionSet,
  type FileShareRoot,
  type FileShareSettingsSaveRequest,
  type FileShareSettingsView,
  type FileShareStatus,
  type FileShareUserRootPermissions,
  type FileShareUserSaveRequest,
  type FileShareUserView,
} from '../lib/tauri';

defineOptions({ name: 'FileSharePage' });

const MAX_SESSION_TTL_MINUTES = 7 * 24 * 60;
type PermKey = keyof FileSharePermissionSet;
type EditUser = FileShareUserView & {
  draft_key: string;
  previous_username: string | null;
  new_password: string;
  clear_password: boolean;
};
type Draft = Omit<FileShareSettingsView, 'guest_account' | 'accounts'> & {
  guest_account: EditUser;
  accounts: EditUser[];
};

const { t } = useI18n();
let draftKeySeed = 0;

const permDefs: PermKey[] = [
  'browse',
  'download_file',
  'download_archive',
  'upload_file',
  'upload_directory',
  'create_directory',
  'create_text',
  'rename',
  'delete',
  'preview_image',
  'search_current',
  'search_global',
];

const readOnly = (): FileSharePermissionSet => ({
  browse: true,
  download_file: true,
  download_archive: true,
  upload_file: false,
  upload_directory: false,
  create_directory: false,
  create_text: false,
  rename: false,
  delete: false,
  preview_image: true,
  search_current: true,
  search_global: true,
});

const readWrite = (): FileSharePermissionSet => ({
  browse: true,
  download_file: true,
  download_archive: true,
  upload_file: true,
  upload_directory: true,
  create_directory: true,
  create_text: true,
  rename: true,
  delete: true,
  preview_image: true,
  search_current: true,
  search_global: true,
});

const clonePerms = (v: FileSharePermissionSet): FileSharePermissionSet => ({ ...v });
const permsForPreset = (preset: FileSharePermissionPreset) => (preset === 'read_write' ? readWrite() : readOnly());
const cloneRootPerms = (entry: FileShareUserRootPermissions): FileShareUserRootPermissions => ({
  root_id: entry.root_id,
  preset: entry.preset,
  permissions: clonePerms(entry.permissions),
});
const permissionLabel = (key: PermKey) => t(`tools.fileShare.permissions.${key}`);
const nextDraftKey = () => `file-share-user-${draftKeySeed++}`;
const guestView = (): FileShareUserView => ({
  username: t('tools.fileShare.defaultGuestUsername'),
  enabled: true,
  root_permissions: [],
  password_set: false,
});
const editUser = (a: FileShareUserView): EditUser => ({
  ...a,
  draft_key: nextDraftKey(),
  previous_username: a.username,
  root_permissions: a.root_permissions.map(cloneRootPerms),
  new_password: '',
  clear_password: false,
});

const findRootPerm = (user: EditUser, rootId: string): FileShareUserRootPermissions | undefined =>
  user.root_permissions.find((p) => p.root_id === rootId);

const rootAccessRows = (user: EditUser) =>
  draft.value.roots.map((root) => ({
    root,
    entry: user.root_permissions.find((p) => p.root_id === root.id) ?? null,
  }));

const toggleRootAccess = (user: EditUser, rootId: string, grant: boolean) => {
  const existing = findRootPerm(user, rootId);
  if (grant) {
    if (existing) return;
    user.root_permissions.push({
      root_id: rootId,
      preset: 'read_only',
      permissions: readOnly(),
    });
  } else if (existing) {
    user.root_permissions = user.root_permissions.filter((p) => p.root_id !== rootId);
  }
};

const onRootPreset = (entry: FileShareUserRootPermissions) => {
  if (entry.preset !== 'custom') entry.permissions = permsForPreset(entry.preset);
};

const onRootAccessChange = (user: EditUser, rootId: string, ev: Event) => {
  const target = ev.target as HTMLInputElement;
  toggleRootAccess(user, rootId, target.checked);
};

const onRootPresetChange = (user: EditUser, rootId: string, ev: Event) => {
  const entry = findRootPerm(user, rootId);
  if (!entry) return;
  const target = ev.target as HTMLSelectElement;
  entry.preset = target.value as FileSharePermissionPreset;
  onRootPreset(entry);
};

const blankStatus = (): FileShareStatus => ({
  is_active: false,
  connection_count: 0,
  uptime_secs: 0,
  server_url: '',
  all_urls: [],
  shared_dirs: [],
  connected_ips: [],
});

let lastUptimeUpdate = 0;

const blankDraft = (): Draft => ({
  port: 8080,
  roots: [],
  guest_access_enabled: true,
  guest_account: editUser(guestView()),
  accounts: [],
  session_ttl_minutes: 30,
  ip_filter_mode: 'off',
  ip_rules: [],
  image_preview_enabled: true,
  thumbnail_enabled: false,
  delete_mode: 'recycle_bin',
  remember_settings: true,
  auto_start_on_page_open: false,
  auto_start_with_windows: false,
});

const toDraft = (view: FileShareSettingsView, cfg: AppConfig | null): Draft => {
  return {
    ...view,
    guest_account: editUser({
      ...view.guest_account,
      enabled: view.guest_access_enabled,
    }),
    roots: view.roots.map((r) => ({ ...r })),
    accounts: view.accounts.map(editUser),
    ip_rules: [...view.ip_rules],
    auto_start_with_windows: cfg?.launch_and_auto_start_file_share ?? view.auto_start_with_windows,
  };
};

const slug = (s: string, fallback: string) =>
  s.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || fallback;

const buildReq = (d: Draft): FileShareSettingsSaveRequest => ({
  port: d.port,
  roots: d.roots.map((r) => ({ id: r.id.trim(), alias: r.alias.trim(), path: r.path.trim(), enabled: r.enabled })),
  guest_access_enabled: d.guest_access_enabled,
  guest_account: {
    username: d.guest_account.username.trim(),
    enabled: d.guest_access_enabled,
    root_permissions: d.guest_account.root_permissions.map(cloneRootPerms),
    previous_username: d.guest_account.previous_username,
    new_password: d.guest_account.new_password.trim() || null,
    clear_password: d.guest_account.clear_password,
  },
  accounts: d.accounts.map((a): FileShareUserSaveRequest => ({
    username: a.username.trim(),
    enabled: a.enabled,
    root_permissions: a.root_permissions.map(cloneRootPerms),
    previous_username: a.previous_username,
    new_password: a.new_password.trim() || null,
    clear_password: a.clear_password,
  })),
  session_ttl_minutes: d.session_ttl_minutes,
  ip_filter_mode: d.ip_filter_mode,
  ip_rules: d.ip_rules.map((x) => x.trim()).filter(Boolean),
  image_preview_enabled: d.image_preview_enabled,
  thumbnail_enabled: d.thumbnail_enabled,
  delete_mode: d.delete_mode,
  remember_settings: d.remember_settings,
  auto_start_on_page_open: d.auto_start_on_page_open,
  auto_start_with_windows: d.auto_start_with_windows,
});

const draft = ref<Draft>(blankDraft());
const status = ref<FileShareStatus>(blankStatus());
const appConfig = ref<AppConfig | null>(null);
const notice = ref('');
const errorMsg = ref('');
const newIpRule = ref('');
const isLoading = ref(true);
const isSaving = ref(false);
const isApplying = ref(false);
const isActive = ref(false);
const serverUrl = ref('');
const copied = ref(false);
const showQr = ref(false);
const showAltUrls = ref(false);
const showConnections = ref(true);
const showAllIps = ref(false);
const qrCanvas = ref<HTMLCanvasElement | null>(null);
const logs = ref<{ level: string; message: string; time: string }[]>([]);

const guest = computed(() => draft.value.guest_account);
const customAccounts = computed(() => draft.value.accounts);
const enabledRoots = computed(() => draft.value.roots.filter((r) => r.enabled));
const enabledCustomAccounts = computed(() => customAccounts.value.filter((a) => a.enabled));
const altUrls = computed(() => (status.value.all_urls ?? []).filter((u) => u !== serverUrl.value));
const connectedIps = computed(() => status.value.connected_ips ?? []);
const connCount = computed(() => status.value.connection_count ?? connectedIps.value.length);
const visibleIps = computed(() => showAllIps.value ? connectedIps.value : connectedIps.value.slice(0, 10));
const hiddenIpCount = computed(() => Math.max(0, connectedIps.value.length - 10));
const uptime = computed(() => {
  const sec = status.value.uptime_secs;
  return `${String(Math.floor(sec / 3600)).padStart(2, '0')}:${String(Math.floor((sec % 3600) / 60)).padStart(2, '0')}:${String(sec % 60).padStart(2, '0')}`;
});
const presetOpts = computed(() => [
  { value: 'read_only' as FileSharePermissionPreset, label: t('tools.fileShare.presetReadOnly') },
  { value: 'read_write' as FileSharePermissionPreset, label: t('tools.fileShare.presetReadWrite') },
  { value: 'custom' as FileSharePermissionPreset, label: t('tools.fileShare.presetCustom') },
]);
const ipOpts = computed(() => [
  { value: 'off' as FileShareIpFilterMode, label: t('tools.fileShare.ipFilterOff') },
  { value: 'whitelist' as FileShareIpFilterMode, label: t('tools.fileShare.ipFilterWhitelist') },
  { value: 'blacklist' as FileShareIpFilterMode, label: t('tools.fileShare.ipFilterBlacklist') },
]);
const delOpts = computed(() => [
  { value: 'recycle_bin' as FileShareDeleteMode, label: t('tools.fileShare.deleteRecycleBin') },
  { value: 'permanent' as FileShareDeleteMode, label: t('tools.fileShare.deletePermanent') },
]);

const errors = computed(() => {
  const out: string[] = [];
  if (draft.value.port < 1024 || draft.value.port > 65535) out.push(t('tools.fileShare.validation.portRange'));
  if (draft.value.session_ttl_minutes < 1 || draft.value.session_ttl_minutes > MAX_SESSION_TTL_MINUTES) {
    out.push(t('tools.fileShare.validation.sessionTtlRange', { max: MAX_SESSION_TTL_MINUTES }));
  }
  if (draft.value.ip_filter_mode !== 'off' && draft.value.ip_rules.length === 0) out.push(t('tools.fileShare.validation.ipRuleRequired'));
  const rootAliases = new Set<string>();
  const rootPaths = new Set<string>();
  for (const root of draft.value.roots) {
    if (!root.alias.trim()) out.push(t('tools.fileShare.validation.rootAliasRequired'));
    if (!root.path.trim()) out.push(t('tools.fileShare.validation.rootPathRequired'));
    const ak = root.alias.trim().toLowerCase();
    const pk = root.path.trim().toLowerCase();
    if (rootAliases.has(ak)) out.push(t('tools.fileShare.validation.duplicateRootAlias', { value: root.alias }));
    if (rootPaths.has(pk)) out.push(t('tools.fileShare.validation.duplicateRootPath', { value: root.path }));
    rootAliases.add(ak);
    rootPaths.add(pk);
  }
  const usernames = new Set<string>();
  if (!draft.value.guest_account.username.trim()) {
    out.push(t('tools.fileShare.validation.usernameRequired'));
  } else {
    usernames.add(draft.value.guest_account.username.trim().toLowerCase());
  }
  for (const a of draft.value.accounts) {
    if (!a.username.trim()) out.push(t('tools.fileShare.validation.usernameRequired'));
    const key = a.username.trim().toLowerCase();
    if (usernames.has(key)) out.push(t('tools.fileShare.validation.duplicateUsername', { value: a.username }));
    usernames.add(key);
  }
  return [...new Set(out)];
});

const lockoutWarn = computed(() => !draft.value.guest_access_enabled && enabledCustomAccounts.value.length === 0);
const canSave = computed(() => !isLoading.value && !isSaving.value && errors.value.length === 0);
const canStart = computed(() => canSave.value && enabledRoots.value.length > 0 && !isApplying.value);
const formDisabled = computed(() => isLoading.value || isSaving.value || isApplying.value);

const stamp = () => {
  const n = new Date();
  return `${String(n.getHours()).padStart(2, '0')}:${String(n.getMinutes()).padStart(2, '0')}:${String(n.getSeconds()).padStart(2, '0')}`;
};

const setStatus = (next: FileShareStatus) => {
  if (next.is_active && next.uptime_secs < lastUptimeUpdate && lastUptimeUpdate - next.uptime_secs > 1) {
    return;
  }
  if (next.is_active) {
    lastUptimeUpdate = next.uptime_secs;
  }
  status.value = { ...blankStatus(), ...next, all_urls: next.all_urls ?? [], shared_dirs: next.shared_dirs ?? [], connected_ips: next.connected_ips ?? [] };
  isActive.value = status.value.is_active;
  serverUrl.value = status.value.server_url;
  if (!status.value.is_active) {
    showQr.value = false;
    showConnections.value = false;
    showAllIps.value = false;
    lastUptimeUpdate = 0;
  }
};

const saveStartup = async (enabled: boolean) => {
  const base = appConfig.value ?? (await getConfig());
  if (base.launch_and_auto_start_file_share === enabled) return;
  const next = { ...base, launch_and_auto_start_file_share: enabled };
  await saveConfig(next);
  appConfig.value = next;
};

const saveSettings = async (msg = t('tools.fileShare.settingsSaved')) => {
  if (errors.value.length > 0) {
    errorMsg.value = errors.value[0];
    return false;
  }
  isSaving.value = true;
  errorMsg.value = '';
  try {
    const saved = await fileShareSaveSettings(buildReq(draft.value));
    await saveStartup(draft.value.auto_start_with_windows);
    draft.value = toDraft(saved, appConfig.value);
    notice.value = msg;
    return true;
  } catch (e) {
    errorMsg.value = String(e);
    return false;
  } finally {
    isSaving.value = false;
  }
};

const startShare = async (restart = false, persist = true, msg?: string) => {
  if (enabledRoots.value.length === 0) {
    errorMsg.value = t('tools.fileShare.startRequiresRoot');
    return;
  }
  isApplying.value = true;
  errorMsg.value = '';
  try {
    if (persist) {
      const ok = await saveSettings(restart ? t('tools.fileShare.settingsSavedRestarting') : t('tools.fileShare.settingsSavedStarting'));
      if (!ok) return;
    }
    if (restart && isActive.value) {
      try {
        await fileShareStop();
      } catch {
        /* Ignore stop races during restart. */
      }
    }
    serverUrl.value = await fileShareStartSaved();
    try {
      setStatus(await fileShareGetStatus());
    } catch {
      /* Status refresh is best-effort after start. */
    }
    isActive.value = true;
    notice.value = msg ?? (restart ? t('tools.fileShare.restartSuccess') : t('tools.fileShare.startSuccess'));
  } catch (e) {
    errorMsg.value = t('tools.fileShare.errStartFailed', { error: String(e) });
  } finally {
    isApplying.value = false;
  }
};

const stopShare = async () => {
  isApplying.value = true;
  errorMsg.value = '';
  try {
    await fileShareStop();
    setStatus(blankStatus());
    notice.value = t('tools.fileShare.stopSuccess');
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    isApplying.value = false;
  }
};

const addRoot = async (target?: FileShareRoot) => {
  try {
    const dir = await fileSharePickDirectory();
    if (!dir) return;
    if (!target && draft.value.roots.some((r) => r.path === dir.path)) {
      notice.value = t('tools.fileShare.duplicateRootNotice');
      return;
    }
    if (target) {
      target.path = dir.path;
      if (!target.alias.trim()) target.alias = dir.alias;
      return;
    }
    const used = new Set(draft.value.roots.map((r) => r.id));
    let id = slug(dir.alias, 'root');
    let n = 2;
    while (used.has(id)) id = `${slug(dir.alias, 'root')}-${n++}`;
    draft.value.roots.push({ id, alias: dir.alias, path: dir.path, enabled: true });
  } catch (e) {
    errorMsg.value = String(e);
  }
};

const addAccount = () => {
  const base = slug(t('tools.fileShare.newAccountDefaultUsername'), 'user');
  const used = new Set([
    draft.value.guest_account.username.trim().toLowerCase(),
    ...draft.value.accounts.map((a) => a.username.trim().toLowerCase()),
  ]);
  let username = base;
  let n = 2;
  while (used.has(username)) username = `${base}-${n++}`;
  draft.value.accounts.push(editUser({
    username,
    enabled: true,
    root_permissions: [],
    password_set: false,
  }));
  draft.value.accounts[draft.value.accounts.length - 1].previous_username = null;
};

const onPassword = (a: EditUser) => {
  if (a.new_password.trim()) a.clear_password = false;
};

const onClear = (a: EditUser) => {
  if (a.clear_password) a.new_password = '';
};

const copy = async (text: string) => {
  try {
    await navigator.clipboard.writeText(text);
    copied.value = text === serverUrl.value;
    if (copied.value) setTimeout(() => { copied.value = false; }, 1800);
  } catch {
    /* Clipboard access can fail in restricted environments. */
  }
};

const openBrowser = async () => {
  if (!serverUrl.value) return;
  try {
    await invoke('open_url', { url: serverUrl.value });
  } catch {
    /* Opening the system browser is best-effort. */
  }
};

watch(
  () => draft.value.roots.map((r) => r.id).join('|'),
  () => {
    const validIds = new Set(draft.value.roots.map((r) => r.id));
    const prune = (user: EditUser) => {
      user.root_permissions = user.root_permissions.filter((p) => validIds.has(p.root_id));
    };
    prune(draft.value.guest_account);
    draft.value.accounts.forEach(prune);
  },
);

watch([showQr, serverUrl], async ([show, url]) => {
  if (!show || !url || !qrCanvas.value) return;
  await nextTick();
  await QRCode.toCanvas(qrCanvas.value, url, { width: 128, margin: 1, color: { dark: '#0f766e', light: '#ffffff' } });
});

let offStatus: UnlistenFn | null = null;
let offLog: UnlistenFn | null = null;

onMounted(async () => {
  offStatus = await listen<FileShareStatus>('file-share-status', (e) => setStatus(e.payload));
  offLog = await listen<{ level: string; message: string }>('file-share-log', (e) => {
    logs.value.unshift({ level: e.payload.level, message: e.payload.message, time: stamp() });
    if (logs.value.length > 50) logs.value.length = 50;
  });
  try {
    const [view, cfg, current] = await Promise.all([fileShareLoadSettings(), getConfig(), fileShareGetStatus()]);
    appConfig.value = cfg;
    draft.value = toDraft(view, cfg);
    setStatus(current);
    if (draft.value.auto_start_on_page_open && !current.is_active && draft.value.roots.some((r) => r.enabled)) {
      await startShare(false, false, t('tools.fileShare.autoStartedOnPageOpen'));
    }
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    isLoading.value = false;
  }
});

onUnmounted(() => {
  offStatus?.();
  offLog?.();
});
</script>

<template>
  <div class="flex flex-1 flex-col overflow-y-auto bg-gradient-to-br from-slate-50 to-teal-50/40">
    <div class="mx-auto flex w-full max-w-7xl flex-col gap-5 p-6 pb-10">
      <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
        <div class="flex items-start gap-3">
          <div class="relative flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-teal-500 to-cyan-600 shadow-sm">
            <Share2 class="h-5 w-5 text-white" />
            <span v-if="isActive" class="absolute -right-1 -top-1 flex h-3.5 w-3.5 items-center justify-center rounded-full border-2 border-white bg-emerald-500">
              <span class="h-1.5 w-1.5 rounded-full bg-white"></span>
            </span>
          </div>
          <div>
            <h1 class="text-2xl font-bold text-slate-900">{{ t('sidebar.fileShare') }}</h1>
            <p class="mt-1 text-sm text-slate-500">
              {{ isActive ? serverUrl : t('tools.fileShare.consoleDescription') }}
            </p>
          </div>
        </div>
        <div class="inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-xs font-semibold shadow-sm" :class="isActive ? 'border-emerald-200 bg-emerald-50 text-emerald-700' : 'border-slate-200 bg-white text-slate-500'">
          <span class="h-2 w-2 rounded-full" :class="isActive ? 'bg-emerald-500 animate-pulse' : 'bg-slate-300'"></span>
          {{ isActive ? t('tools.fileShare.statusActive') : t('tools.fileShare.statusIdle') }}
        </div>
      </div>

      <div class="grid grid-cols-1 gap-5 lg:grid-cols-[minmax(0,1.7fr)_minmax(320px,380px)]">
        <div class="space-y-4">
          <div class="fs-card">
            <div class="mb-4 flex items-center justify-between gap-3">
              <div>
                <p class="fs-label-sm">{{ t('tools.fileShare.sharedRootsTitle') }}</p>
                <p class="text-sm text-slate-500">{{ t('tools.fileShare.sharedRootsDescription') }}</p>
              </div>
              <button type="button" :disabled="formDisabled" @click="addRoot()" class="fs-btn fs-btn-soft">
                <Plus class="h-4 w-4" />{{ t('tools.fileShare.addDir') }}
              </button>
            </div>
            <div v-if="draft.roots.length === 0" class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-4 py-8 text-center text-sm text-slate-500">
              {{ t('tools.fileShare.noDirs') }}
            </div>
            <div v-else class="fs-root-list">
              <div v-for="(root, index) in draft.roots" :key="root.id" class="fs-root-row">
                <div class="fs-root-row-top">
                  <div class="min-w-0">
                    <label class="fs-label">{{ t('tools.fileShare.aliasLabel') }}</label>
                    <input v-model="root.alias" :disabled="formDisabled" class="fs-input w-full" />
                  </div>

                  <div class="fs-root-actions">
                    <label class="fs-inline-toggle">
                      <span class="fs-toggle">
                        <input v-model="root.enabled" type="checkbox" :disabled="formDisabled" class="sr-only">
                        <span class="fs-toggle-track" :class="root.enabled ? 'bg-teal-600' : 'bg-slate-300'"><span class="fs-toggle-thumb" :class="root.enabled ? 'translate-x-4' : 'translate-x-0'"></span></span>
                      </span>
                      <span>{{ t('tools.fileShare.enabledLabel') }}</span>
                    </label>

                    <button type="button" :disabled="formDisabled" @click="addRoot(root)" class="fs-btn fs-btn-plain fs-btn-compact">
                      {{ t('tools.fileShare.changePath') }}
                    </button>

                    <button type="button" :disabled="formDisabled" @click="draft.roots.splice(index, 1)" class="fs-btn fs-btn-danger fs-btn-icon">
                      <Trash2 class="h-4 w-4" />
                    </button>
                  </div>
                </div>

                <div class="fs-root-path" :title="root.path">{{ root.path }}</div>
              </div>
            </div>
          </div>

          <div class="fs-card">
            <p class="fs-label-sm">{{ t('tools.fileShare.generalSettingsTitle') }}</p>
            <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
              <div>
                <label class="fs-label">{{ t('tools.fileShare.port') }}</label>
                <input v-model.number="draft.port" type="number" min="1024" max="65535" :disabled="formDisabled" class="fs-input w-full" />
              </div>
              <div>
                <label class="fs-label">{{ t('tools.fileShare.sessionTtlMinutes') }}</label>
                <input
                  v-model.number="draft.session_ttl_minutes"
                  type="number"
                  min="1"
                  :max="MAX_SESSION_TTL_MINUTES"
                  :disabled="formDisabled"
                  class="fs-input w-full"
                />
              </div>
              <div>
                <label class="fs-label">{{ t('tools.fileShare.deleteMode') }}</label>
                <select v-model="draft.delete_mode" :disabled="formDisabled" class="fs-select w-full">
                  <option v-for="opt in delOpts" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
                </select>
              </div>
              <div>
                <label class="fs-label">{{ t('tools.fileShare.ipFilter') }}</label>
                <select v-model="draft.ip_filter_mode" :disabled="formDisabled" class="fs-select w-full">
                  <option v-for="opt in ipOpts" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
                </select>
              </div>
            </div>
            <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
              <label class="fs-toggle-line"><span class="fs-toggle"><input v-model="draft.guest_access_enabled" type="checkbox" :disabled="formDisabled" class="sr-only"><span class="fs-toggle-track" :class="draft.guest_access_enabled ? 'bg-teal-600' : 'bg-slate-300'"><span class="fs-toggle-thumb" :class="draft.guest_access_enabled ? 'translate-x-4' : 'translate-x-0'"></span></span></span><span>{{ t('tools.fileShare.guestAccess') }}</span></label>
              <label class="fs-toggle-line"><span class="fs-toggle"><input v-model="draft.image_preview_enabled" type="checkbox" :disabled="formDisabled" class="sr-only"><span class="fs-toggle-track" :class="draft.image_preview_enabled ? 'bg-teal-600' : 'bg-slate-300'"><span class="fs-toggle-thumb" :class="draft.image_preview_enabled ? 'translate-x-4' : 'translate-x-0'"></span></span></span><span>{{ t('tools.fileShare.imagePreview') }}</span></label>
              <label class="fs-toggle-line"><span class="fs-toggle"><input v-model="draft.thumbnail_enabled" type="checkbox" :disabled="formDisabled" class="sr-only"><span class="fs-toggle-track" :class="draft.thumbnail_enabled ? 'bg-teal-600' : 'bg-slate-300'"><span class="fs-toggle-thumb" :class="draft.thumbnail_enabled ? 'translate-x-4' : 'translate-x-0'"></span></span></span><span>{{ t('tools.fileShare.thumbnails') }}</span></label>
              <label class="fs-toggle-line"><span class="fs-toggle"><input v-model="draft.remember_settings" type="checkbox" :disabled="formDisabled" class="sr-only"><span class="fs-toggle-track" :class="draft.remember_settings ? 'bg-teal-600' : 'bg-slate-300'"><span class="fs-toggle-thumb" :class="draft.remember_settings ? 'translate-x-4' : 'translate-x-0'"></span></span></span><span>{{ t('tools.fileShare.rememberSettings') }}</span></label>
              <label class="fs-toggle-line"><span class="fs-toggle"><input v-model="draft.auto_start_on_page_open" type="checkbox" :disabled="formDisabled" class="sr-only"><span class="fs-toggle-track" :class="draft.auto_start_on_page_open ? 'bg-teal-600' : 'bg-slate-300'"><span class="fs-toggle-thumb" :class="draft.auto_start_on_page_open ? 'translate-x-4' : 'translate-x-0'"></span></span></span><span>{{ t('tools.fileShare.autoStartOnPageOpen') }}</span></label>
              <label class="fs-toggle-line"><span class="fs-toggle"><input v-model="draft.auto_start_with_windows" type="checkbox" :disabled="formDisabled" class="sr-only"><span class="fs-toggle-track" :class="draft.auto_start_with_windows ? 'bg-teal-600' : 'bg-slate-300'"><span class="fs-toggle-thumb" :class="draft.auto_start_with_windows ? 'translate-x-4' : 'translate-x-0'"></span></span></span><span>{{ t('tools.fileShare.restoreOnStartup') }}</span></label>
            </div>
            <div v-if="draft.ip_filter_mode !== 'off'" class="mt-4 rounded-xl border border-slate-200 bg-slate-50 p-4">
              <div class="flex flex-col gap-2 sm:flex-row">
                <input v-model="newIpRule" :disabled="formDisabled" class="fs-input w-full" :placeholder="t('tools.fileShare.ipRulePlaceholder')" @keyup.enter="draft.ip_rules.includes(newIpRule.trim()) || !newIpRule.trim() ? null : (draft.ip_rules.push(newIpRule.trim()), newIpRule = '')" />
                <button type="button" :disabled="formDisabled" class="fs-btn fs-btn-plain" @click="draft.ip_rules.includes(newIpRule.trim()) || !newIpRule.trim() ? null : (draft.ip_rules.push(newIpRule.trim()), newIpRule = '')">{{ t('tools.fileShare.addRule') }}</button>
              </div>
              <div v-if="draft.ip_rules.length > 0" class="mt-3 flex flex-wrap gap-2">
                <span v-for="(rule, index) in draft.ip_rules" :key="`${rule}-${index}`" class="rounded-full border border-slate-200 bg-white px-3 py-1 text-xs font-mono text-slate-700">
                  {{ rule }}
                  <button type="button" :disabled="formDisabled" class="ml-2 text-slate-400 hover:text-red-500" @click="draft.ip_rules.splice(index, 1)">×</button>
                </span>
              </div>
            </div>
          </div>

          <div class="fs-card">
            <div class="mb-4 flex items-center justify-between gap-3">
              <div>
                <p class="fs-label-sm">{{ t('tools.fileShare.guestAndAccountsTitle') }}</p>
                <p class="text-sm text-slate-500">{{ t('tools.fileShare.guestAndAccountsDescription') }}</p>
              </div>
              <button type="button" :disabled="formDisabled" @click="addAccount" class="fs-btn fs-btn-soft"><Plus class="h-4 w-4" />{{ t('tools.fileShare.addAccount') }}</button>
            </div>

            <div class="fs-account">
              <div class="mb-3 flex items-center justify-between gap-3">
                <div class="font-semibold text-slate-900">{{ t('tools.fileShare.guestAccount') }}</div>
                <span class="rounded-full border px-2 py-1 text-xs" :class="guest.password_set || guest.new_password ? 'border-emerald-200 bg-emerald-50 text-emerald-700' : 'border-slate-200 bg-white text-slate-500'">
                  {{ guest.password_set || guest.new_password ? t('tools.fileShare.passwordSetState') : t('tools.fileShare.noPasswordState') }}
                </span>
              </div>
              <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                <div><label class="fs-label">{{ t('tools.fileShare.username') }}</label><input v-model="guest.username" :disabled="formDisabled" class="fs-input w-full" /></div>
                <div class="md:col-span-2">
                  <label class="fs-label">{{ t('tools.fileShare.guestPassword') }}</label>
                  <div class="relative"><KeyRound class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" /><input v-model="guest.new_password" type="password" :disabled="formDisabled" class="fs-input fs-input-with-icon w-full" :placeholder="t('tools.fileShare.keepPasswordPlaceholder')" @input="onPassword(guest)" /></div>
                  <label class="mt-2 inline-flex items-center gap-2 text-xs text-slate-500"><input v-model="guest.clear_password" type="checkbox" :disabled="formDisabled" class="rounded border-slate-300" @change="onClear(guest)" />{{ t('tools.fileShare.clearGuestPasswordOnSave') }}</label>
                </div>
              </div>
              <div class="mt-4">
                <label class="fs-label">{{ t('tools.fileShare.rootAccess') }}</label>
                <div v-if="draft.roots.length === 0" class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-4 py-4 text-center text-sm text-slate-500">
                  {{ t('tools.fileShare.noRootsForPermissions') }}
                </div>
                <div v-else class="space-y-3">
                  <div v-for="row in rootAccessRows(guest)" :key="`guest-${row.root.id}`" class="rounded-xl border border-slate-200 bg-white p-3">
                    <label class="flex items-center justify-between gap-3">
                      <span class="flex items-center gap-2 text-sm font-semibold text-slate-900">
                        <input
                          type="checkbox"
                          :checked="!!row.entry"
                          :disabled="formDisabled"
                          class="rounded border-slate-300"
                          @change="(e) => onRootAccessChange(guest, row.root.id, e)"
                        />
                        <span>{{ row.root.alias || row.root.id }}</span>
                      </span>
                    </label>
                    <template v-if="row.entry">
                      <div class="mt-3">
                        <label class="fs-label">{{ t('tools.fileShare.permissionPreset') }}</label>
                        <select
                          :value="row.entry.preset"
                          :disabled="formDisabled"
                          class="fs-select w-full"
                          @change="(e) => onRootPresetChange(guest, row.root.id, e)"
                        >
                          <option v-for="opt in presetOpts" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
                        </select>
                      </div>
                      <div v-if="row.entry.preset === 'custom'" class="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-3">
                        <label v-for="p in permDefs" :key="`guest-${row.root.id}-${p}`" class="fs-perm">
                          <input
                            v-model="row.entry.permissions[p]"
                            type="checkbox"
                            :disabled="formDisabled"
                            class="rounded border-slate-300"
                          />
                          <span>{{ permissionLabel(p) }}</span>
                        </label>
                      </div>
                    </template>
                  </div>
                </div>
              </div>
            </div>

            <div v-if="customAccounts.length === 0" class="mt-4 rounded-xl border border-dashed border-slate-200 bg-slate-50 px-4 py-6 text-center text-sm text-slate-500">
              {{ t('tools.fileShare.noCustomAccounts') }}
            </div>
            <div v-else class="mt-4 space-y-4">
              <div v-for="account in customAccounts" :key="account.draft_key" class="fs-account">
                <div class="mb-3 flex items-center justify-between gap-3">
                  <div class="font-semibold text-slate-900">{{ account.username || t('tools.fileShare.newAccountDefaultUsername') }}</div>
                  <button type="button" :disabled="formDisabled" class="fs-btn fs-btn-danger" @click="draft.accounts = draft.accounts.filter((a) => a.draft_key !== account.draft_key)"><Trash2 class="h-4 w-4" /></button>
                </div>
                <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
                  <div><label class="fs-label">{{ t('tools.fileShare.username') }}</label><input v-model="account.username" :disabled="formDisabled" class="fs-input w-full" /></div>
                  <label class="fs-toggle-line"><span class="fs-toggle"><input v-model="account.enabled" type="checkbox" :disabled="formDisabled" class="sr-only"><span class="fs-toggle-track" :class="account.enabled ? 'bg-teal-600' : 'bg-slate-300'"><span class="fs-toggle-thumb" :class="account.enabled ? 'translate-x-4' : 'translate-x-0'"></span></span></span><span>{{ t('tools.fileShare.accountEnabled') }}</span></label>
                  <div class="md:col-span-2">
                    <label class="fs-label">{{ t('tools.fileShare.accountPassword') }}</label>
                    <div class="relative"><KeyRound class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" /><input v-model="account.new_password" type="password" :disabled="formDisabled" class="fs-input fs-input-with-icon w-full" :placeholder="t('tools.fileShare.keepPasswordPlaceholder')" @input="onPassword(account)" /></div>
                    <label class="mt-2 inline-flex items-center gap-2 text-xs text-slate-500"><input v-model="account.clear_password" type="checkbox" :disabled="formDisabled" class="rounded border-slate-300" @change="onClear(account)" />{{ t('tools.fileShare.clearAccountPasswordOnSave') }}</label>
                  </div>
                </div>
                <div class="mt-4">
                  <label class="fs-label">{{ t('tools.fileShare.rootAccess') }}</label>
                  <div v-if="draft.roots.length === 0" class="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-4 py-4 text-center text-sm text-slate-500">
                    {{ t('tools.fileShare.noRootsForPermissions') }}
                  </div>
                  <div v-else class="space-y-3">
                    <div v-for="row in rootAccessRows(account)" :key="`${account.draft_key}-${row.root.id}`" class="rounded-xl border border-slate-200 bg-white p-3">
                      <label class="flex items-center justify-between gap-3">
                        <span class="flex items-center gap-2 text-sm font-semibold text-slate-900">
                          <input
                            type="checkbox"
                            :checked="!!row.entry"
                            :disabled="formDisabled"
                            class="rounded border-slate-300"
                            @change="(e) => onRootAccessChange(account, row.root.id, e)"
                          />
                          <span>{{ row.root.alias || row.root.id }}</span>
                        </span>
                      </label>
                      <template v-if="row.entry">
                        <div class="mt-3">
                          <label class="fs-label">{{ t('tools.fileShare.permissionPreset') }}</label>
                          <select
                            :value="row.entry.preset"
                            :disabled="formDisabled"
                            class="fs-select w-full"
                            @change="(e) => onRootPresetChange(account, row.root.id, e)"
                          >
                            <option v-for="opt in presetOpts" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
                          </select>
                        </div>
                        <div v-if="row.entry.preset === 'custom'" class="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-3">
                          <label v-for="p in permDefs" :key="`${account.draft_key}-${row.root.id}-${p}`" class="fs-perm">
                            <input
                              v-model="row.entry.permissions[p]"
                              type="checkbox"
                              :disabled="formDisabled"
                              class="rounded border-slate-300"
                            />
                            <span>{{ permissionLabel(p) }}</span>
                          </label>
                        </div>
                      </template>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <div class="space-y-4 lg:sticky lg:top-6">
          <div class="fs-card">
            <p class="fs-label-sm">{{ t('tools.fileShare.applyAndRuntimeTitle') }}</p>
            <div v-if="errorMsg" class="mb-3 rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-600">{{ errorMsg }}</div>
            <div v-if="notice" class="mb-3 rounded-xl border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm text-emerald-700">{{ notice }}</div>
            <div v-if="errors.length" class="mb-3 rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-700">
              <div class="font-semibold">{{ t('tools.fileShare.fixBeforeSaving') }}</div>
              <ul class="mt-2 list-disc pl-5"><li v-for="item in errors" :key="item">{{ item }}</li></ul>
            </div>
            <div v-if="lockoutWarn" class="mb-3 rounded-xl border border-orange-200 bg-orange-50 px-4 py-3 text-sm text-orange-700">
              {{ t('tools.fileShare.lockoutWarning') }}
            </div>
            <div class="grid grid-cols-1 gap-3">
              <button type="button" :disabled="isActive ? !canStart : !canSave" @click="isActive ? startShare(true, true) : saveSettings()" class="fs-btn fs-btn-main w-full"><component :is="isActive ? RefreshCw : Save" class="h-4 w-4" />{{ isActive && isApplying ? t('tools.fileShare.restarting') : !isActive && isSaving ? t('tools.fileShare.saving') : t('tools.fileShare.saveSettings') }}</button>
              <button v-if="!isActive" type="button" :disabled="!canStart" @click="startShare(false, true)" class="fs-btn fs-btn-start w-full"><Play class="h-4 w-4" />{{ isApplying ? t('tools.fileShare.starting') : t('tools.fileShare.startShare') }}</button>
              <button v-if="isActive" type="button" :disabled="isApplying" @click="stopShare" class="fs-btn fs-btn-danger w-full"><Power class="h-4 w-4" />{{ t('tools.fileShare.stopShare') }}</button>
            </div>
            <div class="mt-4 grid grid-cols-2 gap-3">
              <div class="fs-stat"><div class="fs-stat-label">{{ t('tools.fileShare.enabledRoots') }}</div><div class="fs-stat-value">{{ enabledRoots.length }}</div></div>
              <div class="fs-stat"><div class="fs-stat-label">{{ t('tools.fileShare.enabledAccounts') }}</div><div class="fs-stat-value">{{ enabledCustomAccounts.length + (draft.guest_access_enabled ? 1 : 0) }}</div></div>
            </div>
          </div>

          <template v-if="isActive && serverUrl">
            <div class="fs-card">
              <p class="fs-label-sm">{{ t('tools.fileShare.accessUrl') }}</p>
              <div class="flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2.5">
                <code class="flex-1 truncate font-mono text-sm font-semibold text-teal-700">{{ serverUrl }}</code>
                <button type="button" @click="copy(serverUrl)" class="fs-icon" :title="t('tools.fileShare.copyUrl')"><Copy class="h-4 w-4" :class="copied ? 'text-teal-600' : ''" /></button>
                <button type="button" @click="showQr = !showQr" class="fs-icon" :title="showQr ? t('tools.fileShare.hideQrCode') : t('tools.fileShare.showQrCode')"><QrCode class="h-4 w-4" :class="showQr ? 'text-teal-600' : ''" /></button>
                <button type="button" @click="openBrowser" class="fs-icon" :title="t('tools.fileShare.openInBrowser')"><ExternalLink class="h-4 w-4" /></button>
              </div>
              <div v-if="showQr" class="mt-4 flex justify-center"><div class="rounded-xl border border-slate-200 bg-white p-3 shadow-sm"><canvas ref="qrCanvas" width="128" height="128" /></div></div>
              <div v-if="altUrls.length" class="mt-4">
                <button type="button" @click="showAltUrls = !showAltUrls" class="fs-link"><component :is="showAltUrls ? ChevronUp : ChevronDown" class="h-3.5 w-3.5" />{{ t('tools.fileShare.altUrls', { n: altUrls.length }) }}</button>
                <div v-if="showAltUrls" class="mt-2 space-y-2">
                  <div v-for="url in altUrls" :key="url" class="flex items-center gap-2 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2">
                    <code class="flex-1 truncate font-mono text-xs text-slate-600">{{ url }}</code>
                    <button type="button" @click="copy(url)" class="text-slate-400 hover:text-teal-600"><Copy class="h-3.5 w-3.5" /></button>
                  </div>
                </div>
              </div>
            </div>

            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div class="fs-stat"><div class="fs-stat-label">{{ t('tools.fileShare.connectionCount') }}</div><div class="fs-stat-value">{{ connCount }}</div></div>
              <div class="fs-stat"><div class="fs-stat-label">{{ t('tools.fileShare.uptime') }}</div><div class="fs-stat-value">{{ uptime }}</div></div>
            </div>

            <div class="fs-card">
              <div class="mb-3 text-sm font-semibold text-slate-900">{{ t('tools.fileShare.connectedIpList') }}</div>
              <div v-if="connectedIps.length" class="space-y-2">
                <div v-for="ip in visibleIps" :key="ip" class="rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 font-mono text-sm text-slate-700">{{ ip }}</div>
                <button v-if="hiddenIpCount > 0" type="button" class="fs-link" @click="showAllIps = true"><ChevronDown class="h-3.5 w-3.5" />{{ t('tools.fileShare.showMoreIps', { n: hiddenIpCount }) }}</button>
                <button v-else-if="connectedIps.length > 10" type="button" class="fs-link" @click="showAllIps = false"><ChevronUp class="h-3.5 w-3.5" />{{ t('tools.fileShare.collapseIps') }}</button>
              </div>
              <div v-else class="text-sm text-slate-500">{{ t('tools.fileShare.noConnections') }}</div>
            </div>
          </template>

          <div v-if="logs.length" class="fs-card">
            <p class="fs-label-sm">{{ t('tools.fileShare.logTitle') }}</p>
            <div class="max-h-56 space-y-2 overflow-y-auto">
              <div v-for="(log, index) in logs" :key="index" class="flex gap-3 text-xs">
                <span class="shrink-0 font-mono text-slate-400">{{ log.time }}</span>
                <span :class="log.level === 'error' ? 'text-red-600' : 'text-slate-600'">{{ log.message }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.fs-card,.fs-stat{border:1px solid rgb(226 232 240 / .9);border-radius:.875rem;background:#fff;box-shadow:0 8px 24px rgb(15 23 42 / .05)}
.fs-card{padding:1rem}.fs-stat{padding:.9rem}
.fs-root-list{display:flex;flex-direction:column;gap:.75rem}
.fs-root-row{border:1px solid rgb(226 232 240 / .9);border-radius:.875rem;background:linear-gradient(180deg,#fff 0%,rgb(248 250 252) 100%);padding:.9rem}
.fs-root-row-top{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:.75rem;align-items:end}
.fs-root-actions{display:flex;flex-wrap:wrap;justify-content:flex-end;align-items:center;gap:.5rem}
.fs-inline-toggle{display:inline-flex;align-items:center;gap:.6rem;min-height:2.5rem;padding:0 .75rem;border:1px solid rgb(226 232 240 / .85);border-radius:.75rem;background:#fff;font-size:.8125rem;font-weight:600;color:rgb(51 65 85);white-space:nowrap}
.fs-root-path{margin-top:.75rem;min-height:2.5rem;display:flex;align-items:center;padding:.7rem .85rem;border:1px solid rgb(226 232 240 / .8);border-radius:.75rem;background:rgb(248 250 252);font-size:.8125rem;color:rgb(71 85 105);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.fs-label-sm{margin-bottom:.75rem;font-size:.7rem;font-weight:700;letter-spacing:.14em;text-transform:uppercase;color:rgb(100 116 139)}
.fs-label{display:block;margin-bottom:.4rem;font-size:.75rem;font-weight:600;color:rgb(71 85 105)}
.fs-input,.fs-select{min-height:2.75rem;border:1px solid rgb(203 213 225);border-radius:.75rem;background:#fff;padding:.65rem .9rem;font-size:.875rem;line-height:1.5;color:rgb(15 23 42);outline:none;transition:border-color .15s ease,box-shadow .15s ease}
.fs-input-with-icon{padding-left:3rem}
.fs-select{appearance:none;background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%2364758b' stroke-width='2'%3E%3Cpolyline points='6 9 12 15 18 9'%3E%3C/polyline%3E%3C/svg%3E");background-repeat:no-repeat;background-position:right 12px center;padding-right:2.25rem}
.fs-input:focus,.fs-select:focus{border-color:rgb(13 148 136);box-shadow:0 0 0 3px rgb(13 148 136 / .12)}
.fs-input:disabled,.fs-select:disabled{cursor:not-allowed;background:rgb(248 250 252);color:rgb(148 163 184)}
.fs-toggle{position:relative;display:inline-flex}.fs-toggle-track{display:block;height:20px;width:36px;flex-shrink:0;border-radius:9999px;transition:background-color .2s ease}.fs-toggle-thumb{position:absolute;top:2px;left:2px;height:16px;width:16px;border-radius:9999px;background:#fff;box-shadow:0 1px 3px rgb(15 23 42 /.2);transition:transform .2s ease}
.fs-toggle-line{display:flex;align-items:center;gap:.75rem;border:1px solid rgb(226 232 240 / .8);border-radius:.75rem;background:#fff;padding:.9rem 1rem;font-size:.875rem;font-weight:500;color:rgb(51 65 85)}
.fs-btn{display:inline-flex;align-items:center;justify-content:center;gap:.45rem;min-height:2.75rem;flex-shrink:0;border-radius:.75rem;padding:.8rem 1rem;font-size:.875rem;font-weight:600;white-space:nowrap;transition:all .15s ease}
.fs-btn svg{flex-shrink:0}
.fs-btn-main{border:1px solid rgb(186 230 253);background:rgb(239 246 255);color:rgb(3 105 161)}.fs-btn-soft{border:1px solid rgb(153 246 228);background:rgb(240 253 250);color:rgb(15 118 110)}.fs-btn-start{border:none;background:linear-gradient(135deg,rgb(13 148 136),rgb(8 145 178));color:#fff;box-shadow:0 8px 20px rgb(13 148 136 /.18)}.fs-btn-plain{border:1px solid rgb(226 232 240);background:#fff;color:rgb(51 65 85)}.fs-btn-danger{border:1px solid rgb(254 202 202);background:rgb(254 242 242);color:rgb(220 38 38)}
.fs-btn-compact{min-height:2.5rem;padding:.65rem .9rem}
.fs-btn-icon{min-width:2.5rem;min-height:2.5rem;padding:0}
.fs-btn:disabled{opacity:.4;cursor:not-allowed}.fs-account{border:1px solid rgb(226 232 240 / .9);border-radius:1rem;background:linear-gradient(180deg,#fff 0%,rgb(248 250 252) 100%);padding:1rem}
.fs-perm{display:flex;align-items:center;gap:.6rem;border:1px solid rgb(226 232 240 / .8);border-radius:.85rem;background:#fff;padding:.75rem .85rem;font-size:.875rem;color:rgb(51 65 85)}
.fs-icon{border:1px solid rgb(226 232 240);border-radius:.65rem;background:#fff;padding:.45rem;color:rgb(100 116 139);transition:all .15s ease}.fs-icon:hover{border-color:rgb(153 246 228);background:rgb(240 253 250);color:rgb(13 148 136)}
.fs-link{display:inline-flex;align-items:center;gap:.35rem;font-size:.75rem;font-weight:600;color:rgb(100 116 139)}.fs-link:hover{color:rgb(13 148 136)}
.fs-stat-label{margin-bottom:.35rem;font-size:.7rem;font-weight:700;letter-spacing:.14em;text-transform:uppercase;color:rgb(100 116 139)}.fs-stat-value{font-family:ui-monospace,SFMono-Regular,monospace;font-size:1.5rem;font-weight:700;color:rgb(15 23 42)}
@media (max-width: 900px){.fs-root-row-top{grid-template-columns:1fr}.fs-root-actions{justify-content:flex-start}}
</style>
