<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { ref, computed, onMounted, nextTick } from 'vue';
import { useI18n } from 'vue-i18n';
import {
  AlertCircle,
  Building2,
  Check,
  CheckCircle2,
  Database,
  Eye,
  EyeOff,
  Globe,
  KeyRound,
  Layers,
  Loader,
  Minus,
  Server,
  Trash2,
} from 'lucide-vue-next';
import {
  changeUmsInitPassword,
  type UmsInitPasswordKind,
  type UmsInitPasswordResult,
} from '../lib/tauri';
import { configStore } from '../lib/configStore';
import { mergeRecentItems, normalizeRecentItems, removeRecentItems } from '../lib/recentHistory';
import Empty from '../components/Empty.vue';
import { pushToast } from '../composables/useToast';

const { t } = useI18n();

const selectedIps = ref<string[]>([]);
const manualIpTags = ref<string[]>([]);
const manualIpInput = ref<string>('');
const fpIpInputRef = ref<HTMLInputElement | null>(null);
const recentIps = ref<string[]>([]);
const apiTimeoutSecs = ref<number>(5);
const isLoading = ref<boolean>(false);
const results = ref<UmsInitPasswordResult[]>([]);
const currentProgress = ref<{ current: number; total: number } | null>(null);
const RECENT_IPS_KEY = 'umsInitialPassword.recentIps';
const LEGACY_RECENT_IPS_KEY = 'frameworkPassword.recentIps';
const RECENT_IPS_LIMIT = 10;

// Shared new password plus one editable old password per flow. Each appliance ships
// with a different factory default, so the old passwords cannot be collapsed into one.
const newPassword = ref<string>('admin_123');
const showNewPassword = ref(false);

interface FlowDefinition {
  kind: UmsInitPasswordKind;
  icon: typeof Layers;
  labelKey: string;
  account: string;
  port: number;
}

const FLOWS: FlowDefinition[] = [
  { kind: 'framework', icon: Layers, labelKey: 'tools.umsInitialPassword.scope.framework', account: 'admin', port: 21900 },
  { kind: 'ums', icon: Building2, labelKey: 'tools.umsInitialPassword.scope.ums', account: 'loadmin', port: 80 },
  { kind: 'cdm', icon: Database, labelKey: 'tools.umsInitialPassword.scope.cdm', account: 'admin', port: 25011 },
];

const enabledFlows = ref<Record<UmsInitPasswordKind, boolean>>({
  framework: true,
  ums: true,
  cdm: true,
});

const oldPasswords = ref<Record<UmsInitPasswordKind, string>>({
  framework: '123456',
  ums: 'admin_123',
  cdm: 'admin',
});

const showOldPassword = ref<Record<UmsInitPasswordKind, boolean>>({
  framework: false,
  ums: false,
  cdm: false,
});

const toggleFlow = (kind: UmsInitPasswordKind) => {
  enabledFlows.value[kind] = !enabledFlows.value[kind];
};

const selectedFlowCount = computed(() => FLOWS.filter(flow => enabledFlows.value[flow.kind]).length);

// A flow only conflicts when it is actually selected. UMS ships with `admin_123`,
// which is the very value most people type as the new password, so this check has to
// name the offending flow instead of showing one generic warning.
const conflictingFlows = computed(() =>
  FLOWS.filter(flow => enabledFlows.value[flow.kind] && oldPasswords.value[flow.kind] === newPassword.value),
);

const hasFlowConflict = (kind: UmsInitPasswordKind) =>
  conflictingFlows.value.some(flow => flow.kind === kind);

const SEPARATORS = /[\s,，、;；\n\r]+/;

const isValidIp = (ip: string): boolean => {
  const parts = ip.split('.');
  if (parts.length !== 4) return false;
  return parts.every(p => /^\d+$/.test(p) && Number(p) >= 0 && Number(p) <= 255);
};

const serverOptions = computed(() => {
  const config = configStore.config;
  if (!config) return [];
  return config.servers
    .filter(server => server.enabled)
    .map(server => ({
      id: server.id,
      host: server.host,
      name: server.name || server.host,
    }));
});

