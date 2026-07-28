<script setup lang="ts">
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  ArrowLeft,
  ArrowRight,
  CheckCircle2,
  ChevronRight,
  Eye,
  EyeOff,
  Folder,
  FileUp,
  FolderOpen,
  Loader2,
  PackageSearch,
  Play,
  Server,
  ShieldAlert,
  Sparkles,
  Terminal,
  ClipboardCopy,
  Trash2,
} from 'lucide-vue-next';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import BrowserDialog from '@/components/remote-package-patch/BrowserDialog.vue';
import RemoteDirBrowser from '@/components/remote-package-patch/RemoteDirBrowser.vue';
import {
  enableApplianceSsh,
  getConfig,
  remotePackagePatchApi,
  type DeployServer,
  type InternalLayer,
  type PackageEntry,
  type PackageInventory,
  type PackagePatchRequest,
  type PackagePatchResult,
  type PickedLocalFile,
  type RemotePackagePatchEvent,
  type RemoteSshConfig,
} from '@/lib/tauri';
import {
  REMOTE_PACKAGE_PATCH_DEFAULT_SSH_PORT,
  REMOTE_PACKAGE_PATCH_DEFAULT_PASSWORD,
  buildRemotePackagePatchEnableSshRequest,
  composeInternalTargetPath,
  defaultPatchedPath,
  formatBytes,
  layerKey,
  replacementName,
  resolveRemotePackagePatchSshPort,
  shouldAttemptRemotePackagePatchAutoEnable,
  targetCandidates,
  updateRemotePackagePatchHostHistory,
  validateInternalTargetPath,
  visibleStages,
} from '@/lib/remotePackagePatch';

type TargetMode = 'candidate' | 'directory' | 'manual';

const HOST_HISTORY_STORAGE_KEY = 'remotePackagePatch.hostHistory';

interface DirectoryOption {
  key: string;
  path: string;
  layer: InternalLayer;
  label: string;
}

defineOptions({ name: 'RemotePackagePatchPage' });

const { t, te } = useI18n();

const savedServers = ref<DeployServer[]>([]);
const recentHosts = ref<string[]>([]);
const selectedServerId = ref('');
const host = ref('');
const port = ref(REMOTE_PACKAGE_PATCH_DEFAULT_SSH_PORT);
const username = ref('root');
const password = ref(REMOTE_PACKAGE_PATCH_DEFAULT_PASSWORD);
const showPassword = ref(false);

const connectionBusy = ref(false);
const connected = ref(false);
const connectionMessage = ref('');
const selectedPackage = ref('');
const replacement = ref<PickedLocalFile | null>(null);
const replacementBusy = ref(false);

const inventory = ref<PackageInventory | null>(null);
const scanBusy = ref(false);
const scanStage = ref('');
const scanError = ref('');
const selectedCandidate = ref<PackageEntry | null>(null);
const targetMode = ref<TargetMode>('candidate');
const selectedDirectoryKey = ref('');
const directoryPickerOpen = ref(false);
const browsingLayerKey = ref('');
const browsingDirectoryPath = ref('');
const internalFileName = ref('');
const manualInternalPath = ref('');
const outputPath = ref('');
const overwrite = ref(false);
const overwriteConfirmed = ref(false);

const running = ref(false);
const activeStage = ref('');
const completedStages = ref<string[]>([]);
const failedStage = ref('');
const errorMessage = ref('');
const logs = ref<Array<{ level: string; message: string }>>([]);
const uploadProgress = ref<{ sent: number; total: number } | null>(null);
const result = ref<PackagePatchResult | null>(null);
const logContainer = ref<HTMLElement | null>(null);
const autoScroll = ref(true);
const summaryToast = ref('');

const AUTO_SCROLL_THRESHOLD_PX = 48;

let unlisten: UnlistenFn | null = null;

const sshConfig = computed<RemoteSshConfig>(() => ({
  host: host.value.trim(),
  port: resolveRemotePackagePatchSshPort(port.value),
  username: username.value.trim(),
  auth: { kind: 'password', password: password.value },
}));

const replacementFileName = computed(() =>
  replacement.value ? replacementName(replacement.value.name || replacement.value.path) : '',
);

const candidates = computed(() =>
  inventory.value && replacementFileName.value
    ? targetCandidates(inventory.value, replacementFileName.value)
    : [],
);

const directoryOptions = computed<DirectoryOption[]>(() => {
  if (!inventory.value) return [];
  const map = new Map<string, DirectoryOption>();
  for (const entry of inventory.value.entries) {
    const entryLayerKey = layerKey(entry.layer);
    const rootKey = `${entryLayerKey}::`;
    if (!map.has(rootKey)) {
      map.set(rootKey, {
        key: rootKey,
        path: '',
        layer: entry.layer,
        label: `${formatLayer(entry.layer)} / ${t('remotePackagePatch.target.directoryRoot')}`,
      });
    }
    const normalized = entry.path.replace(/\\/g, '/').replace(/^\.\/+/, '').replace(/\/+$/g, '');
    const directory =
      entry.kind === 'dir'
        ? normalized
        : normalized.includes('/')
          ? normalized.slice(0, normalized.lastIndexOf('/'))
          : '';
    if (!directory) continue;
    const segments = directory.split('/').filter(Boolean);
    for (let index = 0; index < segments.length; index += 1) {
      const ancestorPath = segments.slice(0, index + 1).join('/');
      const key = `${entryLayerKey}::${ancestorPath}`;
      if (!map.has(key)) {
        map.set(key, {
          key,
          path: ancestorPath,
          layer: entry.layer,
          label: `${formatLayer(entry.layer)} / ${ancestorPath}`,
        });
      }
    }
  }
  return Array.from(map.values()).sort((left, right) => left.label.localeCompare(right.label));
});

const selectedDirectory = computed(() =>
  directoryOptions.value.find((option) => option.key === selectedDirectoryKey.value) ?? null,
);

const directoryLayers = computed(() => {
  const map = new Map<string, InternalLayer>();
  for (const option of directoryOptions.value) {
    map.set(layerKey(option.layer), option.layer);
  }
  return Array.from(map, ([key, layer]) => ({ key, layer }));
});

const browsingDirectory = computed(() =>
  directoryOptions.value.find(
    (option) => layerKey(option.layer) === browsingLayerKey.value && option.path === browsingDirectoryPath.value,
  ) ?? null,
);

const browsingChildren = computed(() =>
  directoryOptions.value.filter((option) => {
    if (layerKey(option.layer) !== browsingLayerKey.value || !option.path) return false;
    const separator = option.path.lastIndexOf('/');
    const parent = separator >= 0 ? option.path.slice(0, separator) : '';
    return parent === browsingDirectoryPath.value;
  }),
);

const browsingBreadcrumbs = computed(() => {
  const segments = browsingDirectoryPath.value.split('/').filter(Boolean);
  return [
    { label: t('remotePackagePatch.target.directoryRoot'), path: '' },
    ...segments.map((segment, index) => ({
      label: segment,
      path: segments.slice(0, index + 1).join('/'),
    })),
  ];
});

const targetInternalPath = computed(() => {
  if (targetMode.value === 'candidate') return selectedCandidate.value?.path ?? '';
  if (targetMode.value === 'directory') {
    return composeInternalTargetPath(selectedDirectory.value?.path ?? '', internalFileName.value);
  }
  return manualInternalPath.value;
});

const targetLayer = computed<InternalLayer | null>(() => {
  if (targetMode.value === 'candidate') return selectedCandidate.value?.layer ?? null;
  if (targetMode.value === 'directory') return selectedDirectory.value?.layer ?? null;
  return null;
});

const targetError = computed(() => validateInternalTargetPath(targetInternalPath.value));
const targetErrorText = computed(() =>
  targetError.value ? t(`remotePackagePatch.target.errors.${targetError.value}`) : '',
);

const canConnect = computed(() => {
  if (!host.value.trim() || !username.value.trim() || !port.value) return false;
  return password.value.length > 0;
});

const canScan = computed(() => connected.value && selectedPackage.value && !scanBusy.value);

