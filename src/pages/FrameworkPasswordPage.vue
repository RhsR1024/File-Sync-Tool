<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { AlertCircle, CheckCircle2, Loader, Plus, Trash2 } from 'lucide-vue-next';
import { changeFrameworkPassword, getConfig, type AppConfig, type FrameworkPasswordResult } from '../lib/tauri';

const { t } = useI18n();

const config = ref<AppConfig | null>(null);
const selectedIps = ref<string[]>([]);
const manualIp = ref<string>('');
const isLoading = ref<boolean>(false);
const results = ref<FrameworkPasswordResult[]>([]);
const currentProgress = ref<{ current: number; total: number } | null>(null);

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

const isFormValid = computed(() => {
  return allSelectedIps.value.length > 0 && !isLoading.value;
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
    alert(t('tools.frameworkPassword.noIps'));
    return;
  }

  isLoading.value = true;
  results.value = [];

  try {
    const ipList = allSelectedIps.value;
    currentProgress.value = { current: 0, total: ipList.length };

    const response = await changeFrameworkPassword(ipList);
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
  <div class="flex-1 flex flex-col bg-gradient-to-br from-slate-50 to-slate-100 p-8">
    <!-- Header Section -->
    <div class="mb-8">
      <h1 class="text-4xl font-bold text-slate-900 mb-2">{{ t('tools.frameworkPassword.title') }}</h1>
      <p class="text-slate-600 text-lg">{{ t('tools.frameworkPassword.description') }}</p>
    </div>

    <!-- Info Banner -->
    <div class="mb-8 bg-blue-50 border-l-4 border-blue-500 p-4 rounded-lg">
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
        <div class="bg-white border border-slate-200 rounded-lg p-6 shadow-sm">
          <div class="flex items-center gap-2 mb-4">
            <h3 class="text-lg font-semibold text-slate-900">{{ t('tools.frameworkPassword.selectServer') }}</h3>
            <span class="text-sm text-slate-500">({{ t('tools.frameworkPassword.optional') }})</span>
          </div>

          <div v-if="serverOptions.length > 0" class="space-y-2">
            <div v-for="server in serverOptions" :key="server.id" class="flex items-center gap-3">
              <input
                type="checkbox"
                :id="`server-${server.id}`"
                :checked="isServerSelected(server.host)"
                @change="toggleServerIp(server.host)"
                :disabled="isLoading"
                class="rounded border-slate-300 text-blue-600 focus:ring-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
              />
              <label :for="`server-${server.id}`" class="flex-1 text-sm text-slate-700 cursor-pointer">
                <span class="font-medium">{{ server.name }}</span>
                <span class="text-slate-500 ml-2">({{ server.host }})</span>
              </label>
            </div>
          </div>
          <div v-else class="text-sm text-slate-500">{{ t('tools.frameworkPassword.noServers') }}</div>
        </div>

        <!-- Manual IP Input Card -->
        <div class="bg-white border border-slate-200 rounded-lg p-6 shadow-sm">
          <label class="block text-lg font-semibold text-slate-900 mb-4">{{ t('tools.frameworkPassword.manualIp') }}</label>
          <div class="space-y-2">
            <input
              v-model="manualIp"
              type="text"
              :placeholder="t('tools.frameworkPassword.manualIpPlaceholder')"
              :disabled="isLoading"
              class="w-full px-4 py-2 border border-slate-300 rounded-lg focus:outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/20 disabled:bg-slate-50 disabled:cursor-not-allowed text-slate-900 placeholder-slate-400"
            />
            <p class="text-xs text-slate-500">{{ t('tools.frameworkPassword.manualIpHint') }}</p>
          </div>
        </div>

        <!-- Selected IPs Display -->
        <div v-if="allSelectedIps.length > 0" class="bg-slate-50 border border-slate-200 rounded-lg p-4">
          <p class="text-sm font-medium text-slate-700 mb-3">{{ t('tools.frameworkPassword.selectedIps', { count: allSelectedIps.length }) }}</p>
          <div class="flex flex-wrap gap-2">
            <div v-for="ip in allSelectedIps" :key="ip" class="inline-flex items-center gap-2 bg-blue-100 text-blue-900 px-3 py-1 rounded-full text-sm">
              <span class="font-mono">{{ ip }}</span>
            </div>
          </div>
        </div>

        <!-- Execute Button -->
        <button
          @click="handleExecute"
          :disabled="!isFormValid"
          class="w-full px-6 py-3 bg-gradient-to-r from-blue-600 to-blue-700 text-white font-semibold rounded-lg hover:from-blue-700 hover:to-blue-800 focus:outline-none focus:ring-2 focus:ring-blue-500/50 disabled:opacity-50 disabled:cursor-not-allowed transition-all duration-200 flex items-center justify-center gap-2 text-lg"
        >
          <Loader v-if="isLoading" class="w-5 h-5 animate-spin" />
          <span>{{ isLoading ? t('tools.frameworkPassword.processing') : t('tools.frameworkPassword.executeButton') }}</span>
        </button>
      </div>

      <!-- Stats Card -->
      <div class="bg-white border border-slate-200 rounded-lg p-6 shadow-sm h-fit sticky top-8">
        <h3 class="text-lg font-semibold text-slate-900 mb-4">{{ t('tools.frameworkPassword.results') }}</h3>

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
</template>