const addManualIpTag = (raw: string) => {
  const parts = raw.split(SEPARATORS).map(s => s.trim()).filter(Boolean);
  for (const ip of parts) {
    if (!manualIpTags.value.includes(ip)) {
      manualIpTags.value.push(ip);
    }
  }
};

const removeManualIpTag = (ip: string) => {
  const idx = manualIpTags.value.indexOf(ip);
  if (idx > -1) manualIpTags.value.splice(idx, 1);
};

// Clicking a tag's text moves it back into the input so a single character can
// be edited (e.g. 192.115.2.30 → 192.115.2.130) instead of deleting it whole.
const editManualIpTag = (ip: string) => {
  if (manualIpInput.value.trim()) {
    addManualIpTag(manualIpInput.value);
  }
  removeManualIpTag(ip);
  manualIpInput.value = ip;
  nextTick(() => fpIpInputRef.value?.focus());
};

const handleIpKeydown = (e: KeyboardEvent) => {
  const raw = manualIpInput.value.trim();
  if (['Enter', 'Tab', ' '].includes(e.key)) {
    if (raw) {
      e.preventDefault();
      addManualIpTag(raw);
      manualIpInput.value = '';
    }
  } else if (e.key === 'Backspace' && !raw && manualIpTags.value.length > 0) {
    // Move the last tag back into the input for editing rather than deleting it.
    const last = manualIpTags.value.pop();
    if (last !== undefined) {
      manualIpInput.value = last;
    }
  }
};

const handleIpInputChange = () => {
  if (SEPARATORS.test(manualIpInput.value)) {
    addManualIpTag(manualIpInput.value);
    manualIpInput.value = '';
  }
};

const handleIpPaste = (e: ClipboardEvent) => {
  e.preventDefault();
  const text = e.clipboardData?.getData('text') ?? '';
  addManualIpTag(text);
  manualIpInput.value = '';
};

const handleIpBlur = () => {
  if (manualIpInput.value.trim()) {
    addManualIpTag(manualIpInput.value);
    manualIpInput.value = '';
  }
};

const allSelectedIps = computed(() => {
  const ips = new Set<string>([...selectedIps.value, ...manualIpTags.value]);
  if (manualIpInput.value.trim()) {
    ips.add(manualIpInput.value.trim());
  }
  return Array.from(ips);
});

const isRecentIpSelected = (ip: string) => allSelectedIps.value.includes(ip);