const canStartPatch = computed(
  () =>
    connected.value &&
    Boolean(selectedPackage.value) &&
    Boolean(replacement.value) &&
    !targetError.value &&
    (overwrite.value ? overwriteConfirmed.value : Boolean(outputPath.value.trim())) &&
    !running.value,
);

const stageList = computed(() =>
  visibleStages({ overwrite: overwrite.value, layer: targetLayer.value }),
);

const uploadPercent = computed(() => {
  if (!uploadProgress.value) return 0;
  return Math.min(100, (uploadProgress.value.sent / Math.max(1, uploadProgress.value.total)) * 100);
});

interface StepMeta {
  num: number;
  key: 'connect' | 'package' | 'target' | 'execute';
  state: 'idle' | 'active' | 'done' | 'failed';
}

const stepList = computed<StepMeta[]>(() => {
  const failed = Boolean(failedStage.value);
  const finished = Boolean(result.value);
  if (failed) {
    return [
      { num: 1, key: 'connect', state: 'done' },
      { num: 2, key: 'package', state: 'done' },
      { num: 3, key: 'target', state: 'done' },
      { num: 4, key: 'execute', state: 'failed' },
    ];
  }
  if (finished) {
    return [
      { num: 1, key: 'connect', state: 'done' },
      { num: 2, key: 'package', state: 'done' },
      { num: 3, key: 'target', state: 'done' },
      { num: 4, key: 'execute', state: 'done' },
    ];
  }
  if (running.value) {
    return [
      { num: 1, key: 'connect', state: 'done' },
      { num: 2, key: 'package', state: 'done' },
      { num: 3, key: 'target', state: 'done' },
      { num: 4, key: 'execute', state: 'active' },
    ];
  }
  return [
    { num: 1, key: 'connect', state: connected.value ? 'done' : 'active' },
    { num: 2, key: 'package', state: connected.value ? (selectedPackage.value ? 'done' : 'active') : 'idle' },
    {
      num: 3,
      key: 'target',
      state: !connected.value
        ? 'idle'
        : selectedPackage.value
          ? 'active'
          : 'idle',
    },
    { num: 4, key: 'execute', state: 'idle' },
  ];
});

const summaryStatus = computed<{ key: string; tone: 'ok' | 'warn' | 'err' }>(() => {
  if (failedStage.value) return { key: 'failed', tone: 'err' };
  if (result.value) return { key: 'done', tone: 'ok' };
  if (running.value) return { key: 'running', tone: 'warn' };
  if (connected.value && selectedPackage.value && replacement.value && !targetError.value) {
    return { key: 'ready', tone: 'ok' };
  }
  if (connected.value) return { key: 'configuring', tone: 'warn' };
  return { key: 'notReady', tone: 'err' };
});

const summaryStatusText = computed(() => {
  const map: Record<string, string> = {
    failed: t('remotePackagePatch.execution.failed'),
    done: t('remotePackagePatch.summary.done'),
    running: `${t('remotePackagePatch.summary.running')} ${uploadPercent.value.toFixed(0)}%`,
    ready: t('remotePackagePatch.execution.start'),
    configuring: t('remotePackagePatch.summary.configuring'),
    notReady: t('remotePackagePatch.summary.notReady'),
  };
  return map[summaryStatus.value.key] ?? '';
});

const summaryHostText = computed(() => {
  if (!host.value.trim()) return '';
  return `${host.value.trim()}:${port.value}`;
});

const summaryPackageText = computed(() => {
  if (!selectedPackage.value) return '';
  const normalized = selectedPackage.value.replace(/\\/g, '/');
  return normalized.split('/').at(-1) ?? selectedPackage.value;
});

const summaryFileText = computed(() => {
  if (!replacement.value) return '';
  return replacementName(replacement.value.name || replacement.value.path);
});

const summaryTargetText = computed(() => {
  if (!targetInternalPath.value) return '';
  return targetInternalPath.value.split('/').at(-1) ?? targetInternalPath.value;
});

const summaryTargetLayer = computed(() => {
  const layer = targetLayer.value;
  if (!layer) return '';
  if (layer.kind === 'middle') return t('remotePackagePatch.target.layerMiddle');
  return t('remotePackagePatch.target.layerZst', { path: layer.zstPath });
});

function stepNumClass(state: StepMeta['state']) {
  if (state === 'done') return 'rpp-step-done';
  if (state === 'active') return 'rpp-step-active';
  if (state === 'failed') return 'rpp-step-failed';
  return 'rpp-step-idle';
}

function stepLabelClass(state: StepMeta['state']) {
  if (state === 'done') return 'text-emerald-700';
  if (state === 'active') return 'text-slate-900';
  if (state === 'failed') return 'text-red-700';
  return 'text-slate-400';
}

function stepDividerClass(after: StepMeta) {
  if (after.state === 'done') return 'rpp-div-done';
  if (after.state === 'active') return 'rpp-div-active';
  return '';
}

function stepNumContent(step: StepMeta) {
  if (step.state === 'done') return '\u2713';
  if (step.state === 'failed') return '!';
  return String(step.num);
}

const sortedDirectoryLayers = computed(() => directoryLayers.value);

function logLevelClass(level: string) {
  if (level === 'error') return 'rpp-lvl-error';
  if (level === 'warn') return 'rpp-lvl-warn';
  if (level === 'success' || level === 'ok') return 'rpp-lvl-success';
  return 'rpp-lvl-info';
}

async function copyLogs() {
  const text = logs.value.map((entry) => `[${entry.level}] ${entry.message}`).join('\n');
  try {
    await navigator.clipboard.writeText(text);
    flashToast(t('remotePackagePatch.logs.copy'));
  } catch {
    flashToast(t('remotePackagePatch.execution.copiedFailed'));
  }
}

function clearLogs() {
  logs.value = [];
}

async function copyResultSummary() {
  if (!result.value) return;
  const lines = [
    `${t('remotePackagePatch.execution.outputLabel')}: ${result.value.outputPath}`,
  ];
  if (result.value.backupPath) {
    lines.push(`${t('remotePackagePatch.execution.backupLabel')}: ${result.value.backupPath}`);
  }
  lines.push(`${t('remotePackagePatch.execution.md5Label')}: ${result.value.targetMd5}`);
  lines.push(`${t('remotePackagePatch.execution.workdirLabel')}: ${result.value.workdir}`);
  if (result.value.updatedManifests.length > 0) {
    lines.push(
      `${t('remotePackagePatch.execution.manifestsLabel')}: ${result.value.updatedManifests.join(', ')}`,
    );
  } else {
    lines.push(
      `${t('remotePackagePatch.execution.manifestsLabel')}: ${t('remotePackagePatch.execution.manifestsNone')}`,
    );
  }
  try {
    await navigator.clipboard.writeText(lines.join('\n'));
    flashToast(t('remotePackagePatch.execution.summaryCopied'));
  } catch {
    flashToast(t('remotePackagePatch.execution.copiedFailed'));
  }
}

let toastTimer: ReturnType<typeof setTimeout> | null = null;
function flashToast(message: string) {
  summaryToast.value = message;
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    summaryToast.value = '';
  }, 1800);
}

function restoreDefaultOutputPath() {
  if (selectedPackage.value) {
    outputPath.value = defaultPatchedPath(selectedPackage.value);
  }
}

function formatLayer(layer: InternalLayer | null | undefined): string {
  if (!layer) return t('remotePackagePatch.target.layerAuto');
  if (layer.kind === 'middle') return t('remotePackagePatch.target.layerMiddle');
  return t('remotePackagePatch.target.layerZst', { path: layer.zstPath });
}

function stageText(stage: string): string {
  const key = `remotePackagePatch.stages.${stage}`;
  return te(key) ? t(key) : stage.replace(/_/g, ' ');
}

function log(level: string, message: string) {
  logs.value.push({ level, message });
}

function openDirectoryPicker() {
  const initial = selectedDirectory.value ?? directoryOptions.value[0];
  if (!initial) return;
  browsingLayerKey.value = layerKey(initial.layer);
  browsingDirectoryPath.value = initial.path;
  directoryPickerOpen.value = true;
}

