<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { ref, computed, onMounted, nextTick } from 'vue';
import { useI18n } from 'vue-i18n';
import { AlertCircle, CheckCircle2, Globe, KeyRound, Loader, Server } from 'lucide-vue-next';
import { changeFrameworkPassword, getConfig, saveConfig, type AppConfig, type FrameworkPasswordResult } from '../lib/tauri';
import { mergeRecentItems, normalizeRecentItems } from '../lib/recentHistory';

const { t } = useI18n();

const config = ref<AppConfig | null>(null);
const selectedIps = ref<string[]>([]);
const manualIpTags = ref<string[]>([]);
const manualIpInput = ref<string>('');
const fpIpInputRef = ref<HTMLInputElement | null>(null);
const recentIps = ref<string[]>([]);
const oldPassword = ref<string>('123456');
const newPassword = ref<string>('admin_123');
const apiTimeoutSecs = ref<number>(5);
const isLoading = ref<boolean>(false);
const results = ref<FrameworkPasswordResult[]>([]);
const currentProgress = ref<{ current: number; total: number } | null>(null);
const RECENT_IPS_KEY = 'frameworkPassword.recentIps';
const RECENT_IPS_LIMIT = 10;

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

const SEPARATORS = /[\s,，、;；\n\r]+/;

const isValidIp = (ip: string): boolean => {
  const parts = ip.split('.');
  if (parts.length !== 4) return false;
  return parts.every(p => /^\d+$/.test(p) && Number(p) >= 0 && Number(p) <= 255);
};

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

