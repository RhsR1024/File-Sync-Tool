<script setup lang="ts">
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  CheckCircle2,
  FileUp,
  FolderOpen,
  KeyRound,
  Loader2,
  PackageSearch,
  Play,
  Server,
  ShieldAlert,
} from 'lucide-vue-next';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

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
  buildRemotePackagePatchEnableSshRequest,
  composeInternalTargetPath,
  defaultPatchedPath,
  formatBytes,
  layerKey,
  replacementName,
  resolveRemotePackagePatchSshPort,
  shouldAttemptRemotePackagePatchAutoEnable,
  targetCandidates,
  validateInternalTargetPath,
  visibleStages,
} from '@/lib/remotePackagePatch';

type AuthMode = 'password' | 'keyFile';
type TargetMode = 'candidate' | 'directory' | 'manual';

interface DirectoryOption {
  key: string;
  path: string;
  layer: InternalLayer;
  label: string;
}

defineOptions({ name: 'RemotePackagePatchPage' });

const { t, te } = useI18n();

const savedServers = ref<DeployServer[]>([]);
const selectedServerId = ref('');
const host = ref('');
const port = ref(REMOTE_PACKAGE_PATCH_DEFAULT_SSH_PORT);
const username = ref('root');
const authMode = ref<AuthMode>('password');
const password = ref('');
const keyPath = ref('');
const passphrase = ref('');

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

const AUTO_SCROLL_THRESHOLD_PX = 48;

let unlisten: UnlistenFn | null = null;

const sshConfig = computed<RemoteSshConfig>(() => ({
  host: host.value.trim(),
  port: resolveRemotePackagePatchSshPort(port.value),
  username: username.value.trim(),
  auth:
    authMode.value === 'password'
      ? { kind: 'password', password: password.value }
      : { kind: 'keyFile', keyPath: keyPath.value.trim(), passphrase: passphrase.value || null },
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
    const normalized = entry.path.replace(/\\/g, '/').replace(/^\.\/+/, '').replace(/\/+$/g, '');
    const directory =
      entry.kind === 'dir'
        ? normalized
        : normalized.includes('/')
          ? normalized.slice(0, normalized.lastIndexOf('/'))
          : '';
    if (!directory) continue;
    const key = `${layerKey(entry.layer)}::${directory}`;
    if (!map.has(key)) {
      map.set(key, {
        key,
        path: directory,
        layer: entry.layer,
        label: `${formatLayer(entry.layer)} / ${directory}`,
      });
    }
  }
  return Array.from(map.values()).sort((left, right) => left.label.localeCompare(right.label));
});

const selectedDirectory = computed(() =>
  directoryOptions.value.find((option) => option.key === selectedDirectoryKey.value) ?? null,
);

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
  if (authMode.value === 'password') return password.value.length > 0;
  return keyPath.value.trim().length > 0;
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

const currentStep = computed(() => {
  if (!connected.value) return 1;
  if (!selectedPackage.value) return 2;
  if (!inventory.value || !replacement.value || targetError.value !== null) return 3;
  return 4;
});

const stageList = computed(() =>
  visibleStages({ overwrite: overwrite.value, layer: targetLayer.value }),
);

const uploadPercent = computed(() => {
  if (!uploadProgress.value) return 0;
  return Math.min(100, (uploadProgress.value.sent / Math.max(1, uploadProgress.value.total)) * 100);
});

