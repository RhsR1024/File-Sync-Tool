<script setup lang="ts">
import { ref, computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { AlertCircle, CheckCircle2, Loader } from 'lucide-vue-next';
import { changeFrameworkPassword } from '../lib/tauri';
import type { FrameworkPasswordResult } from '../lib/tauri';

const { t } = useI18n();

const ipInput = ref<string>('');
const isLoading = ref<boolean>(false);
const results = ref<FrameworkPasswordResult[]>([]);
const currentProgress = ref<{ current: number; total: number } | null>(null);

const OLD_PASSWORD_HASH = '8d969eef6ecad3c29a3a629280e686cf0c3f5d5a86aff3ca12020c923adc6c92';
const NEW_PASSWORD_HASH = '4d5c5f61bb3d2c299d3211c2992a28a7849b6ce933919c399ce24903c1715d45';

const ips = computed(() => {
  return ipInput.value
    .split(/[\n,]/)
    .map(ip => ip.trim())
    .filter(ip => ip.length > 0);
});

const isFormValid = computed(() => {
  return ips.value.length > 0 && !isLoading.value;
});

const handleExecute = async () => {
  if (ips.value.length === 0) {
    alert(t('tools.frameworkPassword.noIps'));
    return;
  }

  isLoading.value = true;
  results.value = [];

  try {
    const ipList = ips.value;
    currentProgress.value = { current: 0, total: ipList.length };

    const response = await changeFrameworkPassword(ipList);
    results.value = response;
    currentProgress.value = { current: ipList.length, total: ipList.length };
  } catch (error) {
    const errorMessage = error instanceof Error ? error.message : String(error);
    results.value = ips.value.map(ip => ({
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

const successCount = computed(() => results.value.filter(r => r.success).length);
const failureCount = computed(() => results.value.filter(r => !r.success).length);
</script>

<template>
  <div class="flex-1 flex flex-col bg-gradient-to-br from-slate-900 via-slate-800 to-slate-900 p-8">
    <!-- Header -->
    <div class="mb-8">
      <h1 class="text-3xl font-bold text-white mb-2">{{ t('tools.frameworkPassword.title') }}</h1>
      <p class="text-slate-400">{{ t('tools.frameworkPassword.description', 'Modify the default password of the framework') }}</p>
    </div>

    <!-- Main Content -->
    <div class="grid grid-cols-1 lg:grid-cols-3 gap-8">
      <!-- Input Section -->
      <div class="lg:col-span-2 space-y-6">
        <!-- IP Input Card -->
        <div class="bg-slate-800/50 border border-slate-700 rounded-lg p-6 backdrop-blur-sm hover:border-slate-600 transition-colors">
          <label class="block text-sm font-semibold text-white mb-3">
            {{ t('tools.frameworkPassword.ipLabel') }}
          </label>
          <textarea
            v-model="ipInput"
            :placeholder="t('tools.frameworkPassword.ipPlaceholder')"
            :disabled="isLoading"
            class="w-full h-32 bg-slate-900/50 border border-slate-600 rounded px-4 py-2 text-white placeholder-slate-500 focus:outline-none focus:border-blue-500 focus:ring-1 focus:ring-blue-500/20 disabled:opacity-50 disabled:cursor-not-allowed font-mono"
          />
          <div class="mt-2 text-xs text-slate-400">
            {{ ips.length }} IP {{ ips.length === 1 ? 'address' : 'addresses' }}
          </div>
        </div>

        <!-- Password Info Cards -->
        <div class="grid grid-cols-2 gap-4">
          <div class="bg-slate-800/50 border border-slate-700 rounded-lg p-4 backdrop-blur-sm">
            <label class="block text-xs font-semibold text-white mb-2">
              {{ t('tools.frameworkPassword.oldPasswordLabel') }}
            </label>
            <div class="bg-slate-900/50 border border-slate-600 rounded px-3 py-2 font-mono text-xs text-slate-300 break-all">
              {{ OLD_PASSWORD_HASH }}
            </div>
          </div>
          <div class="bg-slate-800/50 border border-slate-700 rounded-lg p-4 backdrop-blur-sm">
            <label class="block text-xs font-semibold text-white mb-2">
              {{ t('tools.frameworkPassword.newPasswordLabel') }}
            </label>
            <div class="bg-slate-900/50 border border-slate-600 rounded px-3 py-2 font-mono text-xs text-slate-300 break-all">
              {{ NEW_PASSWORD_HASH }}
            </div>
          </div>
        </div>

        <!-- Execute Button -->
        <button
          @click="handleExecute"
          :disabled="!isFormValid"
          class="w-full px-6 py-3 bg-gradient-to-r from-blue-600 to-cyan-600 text-white font-semibold rounded-lg hover:from-blue-700 hover:to-cyan-700 focus:outline-none focus:ring-2 focus:ring-blue-500/50 disabled:opacity-50 disabled:cursor-not-allowed transition-all duration-200 flex items-center justify-center gap-2"
        >
          <Loader v-if="isLoading" class="w-5 h-5 animate-spin" />
          <span>{{ isLoading ? 'Processing...' : t('tools.frameworkPassword.executeButton') }}</span>
        </button>
      </div>

      <!-- Stats Card -->
      <div class="bg-slate-800/50 border border-slate-700 rounded-lg p-6 backdrop-blur-sm h-fit sticky top-8">
        <h3 class="text-sm font-semibold text-white mb-4">{{ t('tools.frameworkPassword.results') }}</h3>

        <div class="space-y-3">
          <div class="flex items-center justify-between text-sm">
            <span class="text-slate-400">Total:</span>
            <span class="text-white font-semibold">{{ results.length }}</span>
          </div>
          <div class="flex items-center justify-between text-sm">
            <span class="text-slate-400">Success:</span>
            <span class="text-green-400 font-semibold">{{ successCount }}</span>
          </div>
          <div class="flex items-center justify-between text-sm">
            <span class="text-slate-400">Failed:</span>
            <span class="text-red-400 font-semibold">{{ failureCount }}</span>
          </div>

          <div v-if="currentProgress" class="mt-6 pt-4 border-t border-slate-700">
            <div class="text-xs text-slate-400 mb-2">
              {{ t('tools.frameworkPassword.progress', `Processing: ${currentProgress.current}/${currentProgress.total}`) }}
            </div>
            <div class="w-full bg-slate-900/50 rounded-full h-2">
              <div
                class="bg-gradient-to-r from-blue-500 to-cyan-500 h-2 rounded-full transition-all duration-300"
                :style="{ width: `${(currentProgress.current / currentProgress.total) * 100}%` }"
              ></div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Results Table -->
    <div v-if="results.length > 0" class="mt-8">
      <div class="bg-slate-800/50 border border-slate-700 rounded-lg overflow-hidden backdrop-blur-sm">
        <div class="overflow-x-auto">
          <table class="w-full">
            <thead>
              <tr class="border-b border-slate-700 bg-slate-900/50">
                <th class="px-6 py-3 text-left text-xs font-semibold text-slate-300">IP</th>
                <th class="px-6 py-3 text-left text-xs font-semibold text-slate-300">{{ t('tools.frameworkPassword.status') }}</th>
                <th class="px-6 py-3 text-left text-xs font-semibold text-slate-300">{{ t('tools.frameworkPassword.message') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="result in results" :key="result.ip" class="border-b border-slate-700 hover:bg-slate-700/20 transition-colors">
                <td class="px-6 py-3 text-sm font-mono text-white">{{ result.ip }}</td>
                <td class="px-6 py-3">
                  <div class="flex items-center gap-2">
                    <component
                      :is="result.success ? CheckCircle2 : AlertCircle"
                      :class="result.success ? 'text-green-400' : 'text-red-400'"
                      class="w-4 h-4"
                    />
                    <span :class="result.success ? 'text-green-400' : 'text-red-400'" class="text-sm font-semibold">
                      {{ result.success ? t('tools.frameworkPassword.success') : t('tools.frameworkPassword.failed') }}
                    </span>
                  </div>
                </td>
                <td class="px-6 py-3 text-sm text-slate-300">{{ result.message }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>
  </div>
</template>