function closeDirectoryPicker() {
  directoryPickerOpen.value = false;
}

function changeBrowsingLayer(nextLayerKey: string) {
  browsingLayerKey.value = nextLayerKey;
  browsingDirectoryPath.value = '';
}

function browseParentDirectory() {
  const separator = browsingDirectoryPath.value.lastIndexOf('/');
  browsingDirectoryPath.value = separator >= 0
    ? browsingDirectoryPath.value.slice(0, separator)
    : '';
}

function chooseBrowsingDirectory() {
  if (!browsingDirectory.value) return;
  selectedDirectoryKey.value = browsingDirectory.value.key;
  closeDirectoryPicker();
}

function loadHostHistory(): string[] {
  try {
    return updateRemotePackagePatchHostHistory(
      JSON.parse(localStorage.getItem(HOST_HISTORY_STORAGE_KEY) ?? '[]'),
      '',
    );
  } catch {
    return [];
  }
}

function rememberHost() {
  recentHosts.value = updateRemotePackagePatchHostHistory(recentHosts.value, host.value);
  try {
    localStorage.setItem(HOST_HISTORY_STORAGE_KEY, JSON.stringify(recentHosts.value));
  } catch (error) {
    console.warn('Failed to save remote package host history', error);
  }
}

function handleHostInput() {
  selectedServerId.value = '';
}

function applyServer(serverId: string) {
  const server = savedServers.value.find((item) => item.id === serverId);
  if (!server) return;
  host.value = server.host;
  port.value = resolveRemotePackagePatchSshPort(server.port);
  username.value = server.user || 'root';
  password.value = server.password || REMOTE_PACKAGE_PATCH_DEFAULT_PASSWORD;
}

async function runConnectionProbe(): Promise<string> {
  const message = await remotePackagePatchApi.testConnection(sshConfig.value);
  const displayMessage = message || 'OK';
  connectionMessage.value = displayMessage;
  connected.value = true;
  rememberHost();
  log('info', t('remotePackagePatch.logs.connected', { info: displayMessage }));
  return displayMessage;
}

async function enableApplianceSshAndRetry(firstError: string): Promise<boolean> {
  if (!shouldAttemptRemotePackagePatchAutoEnable(firstError)) {
    return false;
  }

  const request = buildRemotePackagePatchEnableSshRequest(sshConfig.value);
  if (!request) {
    log('warn', t('remotePackagePatch.connection.autoEnableSkipped'));
    return false;
  }

  log('warn', t('remotePackagePatch.connection.autoEnableStart', { error: firstError }));
  try {
    const results = await enableApplianceSsh(request);
    const result = results[0];
    if (!result?.success) {
      log('error', t('remotePackagePatch.connection.autoEnableFailed', { message: result?.message ?? 'No result' }));
      return false;
    }
    if (result.port) {
      port.value = resolveRemotePackagePatchSshPort(result.port);
    }
    log('info', t('remotePackagePatch.connection.autoEnableSuccess', { message: result.message }));
    return true;
  } catch (error) {
    log('error', t('remotePackagePatch.connection.autoEnableFailed', { message: String(error) }));
    return false;
  }
}

async function testConnection() {
  connectionBusy.value = true;
  connectionMessage.value = '';
  try {
    await runConnectionProbe();
  } catch (err) {
    const firstError = String(err);
    const enabled = await enableApplianceSshAndRetry(firstError);
    if (enabled) {
      try {
        await runConnectionProbe();
      } catch (retryError) {
        connected.value = false;
        connectionMessage.value = t('remotePackagePatch.connection.retryFailed', {
          first: firstError,
          retry: String(retryError),
        });
        log('error', connectionMessage.value);
      }
    } else {
      connected.value = false;
      connectionMessage.value = firstError;
      log('error', connectionMessage.value);
    }
  } finally {
    connectionBusy.value = false;
  }
}

async function pickReplacement() {
  replacementBusy.value = true;
  try {
    const picked = await remotePackagePatchApi.pickLocalFile();
    if (picked) {
      replacement.value = picked;
      internalFileName.value = replacementName(picked.name || picked.path);
      log('info', t('remotePackagePatch.logs.replacementSelected', { path: picked.path }));
    }
  } finally {
    replacementBusy.value = false;
  }
}

async function scanPackage() {
  if (!selectedPackage.value) return;
  scanBusy.value = true;
  scanStage.value = '';
  scanError.value = '';
  inventory.value = null;
  selectedCandidate.value = null;
  try {
    inventory.value = await remotePackagePatchApi.scanPackage(sshConfig.value, selectedPackage.value);
    outputPath.value = defaultPatchedPath(selectedPackage.value);
    selectedCandidate.value = candidates.value[0] ?? null;
    if (selectedCandidate.value) {
      targetMode.value = 'candidate';
    } else if (directoryOptions.value[0]) {
      targetMode.value = 'directory';
      selectedDirectoryKey.value = directoryOptions.value[0].key;
    } else {
      targetMode.value = 'manual';
    }
    log('info', t('remotePackagePatch.logs.scanned', { count: inventory.value.entries.length }));
  } catch (err) {
    scanError.value = String(err);
    log('error', scanError.value);
  } finally {
    scanBusy.value = false;
    scanStage.value = '';
  }
}

function markStage(stage: string) {
  if (activeStage.value && activeStage.value !== stage && !completedStages.value.includes(activeStage.value)) {
    completedStages.value.push(activeStage.value);
  }
  activeStage.value = stage;
}

async function startPatch() {
  if (!replacement.value || !canStartPatch.value) return;
  running.value = true;
  result.value = null;
  errorMessage.value = '';
  failedStage.value = '';
  logs.value = [];
  completedStages.value = [];
  activeStage.value = 'upload';
  uploadProgress.value = null;
  try {
    const request: PackagePatchRequest = {
      config: sshConfig.value,
      packagePath: selectedPackage.value,
      replacementLocalPath: replacement.value.path,
      targetInternalPath: targetInternalPath.value,
      targetLayer: targetLayer.value,
      output: overwrite.value ? { mode: 'overwrite' } : { mode: 'newFile', outputPath: outputPath.value },
    };
    result.value = await remotePackagePatchApi.startPatch(request);
    if (activeStage.value && !completedStages.value.includes(activeStage.value)) {
      completedStages.value.push(activeStage.value);
    }
    log('success', t('remotePackagePatch.logs.patchDone', { path: result.value.outputPath }));
  } catch (err) {
    errorMessage.value = String(err);
    failedStage.value = activeStage.value;
    log('error', errorMessage.value);
  } finally {
    running.value = false;
  }
}

function stageClass(stage: string) {
  if (failedStage.value === stage) return 'bg-red-50 text-red-700';
  if (completedStages.value.includes(stage)) return 'bg-emerald-50 text-emerald-700';
  if (activeStage.value === stage && running.value) return 'bg-sky-50 text-sky-700';
  return 'bg-slate-50 text-slate-500';
}

watch([host, port, username, password], () => {
  connected.value = false;
  connectionMessage.value = '';
});

watch(selectedPackage, (value) => {
  if (value) outputPath.value = defaultPatchedPath(value);
  inventory.value = null;
  selectedCandidate.value = null;
});

watch(candidates, (value) => {
  if (!selectedCandidate.value && value[0]) selectedCandidate.value = value[0];
});

watch(directoryOptions, (value) => {
  if (!selectedDirectoryKey.value && value[0]) selectedDirectoryKey.value = value[0].key;
});

watch(
  () => logs.value.length,
  async () => {
    if (!autoScroll.value) return;
    await nextTick();
    const el = logContainer.value;
    if (!el) return;
    if (el.scrollHeight - el.scrollTop - el.clientHeight < AUTO_SCROLL_THRESHOLD_PX) {
      el.scrollTop = el.scrollHeight;
    }
  },
);

