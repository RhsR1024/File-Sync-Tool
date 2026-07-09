<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { ref, computed, onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { AlertCircle, Check, CheckCircle2, Eye, EyeOff, Loader, Terminal, Shield, ChevronDown, ChevronUp, Server, Globe, Network, Plus, Trash2, X as XIcon } from 'lucide-vue-next';
import { enableApplianceSsh, getConfig, saveConfig, type AppConfig, type ApplianceSshApiVersion, type ApplianceSshResult, type ApplianceSshTarget, type ApplianceSshWhitelistScope } from '../lib/tauri';
import { getApplianceSshEnableState, isValidSshPort } from '../lib/applianceSshPresentation';
import { isValidIp } from '../lib/applianceSshGroups';
import { mergeRecentItems, normalizeRecentItems, removeRecentItems } from '../lib/recentHistory';
import Empty from '../components/Empty.vue';
import IpTagInput from '../components/IpTagInput.vue';
import { pushToast } from '../composables/useToast';

defineOptions({
  name: 'EnableApplianceSshPage',
});

const { t } = useI18n();

const config = ref<AppConfig | null>(null);
const selectedIps = ref<string[]>([]);
const manualIpTags = ref<string[]>([]);
const manualIpPending = ref<string>('');
const manualIpInputRef = ref<InstanceType<typeof IpTagInput> | null>(null);
const recentIps = ref<string[]>([]);
const applianceVersion = ref<ApplianceSshApiVersion>('componentized');
const whitelistScope = ref<ApplianceSshWhitelistScope>('allTcp');
const sshUsername = ref<string>('root');
const sshPassword = ref<string>('admin_123');
const showSshPassword = ref(false);
const addWhitelistRule = ref<boolean>(true);
const isLoading = ref<boolean>(false);
const results = ref<ApplianceSshResult[]>([]);
const currentProgress = ref<{ current: number; total: number } | null>(null);
const showInfoDetail = ref<boolean>(false);
const apiTimeoutSecs = ref<number>(5);

// Jump-host targets: each row is a pair of (jumpHost, target) where the jump
// host A has the management API and the target B sits behind it.
interface JumpHostPair {
  jump: string;
  target: string;
}
const jumpHostPairs = ref<JumpHostPair[]>([]);

// Whitelist source: 'local' auto-detects the local IP; 'all' whitelists every
// IP (0.0.0.0/0) with no address input required.
const whitelistSourceMode = ref<'local' | 'all'>('local');
const WHITELIST_ALL_CIDR = '0.0.0.0/0';

// When at least one jump-host pair is configured, allow using separate SSH
// credentials for the jump host (common credentials by default).
const useSeparateJumpHostCreds = ref<boolean>(false);
const jumpHostUsername = ref<string>('');
const jumpHostPassword = ref<string>('');
const jumpHostSshPort = ref<number>(23333);
const showJumpHostPassword = ref(false);
const RECENT_IPS_KEY = 'applianceSsh.recentIps';
const RECENT_IPS_LIMIT = 10;
// Recent jump-host → target pairs, serialized as "jump=>target".
const recentJumpHostPairs = ref<string[]>([]);
const RECENT_JUMP_HOST_KEY = 'applianceSsh.recentJumpHostPairs';
const RECENT_JUMP_HOST_LIMIT = 5;
const JUMP_HOST_PAIR_SEP = '=>';

const serverOptions = computed(() => {
  if (!config.value) return [];
  return config.value.servers
    .filter(server => server.enabled)
    .map(server => ({
      id: server.id,
      host: server.host,
      name: server.name || server.host,
    }));
});

const directTargetIps = computed(() => {
  const ips = new Set<string>([...selectedIps.value, ...manualIpTags.value]);
  if (manualIpPending.value.trim()) {
    ips.add(manualIpPending.value.trim());
  }
  return Array.from(ips);
});

const isRecentIpSelected = (ip: string) => directTargetIps.value.includes(ip);

const applyRecentIp = (ip: string) => {
  if (isLoading.value || isRecentIpSelected(ip)) {
    return;
  }
  manualIpInputRef.value?.applyTag(ip);
};

const validJumpHostPairs = computed(() =>
  jumpHostPairs.value
    .map(p => ({ jump: p.jump.trim(), target: p.target.trim() }))
    .filter(p => p.jump && p.target)
);

const allTargetsSummary = computed(() => {
  const direct = directTargetIps.value.map(ip => ({ ip, jump: null as string | null }));
  const viaJump = validJumpHostPairs.value.map(p => ({ ip: p.target, jump: p.jump }));
  return [...direct, ...viaJump];
});

const hasAnyJumpHost = computed(() => validJumpHostPairs.value.length > 0);

const jumpHostSshPortInvalid = computed(
  () => hasAnyJumpHost.value && !isValidSshPort(jumpHostSshPort.value),
);

const needsSshCredentials = computed(() => addWhitelistRule.value);

const hasWhitelistConfigError = computed(() => {
  if (!needsSshCredentials.value) return false;
  if (!sshUsername.value.trim() || !sshPassword.value) return true;
  if (hasAnyJumpHost.value && useSeparateJumpHostCreds.value) {
    if (!jumpHostUsername.value.trim() || !jumpHostPassword.value) return true;
  }
  return false;
});

const isFormValid = computed(() => {
  if (isLoading.value || allTargetsSummary.value.length === 0) {
    return false;
  }
  if (needsSshCredentials.value && hasWhitelistConfigError.value) {
    return false;
  }
  return true;
});

const addJumpHostPair = () => {
  jumpHostPairs.value.push({ jump: '', target: '' });
};
const removeJumpHostPair = (index: number) => {
  jumpHostPairs.value.splice(index, 1);
};

const recentJumpHostPairsParsed = computed(() =>
  recentJumpHostPairs.value
    .map(raw => {
      const [jump, target] = raw.split(JUMP_HOST_PAIR_SEP);
      return { jump: (jump ?? '').trim(), target: (target ?? '').trim(), key: raw };
    })
    .filter(p => p.jump && p.target)
);

const isRecentJumpHostSelected = (pair: { jump: string; target: string }) =>
  validJumpHostPairs.value.some(p => p.jump === pair.jump && p.target === pair.target);

const applyRecentJumpHostPair = (pair: { jump: string; target: string }) => {
  if (isLoading.value || isRecentJumpHostSelected(pair)) {
    return;
  }
  // Reuse an empty row if the user just added one, otherwise append.
  const emptyRow = jumpHostPairs.value.find(p => !p.jump.trim() && !p.target.trim());
  if (emptyRow) {
    emptyRow.jump = pair.jump;
    emptyRow.target = pair.target;
  } else {
    jumpHostPairs.value.push({ jump: pair.jump, target: pair.target });
  }
};

const storeRecentIps = async (items: readonly string[]) => {
  const normalized = normalizeRecentItems(items, RECENT_IPS_LIMIT);
  recentIps.value = normalized;
  try {
    await invoke('save_kv', {
      key: RECENT_IPS_KEY,
      value: normalized,
    });
  } catch {
    // Recent history is best-effort only.
  }
};

const rememberRecentIps = async (items: readonly string[]) => {
  if (items.length === 0) {
    return;
  }
  await storeRecentIps(mergeRecentItems(recentIps.value, items, RECENT_IPS_LIMIT));
};

const removeRecentIp = async (ip: string) => {
  await storeRecentIps(removeRecentItems(recentIps.value, ip, RECENT_IPS_LIMIT));
};

const clearRecentIps = async () => {
  await storeRecentIps([]);
};

const storeRecentJumpHostPairs = async (items: readonly string[]) => {
  const normalized = normalizeRecentItems(items, RECENT_JUMP_HOST_LIMIT);
  recentJumpHostPairs.value = normalized;
  try {
    await invoke('save_kv', {
      key: RECENT_JUMP_HOST_KEY,
      value: normalized,
    });
  } catch {
    // Recent history is best-effort only.
  }
};

const rememberRecentJumpHostPairs = async (pairs: readonly { jump: string; target: string }[]) => {
  const items = pairs.map(p => `${p.jump}${JUMP_HOST_PAIR_SEP}${p.target}`);
  if (items.length === 0) {
    return;
  }
  await storeRecentJumpHostPairs(mergeRecentItems(recentJumpHostPairs.value, items, RECENT_JUMP_HOST_LIMIT));
};

const removeRecentJumpHostPair = async (key: string) => {
  await storeRecentJumpHostPairs(removeRecentItems(recentJumpHostPairs.value, key, RECENT_JUMP_HOST_LIMIT));
};

const clearRecentJumpHostPairs = async () => {
  await storeRecentJumpHostPairs([]);
};

onMounted(async () => {
  try {
    config.value = await getConfig();
    apiTimeoutSecs.value = config.value.appliance_ssh_api_timeout_secs ?? 5;
  } catch (e) {
    console.error('Failed to load config:', e);
  }

  try {
    const saved = await invoke<string[] | null>('load_kv', { key: RECENT_IPS_KEY });
    recentIps.value = normalizeRecentItems(saved, RECENT_IPS_LIMIT);
  } catch {
    // Ignore malformed recent history from older builds.
  }

  try {
    const savedPairs = await invoke<string[] | null>('load_kv', { key: RECENT_JUMP_HOST_KEY });
    recentJumpHostPairs.value = normalizeRecentItems(savedPairs, RECENT_JUMP_HOST_LIMIT);
  } catch {
    // Ignore malformed recent history from older builds.
  }
});

const saveApiTimeout = async () => {
  if (!config.value) return;
  config.value.appliance_ssh_api_timeout_secs = apiTimeoutSecs.value;
  try {
    await saveConfig(config.value);
  } catch (e) {
    console.error('Failed to save api timeout:', e);
  }
};

const handleExecute = async () => {
  if (allTargetsSummary.value.length === 0) {
    pushToast(t('tools.applianceSsh.noIps'), 'warning');
    return;
  }

  if (hasWhitelistConfigError.value) {
    pushToast(t('tools.applianceSsh.sshCredentialsRequired'), 'warning');
    return;
  }

  if (jumpHostSshPortInvalid.value) {
    pushToast(t('tools.applianceSsh.jumpHostSshPortInvalid'), 'warning');
    return;
  }

  isLoading.value = true;
  results.value = [];

  const targets: ApplianceSshTarget[] = [
    ...directTargetIps.value.map(ip => ({ ip })),
    ...validJumpHostPairs.value.map(p => ({ ip: p.target, jumpHost: p.jump })),
  ];
  const recentValidIps = targets.map(target => target.ip).filter(isValidIp);

  try {
    currentProgress.value = { current: 0, total: targets.length };
    await rememberRecentIps(recentValidIps);
    await rememberRecentJumpHostPairs(validJumpHostPairs.value);

    const response = await enableApplianceSsh({
      targets,
      applianceVersion: applianceVersion.value,
      whitelistScope: whitelistScope.value,
      sshUsername: sshUsername.value.trim(),
      sshPassword: sshPassword.value,
      addWhitelistRule: addWhitelistRule.value,
      whitelistCidr: whitelistSourceMode.value === 'all'
        ? WHITELIST_ALL_CIDR
        : undefined,
      jumpHostUseSeparateCreds: hasAnyJumpHost.value && useSeparateJumpHostCreds.value,
      jumpHostUsername: useSeparateJumpHostCreds.value ? jumpHostUsername.value.trim() : undefined,
      jumpHostPassword: useSeparateJumpHostCreds.value ? jumpHostPassword.value : undefined,
      jumpHostSshPort: hasAnyJumpHost.value ? jumpHostSshPort.value : undefined,
    });
    results.value = response;
    pushToast(t('tools.applianceSsh.completed', { success: response.filter(item => item.success).length, total: response.length }), 'success', { ttlMs: 2600 });
    currentProgress.value = { current: targets.length, total: targets.length };
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    results.value = targets.map(target => ({
      ip: target.ip,
      success: false,
      message: `Error: ${errorMessage}`,
      jumpHost: target.jumpHost,
    }));
    pushToast(errorMessage, 'error', { ttlMs: 4200 });
  } finally {
    isLoading.value = false;
    currentProgress.value = null;
  }
};

const toggleServerIp = (ip: string) => {
  const idx = selectedIps.value.indexOf(ip);
  if (idx > -1) {
    selectedIps.value.splice(idx, 1);
  } else {
    selectedIps.value.push(ip);
  }
};

const isServerSelected = (ip: string) => selectedIps.value.includes(ip);

const successCount = computed(() => results.value.filter(r => r.success).length);
const failureCount = computed(() => results.value.filter(r => !r.success).length);

const formatEnableState = (value?: number) => {
  const state = getApplianceSshEnableState(value);
  if (state === 'enabled') {
    return t('tools.applianceSsh.stateEnabled');
  }
  if (state === 'disabled') {
    return t('tools.applianceSsh.stateDisabled');
  }
  return t('tools.applianceSsh.stateUnknown');
};

const enableStateClass = (value?: number) => {
  const state = getApplianceSshEnableState(value);
  if (state === 'enabled') return 'bg-emerald-50 text-emerald-700 border-emerald-200';
  if (state === 'disabled') return 'bg-amber-50 text-amber-700 border-amber-200';
  return 'bg-slate-100 text-slate-600 border-slate-200';
};
</script>

<template>
  <div class="flex-1 flex flex-col bg-gradient-to-br from-slate-50 to-slate-100 overflow-y-auto">
    <div class="max-w-6xl w-full mx-auto p-6 pb-10 space-y-5">
      <!-- Header -->
      <div class="flex items-start justify-between gap-4">
        <div class="flex items-center gap-3">
          <div class="w-10 h-10 rounded-xl bg-gradient-to-br from-blue-500 to-indigo-600 flex items-center justify-center shadow-sm">
            <Terminal class="w-5 h-5 text-white" />
          </div>
          <div>
            <h1 class="text-2xl font-bold text-slate-900">{{ t('tools.applianceSsh.title') }}</h1>
            <p class="text-slate-500 text-sm mt-0.5">{{ t('tools.applianceSsh.description') }}</p>
          </div>
        </div>
      </div>

      <!-- Collapsible info banner -->
      <div class="bg-blue-50/70 border border-blue-200/60 rounded-xl overflow-hidden">
        <button
          @click="showInfoDetail = !showInfoDetail"
          class="w-full flex items-center justify-between px-4 py-2.5 text-left hover:bg-blue-50 transition-colors"
        >
          <span class="text-blue-800 text-sm font-medium">{{ t('tools.applianceSsh.info') }}</span>
          <component :is="showInfoDetail ? ChevronUp : ChevronDown" class="w-4 h-4 text-blue-500" />
        </button>
        <div v-show="showInfoDetail" class="px-4 pb-3 -mt-1">
          <p class="text-blue-700 text-xs leading-relaxed">{{ t('tools.applianceSsh.infoDetail') }}</p>
        </div>
      </div>

      <div class="grid grid-cols-1 lg:grid-cols-3 gap-5">
        <!-- Left column: Form -->
        <div class="lg:col-span-2 space-y-4">
          <!-- Server selection + Manual IP in one card -->
          <div class="bg-white border border-slate-200/80 rounded-xl p-5 shadow-sm space-y-5">
            <div>
              <div class="flex items-center gap-2 mb-3">
                <Server class="w-4 h-4 text-slate-400" />
                <h3 class="text-sm font-semibold text-slate-800">{{ t('tools.applianceSsh.selectServer') }}</h3>
                <span class="text-xs text-slate-400">({{ t('tools.applianceSsh.optional') }})</span>
              </div>

              <div v-if="serverOptions.length > 0" class="grid grid-cols-1 sm:grid-cols-2 gap-1.5">
                <label
                  v-for="server in serverOptions"
                  :key="server.id"
                  :for="`appliance-ssh-server-${server.id}`"
                  class="flex items-center gap-2.5 px-3 py-2 rounded-lg cursor-pointer transition-colors"
                  :class="isServerSelected(server.host) ? 'bg-blue-50 border border-blue-200' : 'hover:bg-slate-50 border border-transparent'"
                >
                  <input
                    type="checkbox"
                    :id="`appliance-ssh-server-${server.id}`"
                    :checked="isServerSelected(server.host)"
                    @change="toggleServerIp(server.host)"
                    :disabled="isLoading"
                    class="rounded border-slate-300 text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
                  />
                  <span class="text-sm text-slate-700">
                    <span class="font-medium">{{ server.name }}</span>
                    <span class="text-slate-400 ml-1">({{ server.host }})</span>
                  </span>
                </label>
              </div>
              <div v-else class="text-sm text-slate-400">{{ t('tools.applianceSsh.noServers') }}</div>
            </div>

            <div class="border-t border-slate-100 pt-4">
              <div class="flex items-center gap-2 mb-2">
                <Globe class="w-4 h-4 text-slate-400" />
                <label class="text-sm font-semibold text-slate-800">{{ t('tools.applianceSsh.manualIp') }}</label>
              </div>
              <!-- Tag Input -->
              <IpTagInput
                ref="manualIpInputRef"
                v-model="manualIpTags"
                :disabled="isLoading"
                :placeholder="t('tools.applianceSsh.manualIpPlaceholder')"
                :aria-label="t('tools.applianceSsh.manualIp')"
                datalist-id="appliance-ssh-recent-ips"
                @update:pending="manualIpPending = $event"
              />
              <datalist id="appliance-ssh-recent-ips">
                <option v-for="ip in recentIps" :key="`appliance-ssh-recent-${ip}`" :value="ip" />
              </datalist>
              <p class="text-xs text-slate-400 mt-1.5">{{ t('tools.applianceSsh.manualIpHint') }}</p>
              <div class="mt-3 flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                <div>
                  <label for="appliance-ssh-version" class="text-xs font-medium text-slate-600">{{ t('tools.applianceSsh.applianceVersion') }}</label>
                  <p class="text-xs text-slate-400 mt-0.5">{{ t('tools.applianceSsh.applianceVersionHint') }}</p>
                </div>
                <select
                  id="appliance-ssh-version"
                  v-model="applianceVersion"
                  :disabled="isLoading"
                  class="w-full sm:w-48 px-3 py-2 text-sm border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed bg-white text-slate-700 transition-colors"
                >
                  <option value="componentized">{{ t('tools.applianceSsh.applianceVersionComponentized') }}</option>
                  <option value="mainline">{{ t('tools.applianceSsh.applianceVersionMainline') }}</option>
                </select>
              </div>
              <div v-if="recentIps.length > 0" class="mt-3 space-y-2">
                <div class="flex items-center justify-between gap-2">
                  <span class="text-xs font-medium text-slate-500">{{ t('tools.applianceSsh.recentIps') }}</span>
                  <button
                    type="button"
                    :disabled="isLoading"
                    class="text-xs font-medium text-slate-500 hover:text-slate-700 disabled:cursor-not-allowed disabled:opacity-50"
                    @click="clearRecentIps"
                  >
                    {{ t('tools.applianceSsh.clearRecentIps') }}
                  </button>
                </div>
                <div class="flex items-center gap-2 flex-wrap">
                  <span
                    v-for="ip in recentIps"
                    :key="`appliance-ssh-history-${ip}`"
                    class="inline-flex items-stretch overflow-hidden rounded-full border transition-colors"
                    :class="isRecentIpSelected(ip)
                      ? 'border-blue-600 bg-blue-600 text-white'
                      : 'border-slate-300 bg-white text-slate-600 hover:border-slate-400 hover:bg-slate-50'"
                  >
                    <button
                      type="button"
                      :disabled="isLoading"
                      class="inline-flex items-center gap-1 px-2.5 py-1 text-xs font-medium disabled:cursor-not-allowed"
                      @click="applyRecentIp(ip)"
                    >
                      <Check v-if="isRecentIpSelected(ip)" class="h-3 w-3" />
                      <span class="font-mono">{{ ip }}</span>
                    </button>
                    <button
                      type="button"
                      :disabled="isLoading"
                      class="inline-flex items-center border-l border-current/10 px-2 text-current/70 transition hover:text-current disabled:cursor-not-allowed"
                      :title="t('tools.applianceSsh.removeRecentIp')"
                      @click.stop="removeRecentIp(ip)"
                    >
                      <Trash2 class="h-3.5 w-3.5" />
                    </button>
                  </span>
                </div>
              </div>
            </div>
          </div>

          <!-- Jump host targets card (collapsed by default) -->
          <div class="bg-white border border-slate-200/80 rounded-xl shadow-sm overflow-hidden">
            <div class="flex items-center justify-between gap-3 px-5 py-4">
              <div class="flex items-center gap-2">
                <Network class="w-4 h-4 text-slate-400" />
                <h3 class="text-sm font-semibold text-slate-800">{{ t('tools.applianceSsh.jumpHostSection') }}</h3>
                <span class="text-xs text-slate-400">({{ t('tools.applianceSsh.optional') }})</span>
              </div>
              <button
                type="button"
                @click="addJumpHostPair"
                :disabled="isLoading"
                class="inline-flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium text-blue-600 bg-blue-50 border border-blue-200 hover:bg-blue-100 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <Plus class="w-3.5 h-3.5" />
                {{ t('tools.applianceSsh.jumpHostAdd') }}
              </button>
            </div>
            <div v-if="jumpHostPairs.length === 0" class="px-5 pb-4 pt-0">
              <p class="text-xs text-slate-400 leading-relaxed">{{ t('tools.applianceSsh.jumpHostEmptyHint') }}</p>
            </div>
            <div v-else class="px-5 pb-4 space-y-2">
              <div
                v-for="(pair, idx) in jumpHostPairs"
                :key="idx"
                class="flex items-center gap-2"
              >
                <input
                  v-model="pair.jump"
                  type="text"
                  :placeholder="t('tools.applianceSsh.jumpHostIpPlaceholder')"
                  :disabled="isLoading"
                  class="flex-1 min-w-0 px-3 py-2 text-sm font-mono border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
                  :class="pair.jump.trim() && !isValidIp(pair.jump.trim()) ? 'border-red-300 focus:border-red-400 focus:ring-red-400/20' : ''"
                />
                <span class="text-slate-400 text-sm shrink-0">→</span>
                <input
                  v-model="pair.target"
                  type="text"
                  :placeholder="t('tools.applianceSsh.jumpHostTargetPlaceholder')"
                  :disabled="isLoading"
                  class="flex-1 min-w-0 px-3 py-2 text-sm font-mono border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
                  :class="pair.target.trim() && !isValidIp(pair.target.trim()) ? 'border-red-300 focus:border-red-400 focus:ring-red-400/20' : ''"
                />
                <button
                  type="button"
                  @click="removeJumpHostPair(idx)"
                  :disabled="isLoading"
                  class="shrink-0 p-2 rounded-lg text-slate-400 hover:text-red-500 hover:bg-red-50 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                  :title="t('tools.applianceSsh.jumpHostRemove')"
                >
                  <XIcon class="w-4 h-4" />
                </button>
              </div>
              <p class="text-xs text-slate-400 mt-2">{{ t('tools.applianceSsh.jumpHostRowHint') }}</p>
            </div>

            <!-- Jump host SSH port (shown whenever a jump-host pair exists) -->
            <div v-if="hasAnyJumpHost" class="px-5 pb-4 pt-0">
              <label class="block text-xs font-medium text-slate-600 mb-1.5">{{ t('tools.applianceSsh.jumpHostSshPort') }}</label>
              <input
                v-model.number="jumpHostSshPort"
                type="number"
                min="1"
                max="65535"
                :disabled="isLoading"
                class="w-32 px-3 py-2 text-sm font-mono border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
                :class="jumpHostSshPortInvalid ? 'border-red-300 focus:border-red-400 focus:ring-red-400/20' : ''"
              />
              <p class="text-xs text-slate-400 mt-1.5">{{ t('tools.applianceSsh.jumpHostSshPortHint') }}</p>
            </div>

            <!-- Recent jump-host → target pairs (max 5) -->
            <div v-if="recentJumpHostPairsParsed.length > 0" class="px-5 pb-4 pt-0 space-y-2">
              <div class="flex items-center justify-between gap-2">
                <span class="text-xs font-medium text-slate-500">{{ t('tools.applianceSsh.jumpHostRecent') }}</span>
                <button
                  type="button"
                  :disabled="isLoading"
                  class="text-xs font-medium text-slate-500 hover:text-slate-700 disabled:cursor-not-allowed disabled:opacity-50"
                  @click="clearRecentJumpHostPairs"
                >
                  {{ t('tools.applianceSsh.clearRecentJumpHost') }}
                </button>
              </div>
              <div class="flex items-center gap-2 flex-wrap">
                <span
                  v-for="pair in recentJumpHostPairsParsed"
                  :key="`appliance-ssh-jh-history-${pair.key}`"
                  class="inline-flex items-stretch overflow-hidden rounded-full border transition-colors"
                  :class="isRecentJumpHostSelected(pair)
                    ? 'border-blue-600 bg-blue-600 text-white'
                    : 'border-slate-300 bg-white text-slate-600 hover:border-slate-400 hover:bg-slate-50'"
                >
                  <button
                    type="button"
                    :disabled="isLoading"
                    class="inline-flex items-center gap-1 px-2.5 py-1 text-xs font-medium font-mono disabled:cursor-not-allowed"
                    @click="applyRecentJumpHostPair(pair)"
                  >
                    <Check v-if="isRecentJumpHostSelected(pair)" class="h-3 w-3" />
                    <span>{{ pair.jump }}</span>
                    <span class="opacity-60">→</span>
                    <span>{{ pair.target }}</span>
                  </button>
                  <button
                    type="button"
                    :disabled="isLoading"
                    class="inline-flex items-center border-l border-current/10 px-2 text-current/70 transition hover:text-current disabled:cursor-not-allowed"
                    :title="t('tools.applianceSsh.removeRecentJumpHost')"
                    @click.stop="removeRecentJumpHostPair(pair.key)"
                  >
                    <Trash2 class="h-3.5 w-3.5" />
                  </button>
                </span>
              </div>
            </div>
          </div>

          <!-- Whitelist rule card -->
          <div class="bg-white border border-slate-200/80 rounded-xl p-5 shadow-sm">
            <div class="flex items-start justify-between gap-4">
              <div class="flex items-start gap-2">
                <Shield class="w-4 h-4 text-slate-400 mt-0.5" />
                <div>
                  <h3 class="text-sm font-semibold text-slate-800">{{ t('tools.applianceSsh.whitelistTitle') }}</h3>
                  <p class="text-xs text-slate-400 mt-0.5 leading-relaxed">{{ t('tools.applianceSsh.whitelistDescription') }}</p>
                </div>
              </div>
              <label class="inline-flex items-center gap-2 text-xs font-medium text-slate-600 cursor-pointer shrink-0 mt-0.5">
                <input
                  v-model="addWhitelistRule"
                  type="checkbox"
                  :disabled="isLoading"
                  class="rounded border-slate-300 text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
                />
                <span>{{ t('tools.applianceSsh.addWhitelistRule') }}</span>
              </label>
            </div>

            <!-- SSH credentials + source options - conditionally shown -->
            <Transition
              enter-active-class="transition-all duration-200 ease-out"
              enter-from-class="opacity-0 -translate-y-2 max-h-0"
              enter-to-class="opacity-100 translate-y-0 max-h-[48rem]"
              leave-active-class="transition-all duration-150 ease-in"
              leave-from-class="opacity-100 translate-y-0 max-h-[48rem]"
              leave-to-class="opacity-0 -translate-y-2 max-h-0"
            >
              <div v-if="addWhitelistRule" class="overflow-hidden">
                <div class="grid grid-cols-1 md:grid-cols-2 gap-3 mt-4 pt-4 border-t border-slate-100">
                  <div>
                    <label class="block text-xs font-medium text-slate-600 mb-1.5">{{ t('tools.applianceSsh.sshUsername') }}</label>
                    <input
                      v-model="sshUsername"
                      type="text"
                      :placeholder="t('tools.applianceSsh.sshUsernamePlaceholder')"
                      :disabled="isLoading"
                      class="w-full px-3 py-2 text-sm border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
                    />
                  </div>

                  <div>
                    <label class="block text-xs font-medium text-slate-600 mb-1.5">{{ t('tools.applianceSsh.sshPassword') }}</label>
                    <input
                      v-model="sshPassword"
                      :type="showSshPassword ? 'text' : 'password'"
                      autocomplete="new-password"
                      :placeholder="t('tools.applianceSsh.sshPasswordPlaceholder')"
                      :disabled="isLoading"
                      class="w-full px-3 py-2 text-sm border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
                    />
                    <button
                      type="button"
                      class="mt-2 inline-flex items-center gap-1 text-xs font-medium text-slate-500 transition-colors hover:text-slate-700"
                      @click="showSshPassword = !showSshPassword"
                    >
                      <component :is="showSshPassword ? EyeOff : Eye" class="h-3.5 w-3.5" />
                      {{ t(showSshPassword ? 'tools.applianceSsh.hidePassword' : 'tools.applianceSsh.showPassword') }}
                    </button>
                  </div>
                </div>

                <!-- Whitelist scope -->
                <div class="mt-4 pt-4 border-t border-slate-100">
                  <div class="text-xs font-medium text-slate-600 mb-2">{{ t('tools.applianceSsh.whitelistScopeLabel') }}</div>
                  <div class="flex flex-wrap items-center gap-x-5 gap-y-2">
                    <label class="inline-flex items-center gap-2 text-xs text-slate-700 cursor-pointer">
                      <input
                        v-model="whitelistScope"
                        type="radio"
                        value="allTcp"
                        :disabled="isLoading"
                        class="text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
                      />
                      <span>{{ t('tools.applianceSsh.whitelistScopeAllTcp') }}</span>
                    </label>
                    <label class="inline-flex items-center gap-2 text-xs text-slate-700 cursor-pointer">
                      <input
                        v-model="whitelistScope"
                        type="radio"
                        value="sshOnly"
                        :disabled="isLoading"
                        class="text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
                      />
                      <span>{{ t('tools.applianceSsh.whitelistScopeSshOnly') }}</span>
                    </label>
                  </div>
                  <p class="text-xs text-slate-400 mt-1.5">{{ t('tools.applianceSsh.whitelistScopeHint') }}</p>
                </div>

                <!-- Whitelist source (local IP auto / allow all) -->
                <div class="mt-4 pt-4 border-t border-slate-100">
                  <div class="text-xs font-medium text-slate-600 mb-2">{{ t('tools.applianceSsh.whitelistSourceLabel') }}</div>
                  <div class="flex flex-wrap items-center gap-x-5 gap-y-2">
                    <label class="inline-flex items-center gap-2 text-xs text-slate-700 cursor-pointer">
                      <input
                        v-model="whitelistSourceMode"
                        type="radio"
                        value="local"
                        :disabled="isLoading"
                        class="text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
                      />
                      <span>{{ t('tools.applianceSsh.whitelistSourceLocal') }}</span>
                    </label>
                    <label class="inline-flex items-center gap-2 text-xs text-slate-700 cursor-pointer">
                      <input
                        v-model="whitelistSourceMode"
                        type="radio"
                        value="all"
                        :disabled="isLoading"
                        class="text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
                      />
                      <span>{{ t('tools.applianceSsh.whitelistSourceAll') }}</span>
                    </label>
                  </div>
                  <p v-if="whitelistSourceMode === 'all'" class="text-xs text-amber-600 mt-1.5">{{ t('tools.applianceSsh.whitelistSourceAllHint') }}</p>
                </div>

                <!-- Jump-host credentials (only when any jump-host pair is configured) -->
                <Transition
                  enter-active-class="transition-all duration-200 ease-out"
                  enter-from-class="opacity-0 -translate-y-1 max-h-0"
                  enter-to-class="opacity-100 translate-y-0 max-h-80"
                  leave-active-class="transition-all duration-150 ease-in"
                  leave-from-class="opacity-100 translate-y-0 max-h-80"
                  leave-to-class="opacity-0 -translate-y-1 max-h-0"
                >
                  <div v-if="hasAnyJumpHost" class="overflow-hidden">
                    <div class="mt-4 pt-4 border-t border-slate-100">
                      <label class="inline-flex items-center gap-2 text-xs font-medium text-slate-600 cursor-pointer">
                        <input
                          v-model="useSeparateJumpHostCreds"
                          type="checkbox"
                          :disabled="isLoading"
                          class="rounded border-slate-300 text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
                        />
                        <span>{{ t('tools.applianceSsh.jumpHostSeparateCreds') }}</span>
                      </label>
                      <Transition
                        enter-active-class="transition-all duration-200 ease-out"
                        enter-from-class="opacity-0 -translate-y-1 max-h-0"
                        enter-to-class="opacity-100 translate-y-0 max-h-60"
                        leave-active-class="transition-all duration-150 ease-in"
                        leave-from-class="opacity-100 translate-y-0 max-h-60"
                        leave-to-class="opacity-0 -translate-y-1 max-h-0"
                      >
                        <div v-if="useSeparateJumpHostCreds" class="overflow-hidden">
                          <div class="grid grid-cols-1 md:grid-cols-2 gap-3 mt-3">
                            <div>
                              <label class="block text-xs font-medium text-slate-600 mb-1.5">{{ t('tools.applianceSsh.jumpHostUsername') }}</label>
                              <input
                                v-model="jumpHostUsername"
                                type="text"
                                :placeholder="t('tools.applianceSsh.sshUsernamePlaceholder')"
                                :disabled="isLoading"
                                class="w-full px-3 py-2 text-sm border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
                              />
                            </div>
                            <div>
                              <label class="block text-xs font-medium text-slate-600 mb-1.5">{{ t('tools.applianceSsh.jumpHostPassword') }}</label>
                              <input
                                v-model="jumpHostPassword"
                                :type="showJumpHostPassword ? 'text' : 'password'"
                                autocomplete="new-password"
                                :placeholder="t('tools.applianceSsh.sshPasswordPlaceholder')"
                                :disabled="isLoading"
                                class="w-full px-3 py-2 text-sm border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
                              />
                              <button
                                type="button"
                                class="mt-2 inline-flex items-center gap-1 text-xs font-medium text-slate-500 transition-colors hover:text-slate-700"
                                @click="showJumpHostPassword = !showJumpHostPassword"
                              >
                                <component :is="showJumpHostPassword ? EyeOff : Eye" class="h-3.5 w-3.5" />
                                {{ t(showJumpHostPassword ? 'tools.applianceSsh.hidePassword' : 'tools.applianceSsh.showPassword') }}
                              </button>
                            </div>
                          </div>
                        </div>
                      </Transition>
                    </div>
                  </div>
                </Transition>

                <p class="mt-3 text-xs text-slate-400 leading-relaxed">{{ t('tools.applianceSsh.whitelistHint') }}</p>
                <p v-if="hasWhitelistConfigError" class="mt-1.5 text-xs text-red-500 font-medium">{{ t('tools.applianceSsh.sshCredentialsRequired') }}</p>
              </div>
            </Transition>
          </div>

          <!-- Selected IPs summary -->
          <div v-if="allTargetsSummary.length > 0" class="bg-slate-50 border border-slate-200/80 rounded-xl px-4 py-3">
            <p class="text-xs font-medium text-slate-500 mb-2">{{ t('tools.applianceSsh.selectedIps', { count: allTargetsSummary.length }) }}</p>
            <div class="flex flex-wrap gap-1.5">
              <span
                v-for="item in allTargetsSummary"
                :key="`${item.jump ?? ''}->${item.ip}`"
                class="inline-flex items-center bg-blue-100/80 text-blue-800 px-2.5 py-0.5 rounded-md text-xs font-mono"
              >
                <template v-if="item.jump">
                  <span class="text-blue-500">{{ item.jump }}</span>
                  <span class="text-blue-400 mx-1">→</span>
                </template>
                {{ item.ip }}
              </span>
            </div>
          </div>

          <!-- API Timeout -->
          <div class="flex items-center justify-between gap-3 px-4 py-3 bg-slate-50 border border-slate-200/80 rounded-xl">
            <div>
              <span class="text-xs font-medium text-slate-600">{{ t('tools.applianceSsh.apiTimeout') }}</span>
              <p class="text-xs text-slate-400 mt-0.5">{{ t('tools.applianceSsh.apiTimeoutDesc') }}</p>
            </div>
            <select v-model.number="apiTimeoutSecs" @change="saveApiTimeout"
              class="shrink-0 p-1.5 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none bg-white text-xs text-slate-700">
              <option :value="1">1 {{ t('settings.seconds') }}</option>
              <option :value="3">3 {{ t('settings.seconds') }}</option>
              <option :value="5">5 {{ t('settings.seconds') }}</option>
              <option :value="10">10 {{ t('settings.seconds') }}</option>
              <option :value="30">30 {{ t('settings.seconds') }}</option>
            </select>
          </div>

          <!-- Execute button -->
          <button
            @click="handleExecute"
            :disabled="!isFormValid"
            class="w-full px-5 py-3 bg-gradient-to-r from-blue-600 to-blue-700 text-white font-semibold rounded-xl hover:from-blue-700 hover:to-blue-800 focus:outline-none focus:ring-2 focus:ring-blue-500/40 focus:ring-offset-1 disabled:opacity-40 disabled:cursor-not-allowed transition-all duration-200 flex items-center justify-center gap-2 text-base shadow-sm"
          >
            <Loader v-if="isLoading" class="w-4.5 h-4.5 animate-spin" />
            <span>{{ isLoading ? t('tools.applianceSsh.processing') : t('tools.applianceSsh.executeButton') }}</span>
          </button>
        </div>

        <!-- Right column: Results panel -->
        <div class="bg-white border border-slate-200/80 rounded-xl p-5 shadow-sm h-fit sticky top-6">
          <h3 class="text-sm font-semibold text-slate-800 mb-4">{{ t('tools.applianceSsh.results') }}</h3>

          <div class="grid grid-cols-3 gap-3 text-center">
            <div class="rounded-lg bg-slate-50 p-2.5">
              <div class="text-2xl font-bold text-slate-800 tabular-nums">{{ results.length }}</div>
              <div class="text-[10px] text-slate-400 mt-0.5 uppercase tracking-wide">{{ t('tools.applianceSsh.totalLabel') }}</div>
            </div>
            <div class="rounded-lg bg-emerald-50 p-2.5">
              <div class="text-2xl font-bold text-emerald-600 tabular-nums">{{ successCount }}</div>
              <div class="text-[10px] text-emerald-500 mt-0.5 uppercase tracking-wide">{{ t('tools.applianceSsh.successLabel') }}</div>
            </div>
            <div class="rounded-lg bg-red-50 p-2.5">
              <div class="text-2xl font-bold text-red-500 tabular-nums">{{ failureCount }}</div>
              <div class="text-[10px] text-red-400 mt-0.5 uppercase tracking-wide">{{ t('tools.applianceSsh.failedLabel') }}</div>
            </div>
          </div>

          <div v-if="currentProgress" class="mt-4 pt-3 border-t border-slate-100">
            <div class="flex items-center justify-between text-xs text-slate-500 mb-1.5">
              <span>{{ t('tools.applianceSsh.progress', { current: currentProgress.current, total: currentProgress.total }) }}</span>
              <span class="tabular-nums">{{ Math.round((currentProgress.current / currentProgress.total) * 100) }}%</span>
            </div>
            <div class="w-full bg-slate-100 rounded-full h-1.5">
              <div
                class="bg-gradient-to-r from-blue-500 to-blue-600 h-1.5 rounded-full transition-all duration-300"
                :style="{ width: `${(currentProgress.current / currentProgress.total) * 100}%` }"
              ></div>
            </div>
          </div>

          <!-- Empty state hint -->
          <Empty
            v-if="results.length === 0 && !currentProgress"
            class="mt-4 pt-3 border-t border-slate-100"
            :title="t('tools.applianceSsh.emptyTitle')"
            :description="t('tools.applianceSsh.emptyDescription')"
            dashed
          />
        </div>
      </div>

      <!-- Results table -->
      <div v-if="results.length > 0">
        <div class="bg-white border border-slate-200/80 rounded-xl overflow-hidden shadow-sm">
          <div class="overflow-x-auto">
            <table class="w-full">
              <thead>
                <tr class="border-b border-slate-100 bg-slate-50/80">
                  <th scope="col" class="px-5 py-2.5 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide w-44">IP {{ t('tools.applianceSsh.address') }}</th>
                  <th scope="col" class="px-5 py-2.5 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide w-24">{{ t('tools.applianceSsh.status') }}</th>
                  <th scope="col" class="px-5 py-2.5 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">{{ t('tools.applianceSsh.message') }}</th>
                </tr>
              </thead>
              <tbody class="divide-y divide-slate-100">
                <tr v-for="result in results" :key="`${result.jumpHost ?? ''}->${result.ip}`" class="hover:bg-slate-50/60 transition-colors">
                  <td class="px-5 py-3 text-sm font-mono text-slate-800">
                    <div class="flex flex-col gap-0.5">
                      <span>{{ result.ip }}</span>
                      <span v-if="result.jumpHost" class="text-xs text-slate-500 font-normal">
                        {{ t('tools.applianceSsh.viaJumpHost') }}
                        <span class="font-mono">{{ result.jumpHost }}</span>
                      </span>
                    </div>
                  </td>
                  <td class="px-5 py-3">
                    <div class="flex items-center gap-1.5">
                      <component
                        :is="result.success ? CheckCircle2 : AlertCircle"
                        :class="result.success ? 'text-emerald-500' : 'text-red-500'"
                        class="w-4 h-4"
                      />
                      <span :class="result.success ? 'text-emerald-600' : 'text-red-600'" class="text-sm font-medium">
                        {{ result.success ? t('tools.applianceSsh.success') : t('tools.applianceSsh.failed') }}
                      </span>
                    </div>
                  </td>
                  <td class="px-5 py-3 text-sm text-slate-600">
                    <div class="space-y-2">
                      <p class="leading-relaxed">{{ result.message }}</p>

                      <!-- For jump-host runs the SSH enable state/port belong to the jump
                           host (where the management API lives), while the whitelist is
                           applied on the target via SSH. Group them so the two machines'
                           statuses aren't mistaken for one. -->
                      <template v-if="result.jumpHost">
                        <div class="space-y-2">
                          <div>
                            <div class="text-[11px] font-medium text-slate-500 mb-1">{{ t('tools.applianceSsh.jumpHostGroupLabel', { ip: result.jumpHost }) }}</div>
                            <div class="flex flex-wrap gap-1.5">
                              <span
                                v-if="result.previousEnable !== undefined"
                                class="inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-medium"
                                :class="enableStateClass(result.previousEnable)"
                              >
                                {{ t('tools.applianceSsh.beforeState') }}: {{ formatEnableState(result.previousEnable) }}
                              </span>
                              <span
                                v-if="result.currentEnable !== undefined"
                                class="inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-medium"
                                :class="enableStateClass(result.currentEnable)"
                              >
                                {{ t('tools.applianceSsh.afterState') }}: {{ formatEnableState(result.currentEnable) }}
                              </span>
                              <span
                                v-if="result.port !== undefined"
                                class="inline-flex items-center rounded-md bg-slate-50 border border-slate-200 px-2 py-0.5 text-xs font-medium text-slate-600"
                              >
                                {{ t('tools.applianceSsh.portLabel') }}: {{ result.port }}
                              </span>
                            </div>
                          </div>
                          <div>
                            <div class="text-[11px] font-medium text-slate-500 mb-1">{{ t('tools.applianceSsh.targetGroupLabel', { ip: result.ip }) }}</div>
                            <div class="flex flex-wrap gap-1.5">
                              <span
                                v-if="result.whitelistSourceIp"
                                class="inline-flex items-center rounded-md bg-blue-50 border border-blue-200 px-2 py-0.5 text-xs font-medium text-blue-700"
                              >
                                {{ t('tools.applianceSsh.whitelistSourceIp') }}: {{ result.whitelistSourceIp }}
                              </span>
                              <span
                                v-if="result.whitelistApplied === true || result.whitelistApplied === false"
                                :class="result.whitelistApplied
                                  ? 'bg-emerald-50 text-emerald-700 border-emerald-200'
                                  : 'bg-red-50 text-red-600 border-red-200'"
                                class="inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-medium"
                              >
                                {{ result.whitelistApplied ? t('tools.applianceSsh.whitelistAdded') : t('tools.applianceSsh.whitelistFailed') }}
                              </span>
                              <span
                                v-if="!result.whitelistSourceIp && result.whitelistApplied === undefined"
                                class="text-xs text-slate-400"
                              >—</span>
                            </div>
                          </div>
                        </div>
                      </template>
                      <div v-else class="flex flex-wrap gap-1.5">
                        <span
                          v-if="result.previousEnable !== undefined"
                          class="inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-medium"
                          :class="enableStateClass(result.previousEnable)"
                        >
                          {{ t('tools.applianceSsh.beforeState') }}: {{ formatEnableState(result.previousEnable) }}
                        </span>
                        <span
                          v-if="result.currentEnable !== undefined"
                          class="inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-medium"
                          :class="enableStateClass(result.currentEnable)"
                        >
                          {{ t('tools.applianceSsh.afterState') }}: {{ formatEnableState(result.currentEnable) }}
                        </span>
                        <span
                          v-if="result.port !== undefined"
                          class="inline-flex items-center rounded-md bg-slate-50 border border-slate-200 px-2 py-0.5 text-xs font-medium text-slate-600"
                        >
                          {{ t('tools.applianceSsh.portLabel') }}: {{ result.port }}
                        </span>
                        <span
                          v-if="result.whitelistSourceIp"
                          class="inline-flex items-center rounded-md bg-blue-50 border border-blue-200 px-2 py-0.5 text-xs font-medium text-blue-700"
                        >
                          {{ t('tools.applianceSsh.whitelistSourceIp') }}: {{ result.whitelistSourceIp }}
                        </span>
                        <span
                          v-if="result.whitelistApplied === true || result.whitelistApplied === false"
                          :class="result.whitelistApplied
                            ? 'bg-emerald-50 text-emerald-700 border-emerald-200'
                            : 'bg-red-50 text-red-600 border-red-200'"
                          class="inline-flex items-center rounded-md border px-2 py-0.5 text-xs font-medium"
                        >
                          {{ result.whitelistApplied ? t('tools.applianceSsh.whitelistAdded') : t('tools.applianceSsh.whitelistFailed') }}
                        </span>
                      </div>
                    </div>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