const restoreOrRemoveTag = (ip: string) => {
  removeManualIpTag(ip);
  if (!isValidIp(ip)) {
    manualIpInput.value = ip;
    nextTick(() => fpIpInputRef.value?.focus());
  }
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
    manualIpTags.value.pop();
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

const isFormValid = computed(() => {
  return allSelectedIps.value.length > 0 && !isLoading.value;
});

onMounted(async () => {
  try {
    config.value = await getConfig();
    apiTimeoutSecs.value = config.value.framework_password_api_timeout_secs ?? 5;
  } catch (e) {
    console.error('Failed to load config:', e);
  }

  try {
    const saved = await invoke<string[] | null>('load_kv', { key: RECENT_IPS_KEY });
    recentIps.value = normalizeRecentItems(saved, RECENT_IPS_LIMIT);
  } catch {
    // Ignore malformed recent history from older builds.
  }
});

const saveApiTimeout = async () => {
  if (!config.value) return;
  config.value.framework_password_api_timeout_secs = apiTimeoutSecs.value;
  try {
    await saveConfig(config.value);
  } catch (e) {
    console.error('Failed to save api timeout:', e);
  }
};

const handleExecute = async () => {
  if (allSelectedIps.value.length === 0) {
    alert(t('tools.frameworkPassword.noIps'));
    return;
  }

  isLoading.value = true;
  results.value = [];
  const recentValidIps = allSelectedIps.value.filter(isValidIp);

  try {
    const ipList = allSelectedIps.value;
    currentProgress.value = { current: 0, total: ipList.length };
    await rememberRecentIps(recentValidIps);

    const response = await changeFrameworkPassword(
      ipList,
      oldPassword.value || undefined,
      newPassword.value || undefined,
    );
    results.value = response;
    currentProgress.value = { current: ipList.length, total: ipList.length };
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    results.value = allSelectedIps.value.map(ip => ({
      ip,
      success: false,
      message: `Error: ${errorMessage}`,
      failedAt: 'login',
    }));
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
        <h1 class="text-2xl font-bold text-slate-900 mb-1">{{ t('tools.frameworkPassword.title') }}</h1>
        <p class="text-slate-500 text-sm">{{ t('tools.frameworkPassword.description') }}</p>
      </div>
    </div>

    <!-- Info Banner -->
    <div class="bg-blue-50/70 border border-blue-200/60 p-4 rounded-xl">
      <p class="text-blue-900 text-sm leading-relaxed">
        <span class="font-semibold">{{ t('tools.frameworkPassword.info') }}</span><br>
        {{ t('tools.frameworkPassword.infoDetail') }}
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
            <h3 class="text-sm font-semibold text-slate-800">{{ t('tools.frameworkPassword.selectServer') }}</h3>
            <span class="text-xs text-slate-400">({{ t('tools.frameworkPassword.optional') }})</span>
          </div>

          <div v-if="serverOptions.length > 0" class="grid grid-cols-1 sm:grid-cols-2 gap-1.5">
            <label
              v-for="server in serverOptions"
              :key="server.id"
              :for="`framework-password-server-${server.id}`"
              class="flex items-center gap-2.5 px-3 py-2 rounded-lg cursor-pointer transition-colors"
              :class="isServerSelected(server.host) ? 'bg-blue-50 border border-blue-200' : 'hover:bg-slate-50 border border-transparent'"
            >
              <input
                type="checkbox"
                :id="`framework-password-server-${server.id}`"
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
          <div v-else class="text-sm text-slate-400">{{ t('tools.frameworkPassword.noServers') }}</div>
        </div>

        <!-- Password Config Card -->
        <div class="bg-white border border-slate-200/80 rounded-xl p-5 shadow-sm">
          <div class="flex items-center gap-2 mb-4">
            <KeyRound class="w-4 h-4 text-slate-400" />
            <h3 class="text-sm font-semibold text-slate-800">{{ t('tools.frameworkPassword.passwordConfig') }}</h3>
          </div>
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div class="space-y-1.5">
              <label class="block text-xs font-medium text-slate-600">{{ t('tools.frameworkPassword.oldPassword') }}</label>
              <input
                v-model="oldPassword"
                type="text"
                :placeholder="t('tools.frameworkPassword.oldPasswordPlaceholder')"
                :disabled="isLoading"
                class="w-full px-3 py-2 text-sm border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
              />
            </div>
            <div class="space-y-1.5">
              <label class="block text-xs font-medium text-slate-600">{{ t('tools.frameworkPassword.newPassword') }}</label>
              <input
                v-model="newPassword"
                type="text"
                :placeholder="t('tools.frameworkPassword.newPasswordPlaceholder')"
                :disabled="isLoading"
                class="w-full px-3 py-2 text-sm border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
              />
            </div>
          </div>
          <p class="text-xs text-slate-400 mt-2">{{ t('tools.frameworkPassword.passwordConfigHint') }}</p>
        </div>

        <!-- Manual IP Input Card -->
        <div class="bg-white border border-slate-200/80 rounded-xl p-5 shadow-sm">
          <div class="flex items-center gap-2 mb-3">
            <Globe class="w-4 h-4 text-slate-400" />
            <label class="block text-sm font-semibold text-slate-800">{{ t('tools.frameworkPassword.manualIp') }}</label>
          </div>
          <div class="space-y-2">
            <!-- Tag Input -->
            <div
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
                :title="isValidIp(ip) ? undefined : t('tools.frameworkPassword.invalidIp', { ip })"
              >
                {{ ip }}
                <button
                  type="button"
                  :disabled="isLoading"
                  class="disabled:cursor-not-allowed leading-none"
                  :class="isValidIp(ip) ? 'text-blue-500 hover:text-blue-700' : 'text-red-400 hover:text-red-600'"
                  @click.stop="restoreOrRemoveTag(ip)"
                >×</button>
              </span>
              <input
                ref="fpIpInputRef"
                v-model="manualIpInput"
                type="text"
                list="framework-password-recent-ips"
                :placeholder="manualIpTags.length === 0 ? t('tools.frameworkPassword.manualIpPlaceholder') : ''"
                :disabled="isLoading"
                class="flex-1 min-w-[120px] text-sm bg-transparent outline-none disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 py-0.5"
                @keydown="handleIpKeydown"
                @input="handleIpInputChange"
                @paste="handleIpPaste"
                @blur="handleIpBlur"
              />
            </div>
            <datalist id="framework-password-recent-ips">
              <option v-for="ip in recentIps" :key="`framework-password-recent-${ip}`" :value="ip" />
            </datalist>
            <p class="text-xs text-slate-400">{{ t('tools.frameworkPassword.manualIpHint') }}</p>
            <div v-if="recentIps.length > 0" class="flex items-center gap-2 flex-wrap">
              <span class="text-xs font-medium text-slate-500">{{ t('history.title') }}:</span>
              <button
                v-for="ip in recentIps"
                :key="`framework-password-history-${ip}`"
                type="button"
                :disabled="isLoading"
                class="px-2.5 py-1 text-xs font-medium rounded-full border transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                :class="isRecentIpSelected(ip)
                  ? 'bg-blue-600 text-white border-blue-600'
                  : 'bg-white text-slate-600 border-slate-300 hover:bg-slate-50 hover:border-slate-400'"
                @click="applyRecentIp(ip)"
              >
                <span class="font-mono">{{ ip }}</span>
              </button>
            </div>
          </div>
        </div>

        <!-- Selected IPs Display -->
        <div v-if="allSelectedIps.length > 0" class="bg-slate-50 border border-slate-200/80 rounded-xl px-4 py-3">
          <p class="text-xs font-medium text-slate-500 mb-2">{{ t('tools.frameworkPassword.selectedIps', { count: allSelectedIps.length }) }}</p>
          <div class="flex flex-wrap gap-1.5">
            <div v-for="ip in allSelectedIps" :key="ip" class="inline-flex items-center gap-2 bg-blue-100/80 text-blue-800 px-2.5 py-0.5 rounded-md text-xs">
              <span class="font-mono">{{ ip }}</span>
            </div>
          </div>
        </div>

        <!-- API Timeout -->
        <div class="flex items-center justify-between gap-3 px-4 py-3 bg-slate-50 border border-slate-200/80 rounded-xl">
          <div>
            <span class="text-xs font-medium text-slate-600">{{ t('tools.frameworkPassword.apiTimeout') }}</span>
            <p class="text-xs text-slate-400 mt-0.5">{{ t('tools.frameworkPassword.apiTimeoutDesc') }}</p>
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
          <span>{{ isLoading ? t('tools.frameworkPassword.processing') : t('tools.frameworkPassword.executeButton') }}</span>
        </button>
      </div>

      <!-- Stats Card -->
      <div class="bg-white border border-slate-200/80 rounded-xl p-5 shadow-sm h-fit sticky top-6">
        <h3 class="text-sm font-semibold text-slate-800 mb-4">{{ t('tools.frameworkPassword.results') }}</h3>

        <div class="space-y-4">
          <div class="flex items-center justify-between">
            <span class="text-slate-600">{{ t('tools.frameworkPassword.totalLabel') }}:</span>
            <span class="text-2xl font-bold text-slate-900">{{ results.length }}</span>
          </div>
          <div class="flex items-center justify-between">
            <span class="text-slate-600">{{ t('tools.frameworkPassword.successLabel') }}:</span>
            <span class="text-2xl font-bold text-green-600">{{ successCount }}</span>
          </div>
          <div class="flex items-center justify-between">
            <span class="text-slate-600">{{ t('tools.frameworkPassword.failedLabel') }}:</span>
            <span class="text-2xl font-bold text-red-600">{{ failureCount }}</span>
          </div>

          <div v-if="currentProgress" class="mt-6 pt-4 border-t border-slate-200">
            <div class="text-xs text-slate-500 mb-2">
              {{ t('tools.frameworkPassword.progress', { current: currentProgress.current, total: currentProgress.total }) }}
            </div>
            <div class="w-full bg-slate-200 rounded-full h-2">
              <div
                class="bg-gradient-to-r from-blue-500 to-blue-600 h-2 rounded-full transition-all duration-300"
                :style="{ width: `${(currentProgress.current / currentProgress.total) * 100}%` }"
              ></div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Results Table -->
    <div v-if="results.length > 0" class="mt-8">
      <div class="bg-white border border-slate-200 rounded-lg overflow-hidden shadow-sm">
        <div class="overflow-x-auto">
          <table class="w-full">
            <thead>
              <tr class="border-b border-slate-200 bg-slate-50">
                <th class="px-6 py-3 text-left text-sm font-semibold text-slate-700">IP {{ t('tools.frameworkPassword.address') }}</th>
                <th class="px-6 py-3 text-left text-sm font-semibold text-slate-700">{{ t('tools.frameworkPassword.status') }}</th>
                <th class="px-6 py-3 text-left text-sm font-semibold text-slate-700">{{ t('tools.frameworkPassword.message') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="result in results" :key="result.ip" class="border-b border-slate-200 hover:bg-slate-50 transition-colors">
                <td class="px-6 py-3 text-sm font-mono text-slate-900">{{ result.ip }}</td>
                <td class="px-6 py-3">
                  <div class="flex items-center gap-2">
                    <component
                      :is="result.success ? CheckCircle2 : AlertCircle"
                      :class="result.success ? 'text-green-500' : 'text-red-500'"
                      class="w-5 h-5"
                    />
                    <span :class="result.success ? 'text-green-600 font-semibold' : 'text-red-600 font-semibold'" class="text-sm">
                      {{ result.success ? t('tools.frameworkPassword.success') : t('tools.frameworkPassword.failed') }}
                    </span>
                  </div>
                </td>
                <td class="px-6 py-3 text-sm text-slate-600">{{ result.message }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
    </div>
  </div>
</template>