const applyRecentIp = (ip: string) => {
  if (isLoading.value || isRecentIpSelected(ip)) {
    return;
  }
  manualIpInput.value = '';
  addManualIpTag(ip);
  nextTick(() => fpIpInputRef.value?.focus());
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

const isFormValid = computed(
  () =>
    allSelectedIps.value.length > 0 &&
    selectedFlowCount.value > 0 &&
    newPassword.value.length > 0 &&
    conflictingFlows.value.length === 0 &&
    !isLoading.value,
);

onMounted(async () => {
  try {
    await configStore.ensureLoaded();
    apiTimeoutSecs.value = configStore.config?.framework_password_api_timeout_secs ?? 5;
  } catch (e) {
    console.error('Failed to load config:', e);
  }

  try {
    const saved = await invoke<string[] | null>('load_kv', { key: RECENT_IPS_KEY });
    let normalized = normalizeRecentItems(saved, RECENT_IPS_LIMIT);
    if (normalized.length === 0) {
      // Carry over history collected while this tool was still "framework password".
      const legacy = await invoke<string[] | null>('load_kv', { key: LEGACY_RECENT_IPS_KEY });
      normalized = normalizeRecentItems(legacy, RECENT_IPS_LIMIT);
      if (normalized.length > 0) {
        await storeRecentIps(normalized);
        return;
      }
    }
    recentIps.value = normalized;
  } catch {
    // Ignore malformed recent history from older builds.
  }
});

const saveApiTimeout = async () => {
  try {
    await configStore.ensureLoaded();
    if (!configStore.config) return;
    configStore.config.framework_password_api_timeout_secs = apiTimeoutSecs.value;
    await configStore.saveApp();
  } catch (e) {
    console.error('Failed to save api timeout:', e);
  }
};

const handleExecute = async () => {
  if (allSelectedIps.value.length === 0) {
    pushToast(t('tools.umsInitialPassword.noIps'), 'warning');
    return;
  }
  if (selectedFlowCount.value === 0) {
    pushToast(t('tools.umsInitialPassword.noTargetSelected'), 'warning');
    return;
  }
  if (conflictingFlows.value.length > 0) {
    pushToast(
      t('tools.umsInitialPassword.samePasswordFor', {
        target: conflictingFlows.value.map(flow => t(flow.labelKey)).join('、'),
      }),
      'warning',
    );
    return;
  }

  isLoading.value = true;
  results.value = [];
  const recentValidIps = allSelectedIps.value.filter(isValidIp);

  try {
    const ipList = allSelectedIps.value;
    currentProgress.value = { current: 0, total: ipList.length };
    await rememberRecentIps(recentValidIps);

    const response = await changeUmsInitPassword({
      ips: ipList,
      targets: { ...enabledFlows.value },
      newPassword: newPassword.value,
      frameworkOldPassword: oldPasswords.value.framework,
      umsOldPassword: oldPasswords.value.ums,
      cdmOldPassword: oldPasswords.value.cdm,
    });
    results.value = response;
    pushToast(
      t('tools.umsInitialPassword.completed', {
        success: response.filter(item => item.success).length,
        total: response.length,
      }),
      'success',
      { ttlMs: 2600 },
    );
    currentProgress.value = { current: ipList.length, total: ipList.length };
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    results.value = allSelectedIps.value.map(ip => ({
      ip,
      success: false,
      targets: FLOWS.filter(flow => enabledFlows.value[flow.kind]).map(flow => ({
        kind: flow.kind,
        success: false,
        message: `Error: ${errorMessage}`,
        failedAt: 'login' as const,
      })),
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

const targetOf = (result: UmsInitPasswordResult, kind: UmsInitPasswordKind) =>
  result.targets.find(target => target.kind === kind);

/** Failure text for one row, prefixed with the flow it came from. */
const failureDetail = (result: UmsInitPasswordResult) =>
  result.targets
    .filter(target => !target.success)
    .map(target => `${t(`tools.umsInitialPassword.scope.${target.kind}`)}: ${target.message}`)
    .join(' / ');

/** Notes carried by succeeded flows, e.g. the UMS switch step failing after a good change. */
const successNotes = (result: UmsInitPasswordResult) =>
  result.targets
    .filter(target => target.success && target.message !== '成功' && target.message !== 'Success')
    .map(target => `${t(`tools.umsInitialPassword.scope.${target.kind}`)}: ${target.message}`)
    .join(' / ');

const resultDetail = (result: UmsInitPasswordResult) =>
  [failureDetail(result), successNotes(result)].filter(Boolean).join(' / ');

const umsResultStatusWrapClass = 'flex items-center gap-1.5 whitespace-nowrap';
const umsResultMessageCellClass = 'px-6 py-3 text-sm text-slate-600 break-all';
</script>

<template>
  <div class="flex-1 flex flex-col bg-gradient-to-br from-slate-50 to-slate-100 overflow-y-auto">
    <div class="max-w-6xl w-full mx-auto p-6 pb-10 space-y-5">
    <!-- Header Section -->
    <div class="flex items-start gap-3">
      <div class="w-10 h-10 rounded-xl bg-gradient-to-br from-amber-500 to-orange-600 flex items-center justify-center shadow-sm shrink-0">
        <KeyRound class="w-5 h-5 text-white" />
      </div>
      <div>
        <h1 class="text-2xl font-bold text-slate-900 mb-1">{{ t('tools.umsInitialPassword.title') }}</h1>
        <p class="text-slate-500 text-sm">{{ t('tools.umsInitialPassword.description') }}</p>
      </div>
    </div>

    <!-- Info Banner -->
    <div class="bg-blue-50/70 border border-blue-200/60 p-4 rounded-xl">
      <p class="text-blue-900 text-sm leading-relaxed">
        <span class="font-semibold">{{ t('tools.umsInitialPassword.info') }}</span><br>
        {{ t('tools.umsInitialPassword.infoDetail') }}
      </p>
    </div>

    <!-- Main Form -->
    <div class="grid grid-cols-1 lg:grid-cols-3 gap-8">
      <!-- Input Section -->
      <div class="lg:col-span-2 space-y-6">

        <!-- Server Selection Card -->
        <div class="bg-white border border-slate-200/80 rounded-xl p-5 shadow-sm">
          <div class="flex items-center gap-2 mb-4">
            <Server class="w-4 h-4 text-slate-400" />
            <h3 class="text-sm font-semibold text-slate-800">{{ t('tools.umsInitialPassword.selectServer') }}</h3>
            <span class="text-xs text-slate-400">({{ t('tools.umsInitialPassword.optional') }})</span>
          </div>

          <div v-if="serverOptions.length > 0" class="grid grid-cols-1 sm:grid-cols-2 gap-1.5">
            <label
              v-for="server in serverOptions"
              :key="server.id"
              :for="`ums-init-password-server-${server.id}`"
              class="flex items-center gap-2.5 px-3 py-2 rounded-lg cursor-pointer transition-colors"
              :class="isServerSelected(server.host) ? 'bg-blue-50 border border-blue-200' : 'hover:bg-slate-50 border border-transparent'"
            >
              <input
                type="checkbox"
                :id="`ums-init-password-server-${server.id}`"
                :checked="isServerSelected(server.host)"
                @change="toggleServerIp(server.host)"
                :disabled="isLoading"
                class="rounded border-slate-300 text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
              />
              <span class="flex-1 text-sm text-slate-700 cursor-pointer">
                <span class="font-medium">{{ server.name }}</span>
                <span class="text-slate-400 ml-1">({{ server.host }})</span>
              </span>
            </label>
          </div>
          <div v-else class="text-sm text-slate-400">{{ t('tools.umsInitialPassword.noServers') }}</div>
        </div>

        <!-- Scope + Password Config Card -->
        <div class="bg-white border border-slate-200/80 rounded-xl p-5 shadow-sm">
          <div class="flex items-center gap-2 mb-4">
            <KeyRound class="w-4 h-4 text-slate-400" />
            <h3 class="text-sm font-semibold text-slate-800">{{ t('tools.umsInitialPassword.passwordConfig') }}</h3>
          </div>

          <!-- Shared new password -->
          <div class="space-y-1.5">
            <label class="block text-xs font-medium text-slate-600">{{ t('tools.umsInitialPassword.newPassword') }}</label>
            <div class="flex items-center gap-2">
              <input
                v-model="newPassword"
                :type="showNewPassword ? 'text' : 'password'"
                autocomplete="new-password"
                :placeholder="t('tools.umsInitialPassword.newPasswordPlaceholder')"
                :disabled="isLoading"
                class="flex-1 px-3 py-2 text-sm border rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
                :class="conflictingFlows.length > 0 ? 'border-amber-300 bg-amber-50' : 'border-slate-200'"
              />
              <button
                type="button"
                class="inline-flex items-center gap-1 text-xs font-medium text-slate-500 transition-colors hover:text-slate-700 shrink-0"
                @click="showNewPassword = !showNewPassword"
              >
                <component :is="showNewPassword ? EyeOff : Eye" class="h-3.5 w-3.5" />
                {{ t(showNewPassword ? 'tools.umsInitialPassword.hidePassword' : 'tools.umsInitialPassword.showPassword') }}
              </button>
            </div>
            <p class="text-xs text-slate-400">{{ t('tools.umsInitialPassword.newPasswordHint') }}</p>
          </div>

          <!-- Per-flow selection with its own old password -->
          <div class="mt-5 space-y-2">
            <p class="text-xs font-medium text-slate-600">{{ t('tools.umsInitialPassword.scope.legend') }}</p>
            <div
              v-for="flow in FLOWS"
              :key="flow.kind"
              class="rounded-lg border px-3 py-2.5 transition-colors"
              :class="enabledFlows[flow.kind] ? 'border-blue-200 bg-blue-50/50' : 'border-slate-200 bg-slate-50/60'"
            >
              <label
                :for="`ums-init-password-flow-${flow.kind}`"
                class="flex items-center gap-2.5 cursor-pointer"
              >
                <input
                  type="checkbox"
                  :id="`ums-init-password-flow-${flow.kind}`"
                  :checked="enabledFlows[flow.kind]"
                  @change="toggleFlow(flow.kind)"
                  :disabled="isLoading"
                  class="rounded border-slate-300 text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
                />
                <component :is="flow.icon" class="w-4 h-4 text-slate-400 shrink-0" />
                <span class="text-sm font-medium text-slate-800">{{ t(flow.labelKey) }}</span>
                <span class="text-xs text-slate-400 font-mono">:{{ flow.port }} · {{ flow.account }}</span>
              </label>

              <div v-if="enabledFlows[flow.kind]" class="mt-2.5 pl-7 flex items-center gap-2">
                <label class="text-xs text-slate-500 shrink-0" :for="`ums-init-password-old-${flow.kind}`">
                  {{ t('tools.umsInitialPassword.oldPasswordFor') }}
                </label>
                <input
                  :id="`ums-init-password-old-${flow.kind}`"
                  v-model="oldPasswords[flow.kind]"
                  :type="showOldPassword[flow.kind] ? 'text' : 'password'"
                  autocomplete="new-password"
                  :disabled="isLoading"
                  class="flex-1 max-w-[16rem] px-3 py-1.5 text-sm border rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 transition-colors"
                  :class="hasFlowConflict(flow.kind) ? 'border-amber-300 bg-amber-50' : 'border-slate-200 bg-white'"
                />
                <button
                  type="button"
                  class="inline-flex items-center text-slate-400 transition-colors hover:text-slate-600 shrink-0"
                  :title="t(showOldPassword[flow.kind] ? 'tools.umsInitialPassword.hidePassword' : 'tools.umsInitialPassword.showPassword')"
                  @click="showOldPassword[flow.kind] = !showOldPassword[flow.kind]"
                >
                  <component :is="showOldPassword[flow.kind] ? EyeOff : Eye" class="h-4 w-4" />
                </button>
              </div>
              <p v-if="hasFlowConflict(flow.kind)" class="mt-1.5 pl-7 text-xs font-medium text-amber-700">
                {{ t('tools.umsInitialPassword.samePasswordFor', { target: t(flow.labelKey) }) }}
              </p>
            </div>
            <p v-if="selectedFlowCount === 0" class="text-xs font-medium text-amber-700">
              {{ t('tools.umsInitialPassword.noTargetSelected') }}
            </p>
          </div>
        </div>

        <!-- Manual IP Input Card -->
        <div class="bg-white border border-slate-200/80 rounded-xl p-5 shadow-sm">
          <div class="flex items-center gap-2 mb-3">
            <Globe class="w-4 h-4 text-slate-400" />
            <label class="block text-sm font-semibold text-slate-800">{{ t('tools.umsInitialPassword.manualIp') }}</label>
          </div>
          <div class="space-y-2">
            <!-- Tag Input -->
            <div
              role="listbox"
              :aria-label="t('tools.umsInitialPassword.manualIp')"
              class="min-h-[2.375rem] w-full flex flex-wrap gap-1.5 px-2.5 py-1.5 border border-slate-200 rounded-lg transition-colors cursor-text"
              :class="isLoading ? 'bg-slate-50 cursor-not-allowed' : 'bg-white focus-within:border-blue-400 focus-within:ring-2 focus-within:ring-blue-400/20'"
              @click="fpIpInputRef?.focus()"
            >
              <span
                v-for="ip in manualIpTags"
                :key="ip"
                class="inline-flex items-center gap-1 text-xs font-mono px-2 py-0.5 rounded-md"
                :class="isValidIp(ip)
                  ? 'bg-blue-100 text-blue-800'
                  : 'bg-red-100 text-red-700 border border-red-200'"
                :title="isValidIp(ip) ? undefined : t('tools.umsInitialPassword.invalidIp', { ip })"
              >
                <button
                  type="button"
                  :disabled="isLoading"
                  class="disabled:cursor-not-allowed leading-none font-mono"
                  :title="t('tools.umsInitialPassword.editTag')"
                  @click.stop="editManualIpTag(ip)"
                >{{ ip }}</button>
                <button
                  type="button"
                  :disabled="isLoading"
                  class="disabled:cursor-not-allowed leading-none"
                  :class="isValidIp(ip) ? 'text-blue-500 hover:text-blue-700' : 'text-red-400 hover:text-red-600'"
                  @click.stop="removeManualIpTag(ip)"
                >×</button>
              </span>
              <input
                ref="fpIpInputRef"
                v-model="manualIpInput"
                type="text"
                list="ums-init-password-recent-ips"
                :placeholder="manualIpTags.length === 0 ? t('tools.umsInitialPassword.manualIpPlaceholder') : ''"
                :disabled="isLoading"
                class="flex-1 min-w-[120px] text-sm bg-transparent outline-none disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 py-0.5"
                @keydown="handleIpKeydown"
                @input="handleIpInputChange"
                @paste="handleIpPaste"
                @blur="handleIpBlur"
              />
            </div>
            <datalist id="ums-init-password-recent-ips">
              <option v-for="ip in recentIps" :key="`ums-init-password-recent-${ip}`" :value="ip" />
            </datalist>
            <p class="text-xs text-slate-400">{{ t('tools.umsInitialPassword.manualIpHint') }}</p>
            <div v-if="recentIps.length > 0" class="space-y-2">
              <div class="flex items-center justify-between gap-2">
                <span class="text-xs font-medium text-slate-500">{{ t('tools.umsInitialPassword.recentIps') }}</span>
                <button
                  type="button"
                  :disabled="isLoading"
                  class="text-xs font-medium text-slate-500 hover:text-slate-700 disabled:cursor-not-allowed disabled:opacity-50"
                  @click="clearRecentIps"
                >
                  {{ t('tools.umsInitialPassword.clearRecentIps') }}
                </button>
              </div>
              <div class="flex items-center gap-2 flex-wrap">
                <span
                  v-for="ip in recentIps"
                  :key="`ums-init-password-history-${ip}`"
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
                    :title="t('tools.umsInitialPassword.removeRecentIp')"
                    @click.stop="removeRecentIp(ip)"
                  >
                    <Trash2 class="h-3.5 w-3.5" />
                  </button>
                </span>
              </div>
            </div>
          </div>
        </div>

        <!-- Selected IPs Display -->
        <div v-if="allSelectedIps.length > 0" class="bg-slate-50 border border-slate-200/80 rounded-xl px-4 py-3">
          <p class="text-xs font-medium text-slate-500 mb-2">{{ t('tools.umsInitialPassword.selectedIps', { count: allSelectedIps.length }) }}</p>
          <div class="flex flex-wrap gap-1.5">
            <div v-for="ip in allSelectedIps" :key="ip" class="inline-flex items-center gap-2 bg-blue-100/80 text-blue-800 px-2.5 py-0.5 rounded-md text-xs">
              <span class="font-mono">{{ ip }}</span>
            </div>
          </div>
        </div>

        <!-- API Timeout -->
        <div class="flex items-center justify-between gap-3 px-4 py-3 bg-slate-50 border border-slate-200/80 rounded-xl">
          <div>
            <span class="text-xs font-medium text-slate-600">{{ t('tools.umsInitialPassword.apiTimeout') }}</span>
            <p class="text-xs text-slate-400 mt-0.5">{{ t('tools.umsInitialPassword.apiTimeoutDesc') }}</p>
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

        <!-- Execute Button -->
        <button
          @click="handleExecute"
          :disabled="!isFormValid"
          class="w-full px-5 py-3 bg-gradient-to-r from-blue-600 to-blue-700 text-white font-semibold rounded-xl hover:from-blue-700 hover:to-blue-800 focus:outline-none focus:ring-2 focus:ring-blue-500/40 focus:ring-offset-1 disabled:opacity-40 disabled:cursor-not-allowed transition-all duration-200 flex items-center justify-center gap-2 text-base shadow-sm"
        >
          <Loader v-if="isLoading" class="w-5 h-5 animate-spin" />
          <span>{{ isLoading ? t('tools.umsInitialPassword.processing') : t('tools.umsInitialPassword.executeButton') }}</span>
        </button>
      </div>

      <!-- Stats Card -->
      <div class="bg-white border border-slate-200/80 rounded-xl p-5 shadow-sm h-fit sticky top-6">
        <h3 class="text-sm font-semibold text-slate-800 mb-4">{{ t('tools.umsInitialPassword.results') }}</h3>

        <div class="space-y-4">
          <div class="flex items-center justify-between">
            <span class="text-slate-600">{{ t('tools.umsInitialPassword.totalLabel') }}:</span>
            <span class="text-2xl font-bold text-slate-900">{{ results.length }}</span>
          </div>
          <div class="flex items-center justify-between">
            <span class="text-slate-600">{{ t('tools.umsInitialPassword.successLabel') }}:</span>
            <span class="text-2xl font-bold text-green-600">{{ successCount }}</span>
          </div>
          <div class="flex items-center justify-between">
            <span class="text-slate-600">{{ t('tools.umsInitialPassword.failedLabel') }}:</span>
            <span class="text-2xl font-bold text-red-600">{{ failureCount }}</span>
          </div>

          <div v-if="currentProgress" class="mt-6 pt-4 border-t border-slate-200">
            <div class="text-xs text-slate-500 mb-2">
              {{ t('tools.umsInitialPassword.progress', { current: currentProgress.current, total: currentProgress.total }) }}
            </div>
            <div class="w-full bg-slate-200 rounded-full h-2">
              <div
                class="bg-gradient-to-r from-blue-500 to-blue-600 h-2 rounded-full transition-all duration-300"
                :style="{ width: `${(currentProgress.current / currentProgress.total) * 100}%` }"
              ></div>
            </div>
          </div>

          <p class="text-xs text-slate-400 pt-2 border-t border-slate-200">
            {{ t('tools.umsInitialPassword.logHint') }}
          </p>

          <Empty
            v-if="results.length === 0 && !currentProgress"
            :title="t('tools.umsInitialPassword.emptyTitle')"
            :description="t('tools.umsInitialPassword.emptyDescription')"
            dashed
          />
        </div>
      </div>
    </div>

    <!-- Results Table -->
    <div v-if="results.length > 0" class="mt-8">
      <div class="bg-white border border-slate-200 rounded-lg overflow-hidden shadow-sm">
        <div class="overflow-x-auto">
          <table class="w-full table-fixed">
            <colgroup>
              <col style="width: 160px">
              <col style="width: 110px">
              <col style="width: 110px">
              <col style="width: 110px">
              <col>
            </colgroup>
            <thead>
              <tr class="border-b border-slate-200 bg-slate-50">
                <th scope="col" class="px-6 py-3 text-left text-sm font-semibold text-slate-700">IP {{ t('tools.umsInitialPassword.address') }}</th>
                <th v-for="flow in FLOWS" :key="`head-${flow.kind}`" scope="col" class="px-3 py-3 text-left text-sm font-semibold text-slate-700">
                  {{ t(flow.labelKey) }}
                </th>
                <th scope="col" class="px-6 py-3 text-left text-sm font-semibold text-slate-700">{{ t('tools.umsInitialPassword.detail') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="result in results" :key="result.ip" class="border-b border-slate-200 hover:bg-slate-50 transition-colors">
                <td class="px-6 py-3 text-sm font-mono text-slate-900">{{ result.ip }}</td>
                <td v-for="flow in FLOWS" :key="`${result.ip}-${flow.kind}`" class="px-3 py-3">
                  <div :class="umsResultStatusWrapClass">
                    <template v-if="targetOf(result, flow.kind)">
                      <component
                        :is="targetOf(result, flow.kind)!.success ? CheckCircle2 : AlertCircle"
                        :class="targetOf(result, flow.kind)!.success ? 'text-green-500' : 'text-red-500'"
                        class="w-4 h-4 shrink-0"
                      />
                      <span
                        :class="targetOf(result, flow.kind)!.success ? 'text-green-600' : 'text-red-600'"
                        class="text-sm font-semibold"
                      >
                        {{ targetOf(result, flow.kind)!.success ? t('tools.umsInitialPassword.success') : t('tools.umsInitialPassword.failed') }}
                      </span>
                    </template>
                    <template v-else>
                      <Minus class="w-4 h-4 text-slate-300 shrink-0" />
                      <span class="text-sm text-slate-400">{{ t('tools.umsInitialPassword.targetSkipped') }}</span>
                    </template>
                  </div>
                </td>
                <td :class="umsResultMessageCellClass">{{ resultDetail(result) }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
    </div>
  </div>
</template>