onMounted(async () => {
  recentHosts.value = loadHostHistory();
  const config = await getConfig();
  savedServers.value = config.servers ?? [];
  unlisten = await listen<RemotePackagePatchEvent>('remote-package-patch-event', (event) => {
    const payload = event.payload;
    if (payload.kind === 'stage' && payload.stage) {
      if (payload.stage.startsWith('scan_')) {
        scanStage.value = payload.stage;
      } else {
        markStage(payload.stage);
      }
    } else if (payload.kind === 'log' && payload.message) {
      log(payload.level ?? 'info', payload.message);
    } else if (payload.kind === 'uploadProgress' && payload.sent != null && payload.total != null) {
      uploadProgress.value = { sent: payload.sent, total: payload.total };
    }
  });
});

onBeforeUnmount(() => {
  unlisten?.();
});
</script>

<template>
  <div class="flex-1 overflow-y-auto bg-slate-50">
    <div class="mx-auto flex w-full max-w-[1440px] flex-col gap-5 px-6 py-6">
      <!-- 标题 -->
      <header class="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div class="flex items-start gap-3">
          <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-sky-500 to-blue-600 shadow-sm">
            <PackageSearch class="h-5 w-5 text-white" />
          </div>
          <div>
            <h1 class="text-2xl font-bold text-slate-900">{{ t('remotePackagePatch.title') }}</h1>
            <p class="mt-1 text-sm text-slate-500">{{ t('remotePackagePatch.headerNote') }}</p>
          </div>
        </div>
        <!-- 任务摘要条 -->
        <div class="flex flex-wrap items-center gap-2 rounded-xl border border-slate-200 bg-white px-3 py-2 text-xs shadow-sm">
          <span class="font-semibold text-slate-500">{{ t('remotePackagePatch.summary.label') }}</span>
          <span
            class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-semibold"
            :class="summaryHostText ? 'bg-emerald-50 text-emerald-700' : 'bg-red-50 text-red-600'"
          >
            <Server class="h-3 w-3" />
            {{ summaryHostText || t('remotePackagePatch.connection.hostPlaceholder') }}
          </span>
          <ChevronRight class="h-3 w-3 text-slate-300" />
          <span
            class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-semibold"
            :class="summaryPackageText ? 'bg-emerald-50 text-emerald-700' : 'bg-slate-100 text-slate-500'"
          >
            <FolderOpen class="h-3 w-3" />
            <span class="max-w-[180px] truncate">{{ summaryPackageText || t('remotePackagePatch.summary.package') }}</span>
          </span>
          <ChevronRight class="h-3 w-3 text-slate-300" />
          <span
            class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-semibold"
            :class="summaryFileText ? 'bg-emerald-50 text-emerald-700' : 'bg-slate-100 text-slate-500'"
          >
            <FileUp class="h-3 w-3" />
            <span class="max-w-[160px] truncate">{{ summaryFileText || t('remotePackagePatch.summary.file') }}</span>
          </span>
          <ChevronRight class="h-3 w-3 text-slate-300" />
          <span
            class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-semibold"
            :class="summaryTargetText ? 'bg-emerald-50 text-emerald-700' : 'bg-slate-100 text-slate-500'"
          >
            <span class="max-w-[160px] truncate">{{ summaryTargetText || t('remotePackagePatch.summary.target') }}</span>
          </span>
          <span
            class="ml-auto inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-semibold"
            :class="{
              'bg-emerald-50 text-emerald-700': summaryStatus.tone === 'ok',
              'bg-amber-50 text-amber-700': summaryStatus.tone === 'warn',
              'bg-red-50 text-red-600': summaryStatus.tone === 'err',
            }"
          >
            <Loader2 v-if="summaryStatus.key === 'running'" class="h-3 w-3 animate-spin" />
            <CheckCircle2 v-else-if="summaryStatus.tone === 'ok'" class="h-3 w-3" />
            <ShieldAlert v-else-if="summaryStatus.tone === 'err'" class="h-3 w-3" />
            {{ summaryStatusText }}
          </span>
        </div>
      </header>

      <!-- 步骤进度条 -->
      <section class="rounded-xl border border-slate-200 bg-white px-5 py-4 shadow-sm">
        <div class="flex items-center">
          <template v-for="(step, index) in stepList" :key="step.num">
            <div class="flex items-center gap-2">
              <span class="rpp-step-num" :class="stepNumClass(step.state)">{{ stepNumContent(step) }}</span>
              <span class="text-xs font-semibold" :class="stepLabelClass(step.state)">
                {{ t(`remotePackagePatch.steps.${step.key}`) }}
              </span>
            </div>
            <div
              v-if="index < stepList.length - 1"
              class="mx-3 h-0.5 flex-1 rounded-full"
              :class="stepDividerClass(step)"
            ></div>
          </template>
        </div>
      </section>

      <!-- 主体两列 -->
      <div class="grid grid-cols-12 gap-5">
        <!-- 左：3 张配置卡 -->
        <div class="col-span-12 space-y-5 xl:col-span-8">
          <!-- ============== 1. SSH 连接 ============== -->
          <section class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
            <div class="flex items-center gap-2 border-b border-slate-100 px-5 py-4">
              <span class="rpp-step-num" :class="stepNumClass(stepList[0].state)">{{ stepNumContent(stepList[0]) }}</span>
              <Server class="h-4 w-4 text-sky-600" />
              <h2 class="text-sm font-semibold text-slate-900">{{ t('remotePackagePatch.steps.connect') }}</h2>
              <span
                class="ml-auto rounded-full px-2 py-0.5 text-[11px] font-semibold"
                :class="connected ? 'bg-emerald-100 text-emerald-700' : 'bg-slate-100 text-slate-500'"
              >
                {{ connected ? t('remotePackagePatch.connection.connected') : t('remotePackagePatch.connection.notConnected') }}
              </span>
            </div>
            <div class="grid grid-cols-12 gap-3 p-5">
              <div v-if="savedServers.length > 0" class="col-span-12">
                <label class="rpp-label">{{ t('remotePackagePatch.connection.presetPlaceholder') }}</label>
                <select
                  v-model="selectedServerId"
                  class="rpp-input w-full"
                  :aria-label="t('remotePackagePatch.connection.presetPlaceholder')"
                  @change="applyServer(selectedServerId)"
                >
                  <option value="">{{ t('remotePackagePatch.connection.presetPlaceholder') }}</option>
                  <option v-for="server in savedServers" :key="server.id" :value="server.id">
                    {{ server.name }} / {{ server.host }}
                  </option>
                </select>
              </div>
              <div class="col-span-12 md:col-span-8">
                <label class="rpp-label">{{ t('remotePackagePatch.connection.hostPlaceholder') }}</label>
                <input
                  v-model="host"
                  class="rpp-input w-full font-mono"
                  list="remote-package-host-history"
                  :aria-label="t('remotePackagePatch.connection.hostPlaceholder')"
                  :placeholder="t('remotePackagePatch.connection.hostPlaceholder')"
                  @input="handleHostInput"
                />
                <datalist id="remote-package-host-history">
                  <option v-for="recentHost in recentHosts" :key="recentHost" :value="recentHost" />
                </datalist>
              </div>
              <div class="col-span-12 md:col-span-4">
                <label class="rpp-label">Port</label>
                <input v-model.number="port" class="rpp-input w-full font-mono" type="number" min="1" max="65535" />
              </div>
              <div class="col-span-12 md:col-span-6">
                <label class="rpp-label">{{ t('remotePackagePatch.connection.usernamePlaceholder') }}</label>
                <input v-model="username" class="rpp-input w-full" :placeholder="t('remotePackagePatch.connection.usernamePlaceholder')" />
              </div>
              <div class="col-span-12 md:col-span-6">
                <label class="rpp-label">{{ t('remotePackagePatch.connection.passwordPlaceholder') }}</label>
                <div class="relative">
                  <input
                    v-model="password"
                    class="rpp-input w-full pr-11"
                    :type="showPassword ? 'text' : 'password'"
                    autocomplete="new-password"
                    :placeholder="t('remotePackagePatch.connection.passwordPlaceholder')"
                  />
                  <button
                    type="button"
                    class="absolute inset-y-0 right-0 flex w-11 cursor-pointer items-center justify-center rounded-r-md text-slate-400 transition-colors hover:text-slate-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-sky-500"
                    :aria-label="t(showPassword ? 'remotePackagePatch.connection.hidePassword' : 'remotePackagePatch.connection.showPassword')"
                    :aria-pressed="showPassword"
                    :title="t(showPassword ? 'remotePackagePatch.connection.hidePassword' : 'remotePackagePatch.connection.showPassword')"
                    @click="showPassword = !showPassword"
                  >
                    <EyeOff v-if="showPassword" class="h-4 w-4" aria-hidden="true" />
                    <Eye v-else class="h-4 w-4" aria-hidden="true" />
                  </button>
                </div>
              </div>
              <div class="col-span-12 flex items-center gap-3 pt-1">
                <button class="rpp-primary" :disabled="!canConnect || connectionBusy" @click="testConnection">
                  <Loader2 v-if="connectionBusy" class="h-4 w-4 animate-spin" />
                  <CheckCircle2 v-else class="h-4 w-4" />
                  {{ connectionBusy ? t('remotePackagePatch.connection.testing') : t('remotePackagePatch.connection.test') }}
                </button>
                <div
                  v-if="connectionMessage"
                  class="inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-xs"
                  :class="connected ? 'border-emerald-200 bg-emerald-50 text-emerald-700' : 'border-red-200 bg-red-50 text-red-700'"
                >
                  <CheckCircle2 v-if="connected" class="h-3.5 w-3.5" />
                  <ShieldAlert v-else class="h-3.5 w-3.5" />
                  {{ connectionMessage }}
                </div>
              </div>
            </div>
          </section>

          <!-- ============== 2. 选择远程包 ============== -->
          <section class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
            <div class="flex items-center gap-2 border-b border-slate-100 px-5 py-4">
              <span class="rpp-step-num" :class="stepNumClass(stepList[1].state)">{{ stepNumContent(stepList[1]) }}</span>
              <FolderOpen class="h-4 w-4 text-sky-600" />
              <h2 class="text-sm font-semibold text-slate-900">{{ t('remotePackagePatch.steps.package') }}</h2>
              <span
                v-if="selectedPackage"
                class="ml-auto rounded-full bg-emerald-50 px-2 py-0.5 text-[11px] font-semibold text-emerald-700"
              >
                {{ t('remotePackagePatch.connection.connected') }}
              </span>
            </div>
            <div class="space-y-3 p-5">
              <RemoteDirBrowser
                v-model="selectedPackage"
                :config="connected ? sshConfig : null"
                :disabled="!connected || running"
                @error="log('error', $event)"
              />
              <div
                v-if="selectedPackage"
                class="flex items-start gap-3 rounded-lg border border-slate-200 bg-white p-3"
              >
                <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-sky-50 text-sky-600">
                  <FolderOpen class="h-5 w-5" />
                </div>
                <div class="min-w-0 flex-1">
                  <div class="text-[11px] font-semibold uppercase tracking-wide text-slate-500">
                    {{ t('remotePackagePatch.execution.packageLabel') }}
                  </div>
                  <div class="mt-0.5 break-all font-mono text-xs text-slate-900">{{ selectedPackage }}</div>
                </div>
              </div>
            </div>
          </section>

          <!-- ============== 3. 替换文件与目标 ============== -->
          <section class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
            <div class="flex items-center gap-2 border-b border-slate-100 px-5 py-4">
              <span class="rpp-step-num" :class="stepNumClass(stepList[2].state)">{{ stepNumContent(stepList[2]) }}</span>
              <FileUp class="h-4 w-4 text-sky-600" />
              <h2 class="text-sm font-semibold text-slate-900">{{ t('remotePackagePatch.steps.target') }}</h2>
            </div>
            <div class="space-y-4 p-5">
              <!-- 上排：本地文件 + 包内目标 横向并排 -->
              <div class="grid grid-cols-12 gap-4">
                <!-- 本地替换文件 -->
                <div class="col-span-12 lg:col-span-5">
                  <div class="rpp-label flex items-center gap-1.5">
                    <FileUp class="h-3.5 w-3.5 text-sky-600" />
                    {{ t('remotePackagePatch.target.pickReplacement') }}
                  </div>
                  <button class="rpp-secondary w-full" :disabled="replacementBusy" @click="pickReplacement">
                    <Loader2 v-if="replacementBusy" class="h-4 w-4 animate-spin" />
                    <FileUp v-else class="h-4 w-4" />
                    {{ t('remotePackagePatch.target.pickReplacement') }}
                  </button>
                  <div
                    v-if="replacement"
                    class="mt-2 rounded-md border border-slate-200 bg-white p-2.5 text-xs"
                  >
                    <div class="truncate font-semibold text-slate-800">{{ replacement.name }}</div>
                    <div class="mt-0.5 break-all font-mono text-slate-500">{{ replacement.path }}</div>
                    <div class="mt-0.5 text-slate-400">{{ formatBytes(replacement.size) }}</div>
                  </div>
                </div>

                <!-- 中间箭头 -->
                <div class="hidden items-center justify-center text-slate-300 lg:col-span-2 lg:flex">
                  <ArrowRight class="h-6 w-6" />
                </div>

                <!-- 包内目标路径（候选/目录切换） -->
                <div class="col-span-12 lg:col-span-5">
                  <div class="rpp-label flex items-center gap-1.5">
                    <FolderOpen class="h-3.5 w-3.5 text-sky-600" />
                    {{ t('remotePackagePatch.target.pathLabel') }}
                  </div>
                  <template v-if="inventory">
                    <!-- 同名候选 / 目录 二选一 -->
                    <div class="grid grid-cols-2 gap-2">
                      <button
                        type="button"
                        class="rpp-segment"
                        :class="targetMode === 'candidate' ? 'rpp-segment-active' : ''"
                        @click="targetMode = 'candidate'"
                      >
                        {{ t('remotePackagePatch.target.modeCandidate') }}
                      </button>
                      <button
                        type="button"
                        class="rpp-segment"
                        :class="targetMode === 'directory' ? 'rpp-segment-active' : ''"
                        @click="targetMode = 'directory'"
                      >
                        {{ t('remotePackagePatch.target.modeDirectory') }}
                      </button>
                    </div>

                    <!-- 候选列表 -->
                    <div
                      v-if="targetMode === 'candidate'"
                      class="mt-2 max-h-44 overflow-auto rounded-md border border-slate-200"
                    >
                      <label
                        v-for="entry in candidates"
                        :key="`${layerKey(entry.layer)}:${entry.path}`"
                        class="flex cursor-pointer gap-2 border-b border-slate-100 p-2 text-xs hover:bg-sky-50"
                      >
                        <input v-model="selectedCandidate" type="radio" :value="entry" />
                        <span class="min-w-0 flex-1">
                          <span class="block break-all font-mono text-slate-800">{{ entry.path }}</span>
                          <span class="text-slate-500">{{ formatLayer(entry.layer) }}</span>
                        </span>
                      </label>
                      <div v-if="candidates.length === 0" class="p-3 text-xs text-slate-500">
                        {{ t('remotePackagePatch.target.noCandidates') }}
                      </div>
                    </div>

                    <!-- 目录选择 -->
                    <div v-else-if="targetMode === 'directory'" class="mt-2 space-y-2">
                      <button
                        type="button"
                        class="rpp-secondary min-h-10 w-full justify-start overflow-hidden text-left"
                        :disabled="directoryOptions.length === 0"
                        :aria-label="t('remotePackagePatch.target.directoryDialogTitle')"
                        @click="openDirectoryPicker"
                      >
                        <FolderOpen class="h-4 w-4 shrink-0" />
                        <span class="min-w-0 flex-1 truncate font-mono text-xs">
                          {{ selectedDirectory?.label || t('remotePackagePatch.target.directoryPlaceholder') }}
                        </span>
                        <ChevronRight class="h-4 w-4 shrink-0 text-slate-400" />
                      </button>
                      <input v-model="internalFileName" class="rpp-input w-full" :placeholder="t('remotePackagePatch.target.fileNamePlaceholder')" />
                    </div>
                  </template>
                  <div v-else class="rounded-md border border-dashed border-slate-200 bg-slate-50 p-3 text-xs text-slate-500">
                    {{ t('remotePackagePatch.target.scanHint') }}
                  </div>
                </div>
              </div>

              <!-- 扫描按钮 + 进度 -->
              <div class="flex flex-wrap items-center gap-3">
                <button class="rpp-primary" :disabled="!canScan" @click="scanPackage">
                  <Loader2 v-if="scanBusy" class="h-4 w-4 animate-spin" />
                  <Play v-else class="h-4 w-4" />
                  {{ scanBusy ? t('remotePackagePatch.target.scanning') : t('remotePackagePatch.target.scan') }}
                </button>
                <div v-if="scanBusy && scanStage" class="inline-flex items-center gap-1.5 text-xs text-slate-500">
                  <span class="rpp-pulse-dot"></span>
                  {{ stageText(scanStage) }}
                </div>
                <div v-if="scanError" class="inline-flex items-center gap-1.5 rounded-md bg-red-50 px-3 py-1.5 text-xs text-red-700">
                  <ShieldAlert class="h-3.5 w-3.5" />
                  {{ scanError }}
                </div>
              </div>

              <!-- 目标路径 + 所在层 大块展示 -->
              <div
                v-if="inventory"
                class="overflow-hidden rounded-xl border"
                :class="targetErrorText ? 'border-red-200' : 'border-slate-200'"
              >
                <div class="flex items-center gap-2 border-b border-slate-200 bg-slate-50 px-4 py-2.5 text-xs font-semibold text-slate-700">
                  <Sparkles class="h-3.5 w-3.5 text-sky-600" />
                  {{ t('remotePackagePatch.target.pathLabel') }}
                </div>
                <div class="grid grid-cols-12 gap-3 p-4">
                  <div class="col-span-12 md:col-span-6">
                    <div class="rpp-label-mini">{{ t('remotePackagePatch.target.pathLabel') }}</div>
                    <div
                      class="mt-1 break-all rounded-md px-2.5 py-2 font-mono text-xs"
                      :class="targetErrorText ? 'bg-red-50 text-red-700' : 'bg-slate-50 text-slate-900'"
                    >
                      {{ targetInternalPath || '-' }}
                    </div>
                    <div v-if="targetErrorText" class="mt-1 text-[11px] text-red-600">{{ targetErrorText }}</div>
                  </div>
                  <div class="col-span-12 md:col-span-3">
                    <div class="rpp-label-mini">{{ t('remotePackagePatch.target.layerAuto') }}</div>
                    <div class="mt-1 flex items-center gap-1.5 rounded-md bg-slate-50 px-2.5 py-2 text-xs text-slate-700">
                      <FolderOpen class="h-3.5 w-3.5 text-sky-600" />
                      {{ formatLayer(targetLayer) }}
                    </div>
                  </div>
                  <div class="col-span-12 md:col-span-3">
                    <div class="rpp-label-mini">{{ t('remotePackagePatch.steps.target') }}</div>
                    <div class="mt-1 truncate rounded-md bg-slate-50 px-2.5 py-2 font-mono text-xs text-slate-700">
                      {{ summaryTargetLayer || '-' }}
                    </div>
                  </div>
                </div>

                <!-- 输出 + 覆盖 Toggle -->
                <div class="space-y-2.5 border-t border-slate-200 bg-white px-4 py-3">
                  <div class="flex items-center justify-between">
                    <div>
                      <div class="text-sm font-semibold text-slate-800">{{ t('remotePackagePatch.target.overwriteTitle') }}</div>
                      <div class="mt-0.5 text-[11px] text-slate-500">{{ t('remotePackagePatch.target.overwriteNote') }}</div>
                    </div>
                    <button
                      type="button"
                      class="rpp-toggle"
                      :class="overwrite ? 'rpp-toggle-on' : ''"
                      role="switch"
                      :aria-checked="overwrite"
                      @click="overwrite = !overwrite"
                    >
                      <span class="rpp-toggle-knob"></span>
                    </button>
                  </div>
                  <div v-if="!overwrite" class="grid grid-cols-12 gap-3">
                    <div class="col-span-12 md:col-span-9">
                      <label class="rpp-label">{{ t('remotePackagePatch.target.outputPlaceholder') }}</label>
                      <input
                        v-model="outputPath"
                        class="rpp-input w-full font-mono"
                        :placeholder="t('remotePackagePatch.target.outputPlaceholder')"
                      />
                    </div>
                    <div class="col-span-12 md:col-span-3 flex items-end">
                      <button type="button" class="rpp-secondary w-full" @click="restoreDefaultOutputPath">
                        {{ t('remotePackagePatch.target.restoreDefault') }}
                      </button>
                    </div>
                  </div>
                  <label
                    v-else
                    class="flex items-start gap-2 rounded-md border border-amber-200 bg-amber-50 p-2.5 text-xs text-amber-900"
                  >
                    <input v-model="overwriteConfirmed" type="checkbox" class="mt-0.5" />
                    {{ t('remotePackagePatch.target.overwriteConfirm') }}
                  </label>
                </div>
              </div>

              <!-- 高级：手动模式 -->
              <details v-if="inventory" class="rounded-lg border border-slate-200 bg-white">
                <summary class="flex cursor-pointer items-center gap-2 px-3 py-2.5 text-xs font-semibold text-slate-600 hover:bg-slate-50">
                  <ChevronRight class="h-3.5 w-3.5 text-slate-400" />
                  {{ t('remotePackagePatch.target.advancedManual') }}
                  <span class="ml-auto text-[11px] text-slate-400">{{ t('remotePackagePatch.target.advancedHint') }}</span>
                </summary>
                <div class="px-3 pb-3">
                  <button
                    type="button"
                    class="rpp-segment mb-2"
                    :class="targetMode === 'manual' ? 'rpp-segment-active' : ''"
                    @click="targetMode = 'manual'"
                  >
                    {{ t('remotePackagePatch.target.modeManual') }}
                  </button>
                  <input
                    v-model="manualInternalPath"
                    class="rpp-input w-full font-mono"
                    :placeholder="t('remotePackagePatch.target.manualPlaceholder')"
                  />
                </div>
              </details>
            </div>
          </section>
        </div>

        <!-- 右：执行 + 日志 + 结果 sticky -->
        <div class="col-span-12 xl:col-span-4">
          <div class="space-y-5 xl:sticky xl:top-4">
            <!-- 执行卡 -->
            <section class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
              <div class="flex items-center gap-2 border-b border-slate-100 px-5 py-4">
                <span class="rpp-step-num" :class="stepNumClass(stepList[3].state)">{{ stepNumContent(stepList[3]) }}</span>
                <h2 class="text-sm font-semibold text-slate-900">{{ t('remotePackagePatch.steps.execute') }}</h2>
                <span
                  class="ml-auto rounded-full px-2 py-0.5 text-[11px] font-semibold"
                  :class="{
                    'bg-sky-50 text-sky-700': running,
                    'bg-emerald-50 text-emerald-700': Boolean(result),
                    'bg-red-50 text-red-700': Boolean(failedStage),
                    'bg-slate-100 text-slate-500': !running && !result && !failedStage,
                  }"
                >
                  {{ running ? t('remotePackagePatch.execution.running') : result ? t('remotePackagePatch.execution.done') : failedStage ? t('remotePackagePatch.execution.failed') : t('remotePackagePatch.execution.start') }}
                </span>
              </div>
              <div class="space-y-3 p-5">
                <button class="rpp-primary w-full" :disabled="!canStartPatch" @click="startPatch">
                  <Loader2 v-if="running" class="h-4 w-4 animate-spin" />
                  <Play v-else class="h-4 w-4" />
                  {{ running ? t('remotePackagePatch.execution.running') : t('remotePackagePatch.execution.start') }}
                </button>

                <!-- 上传进度 -->
                <div v-if="running && activeStage === 'upload' && uploadProgress">
                  <div class="mb-1 flex justify-between text-xs text-slate-500">
                    <span>{{ t('remotePackagePatch.execution.uploading') }}</span>
                    <span>{{ uploadPercent.toFixed(0) }}%</span>
                  </div>
                  <div class="h-2 overflow-hidden rounded-full bg-slate-100">
                    <div class="h-full bg-sky-500 transition-all" :style="{ width: `${uploadPercent}%` }"></div>
                  </div>
                </div>
                <div
                  v-else-if="uploadProgress && activeStage !== 'upload'"
                  class="inline-flex items-center gap-1 text-xs text-emerald-600"
                >
                  <CheckCircle2 class="h-3.5 w-3.5" />
                  {{ t('remotePackagePatch.execution.uploaded') }}
                </div>

                <!-- 错误提示 -->
                <div
                  v-if="errorMessage"
                  class="flex items-start gap-2 rounded-md border border-red-200 bg-red-50 p-3 text-xs text-red-800"
                >
                  <ShieldAlert class="h-4 w-4 shrink-0" />
                  <div class="min-w-0">
                    <div class="font-semibold">{{ t('remotePackagePatch.execution.failed') }}</div>
                    <div class="mt-1 break-all">{{ errorMessage }}</div>
                  </div>
                </div>

                <!-- 步骤状态列表 -->
                <ol class="space-y-1">
                  <li
                    v-for="stage in stageList"
                    :key="stage"
                    class="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs font-medium"
                    :class="stageClass(stage)"
                  >
                    <CheckCircle2 v-if="completedStages.includes(stage)" class="h-3.5 w-3.5 shrink-0" />
                    <Loader2 v-else-if="activeStage === stage && running" class="h-3.5 w-3.5 shrink-0 animate-spin" />
                    <ShieldAlert v-else-if="failedStage === stage" class="h-3.5 w-3.5 shrink-0" />
                    <span v-else class="h-3.5 w-3.5 shrink-0 rounded-full border border-current opacity-40"></span>
                    {{ stageText(stage) }}
                  </li>
                </ol>

                <!-- 覆盖未确认警告 -->
                <div
                  v-if="overwrite && !overwriteConfirmed"
                  class="flex items-start gap-2 rounded-md bg-amber-50 p-3 text-xs text-amber-800"
                >
                  <ShieldAlert class="h-4 w-4 shrink-0" />
                  {{ t('remotePackagePatch.execution.overwriteNeedConfirm') }}
                </div>
              </div>
            </section>

            <!-- 日志卡 -->
            <section class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
              <div class="flex items-center gap-2 border-b border-slate-100 px-4 py-2.5">
                <Terminal class="h-4 w-4 text-slate-500" />
                <h2 class="text-xs font-semibold text-slate-700">{{ t('remotePackagePatch.execution.noLogs') }}</h2>
                <span class="text-[11px] text-slate-400">{{ t('remotePackagePatch.logs.lineCount', { count: logs.length }) }}</span>
                <div class="ml-auto flex items-center gap-1">
                  <label class="mr-1 inline-flex cursor-pointer items-center gap-1 text-[11px] text-slate-500">
                    <input v-model="autoScroll" type="checkbox" class="h-3 w-3" />
                    {{ t('remotePackagePatch.logs.autoScroll') }}
                  </label>
                  <button
                    type="button"
                    class="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100"
                    :title="t('remotePackagePatch.logs.copy')"
                    @click="copyLogs"
                  >
                    <ClipboardCopy class="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    class="flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100"
                    :title="t('remotePackagePatch.logs.clear')"
                    @click="clearLogs"
                  >
                    <Trash2 class="h-3.5 w-3.5" />
                  </button>
                </div>
              </div>
              <div
                ref="logContainer"
                class="h-72 overflow-auto bg-slate-950 p-3 font-mono text-[12px] text-slate-100"
              >
                <div
                  v-for="(entry, index) in logs"
                  :key="index"
                  class="rpp-log-line"
                >
                  <span class="rpp-log-lvl" :class="logLevelClass(entry.level)"></span>
                  <span
                    class="rpp-log-msg"
                    :class="{
                      'text-red-300': entry.level === 'error',
                      'text-amber-300': entry.level === 'warn',
                      'text-emerald-300': entry.level === 'success' || entry.level === 'ok',
                      'text-slate-100': entry.level !== 'error' && entry.level !== 'warn' && entry.level !== 'success' && entry.level !== 'ok',
                    }"
                  >[{{ entry.level }}] {{ entry.message }}</span>
                </div>
                <div v-if="logs.length === 0" class="text-slate-500">{{ t('remotePackagePatch.execution.noLogs') }}</div>
              </div>
            </section>

            <!-- 结果摘要 -->
            <section
              v-if="result"
              class="overflow-hidden rounded-xl border border-emerald-200 bg-white shadow-sm"
            >
              <div class="flex items-center gap-2 border-b border-emerald-200 bg-emerald-50 px-4 py-2.5">
                <CheckCircle2 class="h-4 w-4 text-emerald-600" />
                <h2 class="text-xs font-semibold text-emerald-900">{{ t('remotePackagePatch.execution.done') }}</h2>
              </div>
              <div class="space-y-2.5 p-4">
                <div class="rpp-kv">
                  <div class="rpp-kv-k">{{ t('remotePackagePatch.execution.outputLabel') }}</div>
                  <div class="rpp-kv-v">{{ result.outputPath }}</div>
                </div>
                <div class="grid grid-cols-2 gap-2.5">
                  <div class="rpp-kv">
                    <div class="rpp-kv-k">{{ t('remotePackagePatch.execution.md5Label') }}</div>
                    <div class="rpp-kv-v">{{ result.targetMd5 }}</div>
                  </div>
                  <div class="rpp-kv">
                    <div class="rpp-kv-k">{{ t('remotePackagePatch.execution.manifestsLabel') }}</div>
                    <div class="rpp-kv-v">
                      <template v-if="result.updatedManifests.length > 0">
                        <div v-for="manifest in result.updatedManifests" :key="manifest">{{ manifest }}</div>
                      </template>
                      <span v-else>{{ t('remotePackagePatch.execution.manifestsNone') }}</span>
                    </div>
                  </div>
                </div>
                <div v-if="result.backupPath" class="rpp-kv">
                  <div class="rpp-kv-k">{{ t('remotePackagePatch.execution.backupLabel') }}</div>
                  <div class="rpp-kv-v">{{ result.backupPath }}</div>
                </div>
                <div class="rpp-kv">
                  <div class="rpp-kv-k">{{ t('remotePackagePatch.execution.workdirLabel') }}</div>
                  <div class="rpp-kv-v">{{ result.workdir }}</div>
                </div>
                <button class="rpp-secondary w-full" @click="copyResultSummary">
                  <ClipboardCopy class="h-4 w-4" />
                  {{ t('remotePackagePatch.execution.copySummary') }}
                </button>
              </div>
            </section>
          </div>
        </div>
      </div>
    </div>

    <!-- Toast -->
    <Transition name="rpp-fade">
      <div
        v-if="summaryToast"
        class="fixed bottom-6 left-1/2 z-50 -translate-x-1/2 rounded-lg bg-slate-900 px-4 py-2 text-xs font-semibold text-white shadow-lg"
      >
        {{ summaryToast }}
      </div>
    </Transition>
  </div>

  <BrowserDialog
    :open="directoryPickerOpen"
    :title="t('remotePackagePatch.target.directoryDialogTitle')"
    :hint="t('remotePackagePatch.target.directoryDialogHint')"
    :close-label="t('remotePackagePatch.target.directoryCancel')"
    @close="closeDirectoryPicker"
  >
    <div class="border-b border-slate-200 bg-slate-50 px-5 py-3">
      <label class="flex items-center gap-3 text-xs font-medium text-slate-600">
        <span class="shrink-0">{{ t('remotePackagePatch.target.directoryLayer') }}</span>
        <select
          :value="browsingLayerKey"
          class="rpp-input min-w-0 flex-1 bg-white"
          @change="changeBrowsingLayer(($event.target as HTMLSelectElement).value)"
        >
          <option v-for="item in sortedDirectoryLayers" :key="item.key" :value="item.key">
            {{ formatLayer(item.layer) }}
          </option>
        </select>
      </label>
    </div>

    <nav
      class="flex min-h-12 items-center gap-1 overflow-x-auto border-b border-slate-200 px-4 py-2"
      :aria-label="t('remotePackagePatch.target.directoryCurrentPath')"
    >
      <button
        type="button"
        class="mr-1 flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-md text-slate-500 hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-30"
        :disabled="!browsingDirectoryPath"
        :aria-label="t('remotePackagePatch.target.directoryUp')"
        @click="browseParentDirectory"
      >
        <ArrowLeft class="h-4 w-4" />
      </button>
      <template v-for="(crumb, index) in browsingBreadcrumbs" :key="crumb.path">
        <ChevronRight v-if="index > 0" class="h-3.5 w-3.5 shrink-0 text-slate-300" />
        <button
          type="button"
          class="shrink-0 cursor-pointer rounded-md px-2 py-1 font-mono text-xs text-slate-600 hover:bg-slate-100 hover:text-sky-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50"
          @click="browsingDirectoryPath = crumb.path"
        >
          {{ crumb.label }}
        </button>
      </template>
    </nav>

    <div class="min-h-64 flex-1 overflow-y-auto p-3">
      <button
        v-for="directory in browsingChildren"
        :key="directory.key"
        type="button"
        class="flex w-full cursor-pointer items-center gap-3 rounded-lg px-3 py-2.5 text-left transition-colors hover:bg-sky-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-sky-500/50"
        @click="browsingDirectoryPath = directory.path"
      >
        <Folder class="h-5 w-5 shrink-0 fill-amber-100 text-amber-500" />
        <span class="min-w-0 flex-1 truncate font-mono text-sm text-slate-700">
          {{ directory.path.split('/').at(-1) }}
        </span>
        <ChevronRight class="h-4 w-4 shrink-0 text-slate-300" />
      </button>
      <div
        v-if="browsingChildren.length === 0"
        class="flex min-h-48 flex-col items-center justify-center text-center text-slate-400"
      >
        <FolderOpen class="mb-2 h-8 w-8" />
        <p class="text-sm">{{ t('remotePackagePatch.target.directoryEmpty') }}</p>
      </div>
    </div>

    <footer class="flex flex-col gap-3 border-t border-slate-200 bg-slate-50 px-5 py-4 sm:flex-row sm:items-center">
      <div class="min-w-0 flex-1">
        <div class="text-xs text-slate-500">{{ t('remotePackagePatch.target.directoryCurrentPath') }}</div>
        <div class="mt-0.5 truncate font-mono text-xs font-medium text-slate-700">
          {{ browsingDirectoryPath || t('remotePackagePatch.target.directoryRoot') }}
        </div>
      </div>
      <div class="flex justify-end gap-2">
        <button type="button" class="rpp-secondary" @click="closeDirectoryPicker">
          {{ t('remotePackagePatch.target.directoryCancel') }}
        </button>
        <button type="button" class="rpp-primary" :disabled="!browsingDirectory" @click="chooseBrowsingDirectory">
          {{ t('remotePackagePatch.target.directoryChoose') }}
        </button>
      </div>
    </footer>
  </BrowserDialog>