function stepBadgeClass(step: number) {
  if (step < currentStep.value) return 'bg-emerald-100 text-emerald-700';
  if (step === currentStep.value) return 'bg-sky-600 text-white';
  return 'bg-slate-100 text-slate-400';
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

function applyServer(serverId: string) {
  const server = savedServers.value.find((item) => item.id === serverId);
  if (!server) return;
  host.value = server.host;
  port.value = resolveRemotePackagePatchSshPort(server.port);
  username.value = server.user || 'root';
  password.value = server.password || '';
  authMode.value = 'password';
}

async function runConnectionProbe(): Promise<string> {
  const message = await remotePackagePatchApi.testConnection(sshConfig.value);
  const displayMessage = message || 'OK';
  connectionMessage.value = displayMessage;
  connected.value = true;
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

async function pickPrivateKey() {
  const picked = await remotePackagePatchApi.pickLocalFile('privateKey');
  if (picked) keyPath.value = picked.path;
}

async function pickReplacement() {
  replacementBusy.value = true;
  try {
    const picked = await remotePackagePatchApi.pickLocalFile('replacement');
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
    log('info', t('remotePackagePatch.logs.patchDone', { path: result.value.outputPath }));
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

watch([host, port, username, password, keyPath, passphrase, authMode], () => {
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
    await nextTick();
    const el = logContainer.value;
    if (!el) return;
    if (el.scrollHeight - el.scrollTop - el.clientHeight < AUTO_SCROLL_THRESHOLD_PX) {
      el.scrollTop = el.scrollHeight;
    }
  },
);

onMounted(async () => {
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
  <div class="flex-1 overflow-y-auto bg-slate-100">
    <div class="mx-auto flex w-full max-w-7xl flex-col gap-4 px-5 py-5">
      <header class="flex flex-col gap-3 border-b border-slate-200 pb-4 md:flex-row md:items-center md:justify-between">
        <div class="flex items-center gap-2">
          <PackageSearch class="h-6 w-6 text-sky-600" />
          <h1 class="text-2xl font-bold text-slate-950">{{ t('remotePackagePatch.title') }}</h1>
        </div>
        <div class="rounded-md border border-slate-200 bg-white px-3 py-2 text-xs text-slate-600">
          {{ t('remotePackagePatch.headerNote') }}
        </div>
      </header>

      <div class="grid grid-cols-1 gap-4 xl:grid-cols-[400px_minmax(0,1fr)] xl:items-start">
        <section class="rounded-lg border border-slate-200 bg-white p-4">
          <div class="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-800">
            <span class="rpp-step-badge" :class="stepBadgeClass(1)">1</span>
            <Server class="h-4 w-4 text-sky-600" />
            {{ t('remotePackagePatch.steps.connect') }}
            <span
              class="ml-auto rounded-full px-2 py-0.5 text-[11px] font-semibold"
              :class="connected ? 'bg-emerald-100 text-emerald-700' : 'bg-slate-100 text-slate-500'"
            >
              {{ connected ? t('remotePackagePatch.connection.connected') : t('remotePackagePatch.connection.notConnected') }}
            </span>
          </div>
          <div class="space-y-3">
            <select
              v-if="savedServers.length > 0"
              v-model="selectedServerId"
              class="rpp-input w-full"
              @change="applyServer(selectedServerId)"
            >
              <option value="">{{ t('remotePackagePatch.connection.presetPlaceholder') }}</option>
              <option v-for="server in savedServers" :key="server.id" :value="server.id">
                {{ server.name }} / {{ server.host }}
              </option>
            </select>
            <div class="grid grid-cols-[minmax(0,1fr)_92px] gap-2">
              <input v-model="host" class="rpp-input" :placeholder="t('remotePackagePatch.connection.hostPlaceholder')" />
              <input v-model.number="port" class="rpp-input" type="number" min="1" max="65535" />
            </div>
            <input v-model="username" class="rpp-input w-full" :placeholder="t('remotePackagePatch.connection.usernamePlaceholder')" />
            <div class="grid grid-cols-2 gap-2">
              <button
                type="button"
                class="rpp-segment"
                :class="authMode === 'password' ? 'rpp-segment-active' : ''"
                @click="authMode = 'password'"
              >
                {{ t('remotePackagePatch.connection.authPassword') }}
              </button>
              <button
                type="button"
                class="rpp-segment"
                :class="authMode === 'keyFile' ? 'rpp-segment-active' : ''"
                @click="authMode = 'keyFile'"
              >
                {{ t('remotePackagePatch.connection.authKeyFile') }}
              </button>
            </div>
            <input
              v-if="authMode === 'password'"
              v-model="password"
              class="rpp-input w-full"
              type="password"
              autocomplete="new-password"
              :placeholder="t('remotePackagePatch.connection.passwordPlaceholder')"
            />
            <div v-else class="space-y-2">
              <div class="flex gap-2">
                <input v-model="keyPath" class="rpp-input min-w-0 flex-1" :placeholder="t('remotePackagePatch.connection.keyPathPlaceholder')" />
                <button
                  type="button"
                  class="rpp-secondary shrink-0"
                  :title="t('remotePackagePatch.connection.pickKey')"
                  :aria-label="t('remotePackagePatch.connection.pickKey')"
                  @click="pickPrivateKey"
                >
                  <KeyRound class="h-4 w-4" />
                </button>
              </div>
              <input
                v-model="passphrase"
                class="rpp-input w-full"
                type="password"
                autocomplete="new-password"
                :placeholder="t('remotePackagePatch.connection.passphrasePlaceholder')"
              />
            </div>
            <button class="rpp-primary w-full" :disabled="!canConnect || connectionBusy" @click="testConnection">
              <Loader2 v-if="connectionBusy" class="h-4 w-4 animate-spin" />
              <CheckCircle2 v-else class="h-4 w-4" />
              {{ connectionBusy ? t('remotePackagePatch.connection.testing') : t('remotePackagePatch.connection.test') }}
            </button>
            <div
              v-if="connectionMessage"
              class="rounded-md px-3 py-2 text-xs"
              :class="connected ? 'bg-emerald-50 text-emerald-700' : 'bg-red-50 text-red-700'"
            >
              {{ connectionMessage }}
            </div>
          </div>
        </section>

        <section class="rounded-lg border border-slate-200 bg-white p-4">
          <div class="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-800">
            <span class="rpp-step-badge" :class="stepBadgeClass(2)">2</span>
            <FolderOpen class="h-4 w-4 text-sky-600" />
            {{ t('remotePackagePatch.steps.package') }}
          </div>
          <RemoteDirBrowser
            v-model="selectedPackage"
            :config="connected ? sshConfig : null"
            :disabled="!connected || running"
            @error="log('error', $event)"
          />
          <div v-if="selectedPackage" class="mt-3 rounded-md bg-slate-50 p-2 text-xs">
            <div class="font-medium text-slate-600">{{ t('remotePackagePatch.execution.packageLabel') }}</div>
            <div class="mt-1 break-all font-mono text-slate-800">{{ selectedPackage }}</div>
          </div>
        </section>

        <section class="rounded-lg border border-slate-200 bg-white p-4">
          <div class="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-800">
            <span class="rpp-step-badge" :class="stepBadgeClass(3)">3</span>
            <FileUp class="h-4 w-4 text-sky-600" />
            {{ t('remotePackagePatch.steps.target') }}
          </div>
          <div class="space-y-3">
            <button class="rpp-secondary w-full" :disabled="replacementBusy" @click="pickReplacement">
              <Loader2 v-if="replacementBusy" class="h-4 w-4 animate-spin" />
              <FileUp v-else class="h-4 w-4" />
              {{ t('remotePackagePatch.target.pickReplacement') }}
            </button>
            <div v-if="replacement" class="rounded-md bg-slate-50 p-2 text-xs">
              <div class="font-medium text-slate-700">{{ replacement.name }}</div>
              <div class="mt-1 break-all font-mono text-slate-500">{{ replacement.path }}</div>
              <div class="mt-1 text-slate-500">{{ formatBytes(replacement.size) }}</div>
            </div>
            <button class="rpp-primary w-full" :disabled="!canScan" @click="scanPackage">
              <Loader2 v-if="scanBusy" class="h-4 w-4 animate-spin" />
              <Play v-else class="h-4 w-4" />
              {{ scanBusy ? t('remotePackagePatch.target.scanning') : t('remotePackagePatch.target.scan') }}
            </button>
            <div v-if="scanBusy && scanStage" class="text-center text-xs text-slate-500">
              {{ stageText(scanStage) }}
            </div>
            <div v-if="scanError" class="rounded-md bg-red-50 px-3 py-2 text-xs text-red-700">{{ scanError }}</div>

            <template v-if="inventory">
              <div class="grid grid-cols-3 gap-2">
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
                <button
                  type="button"
                  class="rpp-segment"
                  :class="targetMode === 'manual' ? 'rpp-segment-active' : ''"
                  @click="targetMode = 'manual'"
                >
                  {{ t('remotePackagePatch.target.modeManual') }}
                </button>
              </div>

              <div v-if="targetMode === 'candidate'" class="max-h-44 overflow-auto rounded-md border border-slate-200">
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

              <div v-else-if="targetMode === 'directory'" class="space-y-2">
                <select v-model="selectedDirectoryKey" class="rpp-input w-full">
                  <option value="">{{ t('remotePackagePatch.target.directoryPlaceholder') }}</option>
                  <option v-for="option in directoryOptions" :key="option.key" :value="option.key">
                    {{ option.label }}
                  </option>
                </select>
                <input v-model="internalFileName" class="rpp-input w-full" :placeholder="t('remotePackagePatch.target.fileNamePlaceholder')" />
              </div>

              <div v-else>
                <input v-model="manualInternalPath" class="rpp-input w-full font-mono" :placeholder="t('remotePackagePatch.target.manualPlaceholder')" />
              </div>

              <div class="rounded-md bg-slate-50 p-2 text-xs">
                <div class="font-medium text-slate-600">{{ t('remotePackagePatch.target.pathLabel') }}</div>
                <div class="mt-1 break-all font-mono text-slate-800">{{ targetInternalPath || '-' }}</div>
                <div class="mt-1 text-slate-500">{{ formatLayer(targetLayer) }}</div>
              </div>
              <div v-if="targetErrorText" class="rounded-md bg-red-50 px-3 py-2 text-xs text-red-700">{{ targetErrorText }}</div>

              <label class="flex items-start gap-2 rounded-md border border-slate-200 p-2 text-sm">
                <input v-model="overwrite" type="checkbox" class="mt-1" />
                <span>
                  <span class="block font-medium text-slate-800">{{ t('remotePackagePatch.target.overwriteTitle') }}</span>
                  <span class="text-xs text-slate-500">{{ t('remotePackagePatch.target.overwriteNote') }}</span>
                </span>
              </label>
              <input
                v-if="!overwrite"
                v-model="outputPath"
                class="rpp-input w-full font-mono"
                :placeholder="t('remotePackagePatch.target.outputPlaceholder')"
              />
              <label v-else class="flex items-start gap-2 rounded-md border border-amber-200 bg-amber-50 p-2 text-xs text-amber-800">
                <input v-model="overwriteConfirmed" type="checkbox" class="mt-0.5" />
                {{ t('remotePackagePatch.target.overwriteConfirm') }}
              </label>
            </template>
            <div v-else class="rounded-md bg-slate-50 p-3 text-xs text-slate-500">
              {{ t('remotePackagePatch.target.scanHint') }}
            </div>
          </div>
        </section>

        <section class="rounded-lg border border-slate-200 bg-white p-4">
          <div class="mb-3 flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <div class="flex items-center gap-2 text-sm font-semibold text-slate-800">
              <span class="rpp-step-badge" :class="stepBadgeClass(4)">4</span>
              {{ t('remotePackagePatch.steps.execute') }}
            </div>
            <button class="rpp-primary" :disabled="!canStartPatch" @click="startPatch">
              <Loader2 v-if="running" class="h-4 w-4 animate-spin" />
              <Play v-else class="h-4 w-4" />
              {{ running ? t('remotePackagePatch.execution.running') : t('remotePackagePatch.execution.start') }}
            </button>
          </div>

          <div
            v-if="errorMessage"
            class="mb-3 flex items-start gap-2 rounded-md border border-red-200 bg-red-50 p-3 text-xs text-red-800"
          >
            <ShieldAlert class="h-4 w-4 shrink-0" />
            <div class="min-w-0">
              <div class="font-semibold">{{ t('remotePackagePatch.execution.failed') }}</div>
              <div class="mt-1 break-all">{{ errorMessage }}</div>
            </div>
          </div>

          <div v-if="running && activeStage === 'upload' && uploadProgress" class="mb-3">
            <div class="mb-1 flex justify-between text-xs text-slate-500">
              <span>{{ t('remotePackagePatch.execution.uploading') }}</span>
              <span>{{ uploadPercent.toFixed(0) }}%</span>
            </div>
            <div class="h-2 overflow-hidden rounded-full bg-slate-100">
              <div class="h-full bg-sky-500" :style="{ width: `${uploadPercent}%` }"></div>
            </div>
          </div>
          <div
            v-else-if="uploadProgress && activeStage !== 'upload'"
            class="mb-3 flex items-center gap-1 text-xs text-emerald-600"
          >
            <CheckCircle2 class="h-3.5 w-3.5" />
            {{ t('remotePackagePatch.execution.uploaded') }}
          </div>

          <div class="grid grid-cols-1 gap-4 lg:grid-cols-[220px_minmax(0,1fr)]">
            <ol class="space-y-1">
              <li
                v-for="stage in stageList"
                :key="stage"
                class="rounded-md px-2 py-1 text-xs font-medium"
                :class="stageClass(stage)"
              >
                {{ stageText(stage) }}
              </li>
            </ol>
            <div ref="logContainer" class="max-h-72 overflow-auto rounded-md bg-slate-950 p-3 font-mono text-xs text-slate-100">
              <div
                v-for="(entry, index) in logs"
                :key="index"
                :class="entry.level === 'error' ? 'text-red-300' : entry.level === 'warn' ? 'text-amber-300' : 'text-slate-100'"
              >
                [{{ entry.level }}] {{ entry.message }}
              </div>
              <div v-if="logs.length === 0" class="text-slate-500">{{ t('remotePackagePatch.execution.noLogs') }}</div>
            </div>
          </div>

          <div v-if="result" class="mt-4 rounded-md border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-900">
            <div class="flex items-center gap-2 font-semibold">
              <CheckCircle2 class="h-4 w-4" />
              {{ t('remotePackagePatch.execution.done') }}
            </div>
            <div class="mt-2 space-y-1 text-xs">
              <div>
                <span class="font-medium">{{ t('remotePackagePatch.execution.outputLabel') }}: </span>
                <span class="break-all font-mono">{{ result.outputPath }}</span>
              </div>
              <div v-if="result.backupPath">
                <span class="font-medium">{{ t('remotePackagePatch.execution.backupLabel') }}: </span>
                <span class="break-all font-mono">{{ result.backupPath }}</span>
              </div>
              <div>
                <span class="font-medium">{{ t('remotePackagePatch.execution.md5Label') }}: </span>
                <span class="font-mono">{{ result.targetMd5 }}</span>
              </div>
              <div>
                <span class="font-medium">{{ t('remotePackagePatch.execution.workdirLabel') }}: </span>
                <span class="break-all font-mono">{{ result.workdir }}</span>
              </div>
              <div>
                <span class="font-medium">{{ t('remotePackagePatch.execution.manifestsLabel') }}: </span>
                <template v-if="result.updatedManifests.length > 0">
                  <div v-for="manifest in result.updatedManifests" :key="manifest" class="break-all pl-3 font-mono">
                    {{ manifest }}
                  </div>
                </template>
                <span v-else>{{ t('remotePackagePatch.execution.manifestsNone') }}</span>
              </div>
            </div>
          </div>
          <div v-if="overwrite && !overwriteConfirmed" class="mt-3 flex gap-2 rounded-md bg-amber-50 p-3 text-xs text-amber-800">
            <ShieldAlert class="h-4 w-4 shrink-0" />
            {{ t('remotePackagePatch.execution.overwriteNeedConfirm') }}
          </div>
        </section>
      </div>
    </div>
  </div>
</template>

<style scoped>
@reference "../style.css";

.rpp-input {
  @apply rounded-md border border-slate-200 px-3 py-2 text-sm outline-none transition-colors focus:border-sky-400 focus:ring-2 focus:ring-sky-100 disabled:cursor-not-allowed disabled:bg-slate-50 disabled:text-slate-400;
}

.rpp-primary {
  @apply inline-flex cursor-pointer items-center justify-center gap-2 rounded-md bg-sky-600 px-3 py-2 text-sm font-semibold text-white transition-colors hover:bg-sky-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-300 disabled:cursor-not-allowed disabled:bg-slate-300;
}

.rpp-secondary {
  @apply inline-flex cursor-pointer items-center justify-center gap-2 rounded-md border border-slate-200 bg-white px-3 py-2 text-sm font-semibold text-slate-700 transition-colors hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-300 disabled:cursor-not-allowed disabled:opacity-50;
}

.rpp-segment {
  @apply cursor-pointer rounded-md border border-slate-200 bg-white px-3 py-2 text-sm font-semibold text-slate-600 transition-colors hover:bg-slate-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-300;
}

.rpp-segment-active {
  @apply border-sky-200 bg-sky-50 text-sky-700;
}

.rpp-step-badge {
  @apply flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[11px] font-bold;
}
</style>
