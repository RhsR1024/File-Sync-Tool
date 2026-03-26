<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { AlertCircle, CheckCircle2, Loader, Terminal, Shield, ChevronDown, ChevronUp, Server, Globe } from 'lucide-vue-next';
import { enableApplianceSsh, getConfig, type AppConfig, type ApplianceSshResult } from '../lib/tauri';

defineOptions({
  name: 'EnableApplianceSshPage',
});

const { t } = useI18n();

const config = ref<AppConfig | null>(null);
const selectedIps = ref<string[]>([]);
const manualIp = ref<string>('');
const sshUsername = ref<string>('root');
const sshPassword = ref<string>('admin_123');
const addWhitelistRule = ref<boolean>(true);
const isLoading = ref<boolean>(false);
const results = ref<ApplianceSshResult[]>([]);
const currentProgress = ref<{ current: number; total: number } | null>(null);
const showInfoDetail = ref<boolean>(false);

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

const allSelectedIps = computed(() => {
  const ips = new Set<string>([...selectedIps.value]);
  if (manualIp.value.trim()) {
    ips.add(manualIp.value.trim());
  }
  return Array.from(ips);
});

const needsSshCredentials = computed(() => addWhitelistRule.value);

const hasWhitelistConfigError = computed(() => {
  return needsSshCredentials.value && (!sshUsername.value.trim() || !sshPassword.value);
});

const isFormValid = computed(() => {
  if (isLoading.value || allSelectedIps.value.length === 0) {
    return false;
  }

  if (needsSshCredentials.value) {
    return !!sshUsername.value.trim() && !!sshPassword.value;
  }

  return true;
});

onMounted(async () => {
  try {
    config.value = await getConfig();
  } catch (e) {
    console.error('Failed to load config:', e);
  }
});

const handleExecute = async () => {
  if (allSelectedIps.value.length === 0) {
    alert(t('tools.applianceSsh.noIps'));
    return;
  }

  if (hasWhitelistConfigError.value) {
    alert(t('tools.applianceSsh.sshCredentialsRequired'));
    return;
  }

  isLoading.value = true;
  results.value = [];

  try {
    const ipList = allSelectedIps.value;
    currentProgress.value = { current: 0, total: ipList.length };

    const response = await enableApplianceSsh({
      ips: ipList,
      sshUsername: sshUsername.value.trim(),
      sshPassword: sshPassword.value,
      addWhitelistRule: addWhitelistRule.value,
    });
    results.value = response;
    currentProgress.value = { current: ipList.length, total: ipList.length };
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    results.value = allSelectedIps.value.map(ip => ({
      ip,
      success: false,
      message: `Error: ${errorMessage}`,
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

const formatEnableState = (value?: number) => {
  if (value === 1) {
    return t('tools.applianceSsh.stateEnabled');
  }
  if (value === 2) {
    return t('tools.applianceSsh.stateDisabled');
  }
  return t('tools.applianceSsh.stateUnknown');
};

const enableStateClass = (value?: number) => {
  if (value === 1) return 'bg-emerald-50 text-emerald-700 border-emerald-200';
  if (value === 2) return 'bg-amber-50 text-amber-700 border-amber-200';
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
              <input
                v-model="manualIp"
                type="text"
                :placeholder="t('tools.applianceSsh.manualIpPlaceholder')"
                :disabled="isLoading"
                class="w-full px-3 py-2 text-sm border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
              />
              <p class="text-xs text-slate-400 mt-1.5">{{ t('tools.applianceSsh.manualIpHint') }}</p>
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

            <!-- SSH credentials - conditionally shown -->
            <Transition
              enter-active-class="transition-all duration-200 ease-out"
              enter-from-class="opacity-0 -translate-y-2 max-h-0"
              enter-to-class="opacity-100 translate-y-0 max-h-60"
              leave-active-class="transition-all duration-150 ease-in"
              leave-from-class="opacity-100 translate-y-0 max-h-60"
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
                      type="password"
                      :placeholder="t('tools.applianceSsh.sshPasswordPlaceholder')"
                      :disabled="isLoading"
                      class="w-full px-3 py-2 text-sm border border-slate-200 rounded-lg focus:outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-400/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400 transition-colors"
                    />
                  </div>
                </div>

                <p class="mt-3 text-xs text-slate-400 leading-relaxed">{{ t('tools.applianceSsh.whitelistHint') }}</p>
                <p v-if="hasWhitelistConfigError" class="mt-1.5 text-xs text-red-500 font-medium">{{ t('tools.applianceSsh.sshCredentialsRequired') }}</p>
              </div>
            </Transition>
          </div>

          <!-- Selected IPs summary -->
          <div v-if="allSelectedIps.length > 0" class="bg-slate-50 border border-slate-200/80 rounded-xl px-4 py-3">
            <p class="text-xs font-medium text-slate-500 mb-2">{{ t('tools.applianceSsh.selectedIps', { count: allSelectedIps.length }) }}</p>
            <div class="flex flex-wrap gap-1.5">
              <span v-for="ip in allSelectedIps" :key="ip" class="inline-flex items-center bg-blue-100/80 text-blue-800 px-2.5 py-0.5 rounded-md text-xs font-mono">
                {{ ip }}
              </span>
            </div>
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
          <div v-if="results.length === 0 && !currentProgress" class="mt-4 pt-3 border-t border-slate-100">
            <p class="text-xs text-slate-400 text-center py-2">{{ t('tools.applianceSsh.noIps') }}</p>
          </div>
        </div>
      </div>

      <!-- Results table -->
      <div v-if="results.length > 0">
        <div class="bg-white border border-slate-200/80 rounded-xl overflow-hidden shadow-sm">
          <div class="overflow-x-auto">
            <table class="w-full">
              <thead>
                <tr class="border-b border-slate-100 bg-slate-50/80">
                  <th class="px-5 py-2.5 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide w-44">IP {{ t('tools.applianceSsh.address') }}</th>
                  <th class="px-5 py-2.5 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide w-24">{{ t('tools.applianceSsh.status') }}</th>
                  <th class="px-5 py-2.5 text-left text-xs font-semibold text-slate-500 uppercase tracking-wide">{{ t('tools.applianceSsh.message') }}</th>
                </tr>
              </thead>
              <tbody class="divide-y divide-slate-100">
                <tr v-for="result in results" :key="result.ip" class="hover:bg-slate-50/60 transition-colors">
                  <td class="px-5 py-3 text-sm font-mono text-slate-800">{{ result.ip }}</td>
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