</template>

<style scoped>
@reference "../style.css";

.rpp-input {
  @apply rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none transition-colors focus:border-sky-500 focus:ring-2 focus:ring-sky-100 disabled:cursor-not-allowed disabled:bg-slate-50 disabled:text-slate-400;
}

.rpp-label {
  @apply mb-1.5 block text-xs font-semibold text-slate-600;
}

.rpp-label-mini {
  @apply text-[11px] font-semibold uppercase tracking-wide text-slate-500;
}

.rpp-primary {
  @apply inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg bg-sky-600 px-3.5 py-2 text-sm font-semibold text-white transition-colors hover:bg-sky-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:bg-slate-300;
}

.rpp-secondary {
  @apply inline-flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white px-3.5 py-2 text-sm font-semibold text-slate-700 transition-colors hover:border-sky-200 hover:bg-sky-50 hover:text-sky-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50;
}

.rpp-segment {
  @apply cursor-pointer rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm font-semibold text-slate-600 transition-colors hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-500/50 focus-visible:ring-offset-2;
}

.rpp-segment-active {
  @apply border-sky-200 bg-sky-50 text-sky-700;
}

/* 步骤进度条数字 */
.rpp-step-num {
  @apply flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-[11px] font-bold transition-all;
}

.rpp-step-idle {
  @apply bg-slate-100 text-slate-400;
}

.rpp-step-active {
  @apply bg-sky-600 text-white;
  box-shadow: 0 0 0 4px rgba(2, 132, 199, 0.15);
}

.rpp-step-done {
  @apply bg-emerald-500 text-white;
}

.rpp-step-failed {
  @apply bg-red-500 text-white;
}

.rpp-div-done {
  @apply bg-emerald-400;
}

.rpp-div-active {
  background: linear-gradient(90deg, #10b981 0%, #cbd5e1 50%, #cbd5e1 100%);
}

/* Toggle 开关 */
.rpp-toggle {
  position: relative;
  display: inline-flex;
  align-items: center;
  width: 40px;
  height: 22px;
  border-radius: 999px;
  background: #cbd5e1;
  cursor: pointer;
  transition: background 0.2s;
  flex-shrink: 0;
  border: none;
  padding: 0;
}

.rpp-toggle-knob {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 18px;
  height: 18px;
  background: #fff;
  border-radius: 50%;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.15);
  transition: transform 0.2s;
}

.rpp-toggle-on {
  background: #f59e0b;
}

.rpp-toggle-on .rpp-toggle-knob {
  transform: translateX(18px);
}

/* 脉冲点 */
.rpp-pulse-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #0284c7;
  box-shadow: 0 0 0 0 rgba(2, 132, 199, 0.6);
  animation: rpp-pulse 1.6s infinite;
}

@keyframes rpp-pulse {
  0% {
    box-shadow: 0 0 0 0 rgba(2, 132, 199, 0.6);
  }

  70% {
    box-shadow: 0 0 0 6px rgba(2, 132, 199, 0);
  }

  100% {
    box-shadow: 0 0 0 0 rgba(2, 132, 199, 0);
  }
}

@media (prefers-reduced-motion: reduce) {
  .rpp-pulse-dot {
    animation: none;
  }
}

/* 日志 */
.rpp-log-line {
  display: flex;
  gap: 8px;
  align-items: stretch;
  padding: 1px 4px;
  border-radius: 3px;
  line-height: 1.55;
}

.rpp-log-lvl {
  flex-shrink: 0;
  width: 3px;
  border-radius: 2px;
}

.rpp-log-msg {
  flex: 1;
  white-space: pre-wrap;
  word-break: break-word;
}

.rpp-lvl-info {
  background: #38bdf8;
}

.rpp-lvl-warn {
  background: #f59e0b;
}

.rpp-lvl-error {
  background: #ef4444;
}

.rpp-lvl-success {
  background: #10b981;
}

/* KV 摘要块 */
.rpp-kv {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 10px 12px;
  background: #f8fafc;
  border-radius: 8px;
  min-width: 0;
}

.rpp-kv-k {
  font-size: 11px;
  font-weight: 600;
  color: #64748b;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.rpp-kv-v {
  font-family: ui-monospace, "Cascadia Code", Menlo, Consolas, monospace;
  font-size: 12px;
  color: #0f172a;
  word-break: break-all;
}

/* Toast 过渡 */
.rpp-fade-enter-active,
.rpp-fade-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}

.rpp-fade-enter-from,
.rpp-fade-leave-to {
  opacity: 0;
  transform: translate(-50%, 8px);
}
</style>
