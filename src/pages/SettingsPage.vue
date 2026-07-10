<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { Copy, Edit, FileText, FolderOpen, Globe, RefreshCw, Save, Settings2 } from 'lucide-vue-next';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { useI18n } from 'vue-i18n';

import { pushToast } from '@/composables/useToast';
import { configStore } from '@/lib/configStore';
import { getAppPaths, getCustomDataDir, openPathParent, setCustomDataDir } from '@/lib/tauri';

defineOptions({ name: 'SettingsPage' });

const { locale, t } = useI18n();
const config = computed(() => configStore.config);
const configPath = ref('');
const logPath = ref('');
const customDataDir = ref('');
const customDataDirInput = ref('');
const customDataDirSaving = ref(false);
const showDirEditor = ref(false);

async function refreshPaths() {
  const [paths, dataDir] = await Promise.all([getAppPaths(), getCustomDataDir()]);
  [configPath.value, logPath.value] = paths;
  customDataDir.value = dataDir;
  customDataDirInput.value = dataDir;
}

async function load() {
  try {
    await Promise.all([configStore.ensureLoaded(), refreshPaths()]);
  } catch (error) {
    console.error('Failed to load application settings', error);
    pushToast(String(error), 'error', { ttlMs: 5000 });
  }
}

async function save() {
  if (!config.value || configStore.isSaving) return;
  try {
    await configStore.saveApp();
    pushToast(t('settings.toast.saved'), 'success');
  } catch (error) {
    pushToast(t('settings.toast.saveError', { error: String(error) }), 'error', { ttlMs: 5000 });
  }
}

function changeLanguage(language: string) {
  locale.value = language;
  localStorage.setItem('locale', language);
}

async function copyToClipboard(text: string) {
  if (!text) return;
  try {
    await writeText(text);
    pushToast(t('settings.pathCopied'), 'success');
  } catch (error) {
    pushToast(String(error), 'error');
  }
}

async function openParentFolder(path: string) {
  if (!path) return;
  try {
    await openPathParent(path);
  } catch (error) {
    pushToast(String(error), 'error');
  }
}

async function saveCustomDataDir() {
  if (customDataDirSaving.value) return;
  customDataDirSaving.value = true;
  try {
    await setCustomDataDir(customDataDirInput.value.trim());
    await Promise.all([configStore.refresh(), refreshPaths()]);
    pushToast(t('settings.customDataDirMigrated'), 'success', { ttlMs: 4000 });
  } catch (error) {
    pushToast(String(error), 'error', { ttlMs: 4000 });
  } finally {
    customDataDirSaving.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="h-full min-h-0 overflow-y-auto overscroll-y-none bg-slate-50">
    <div v-if="config" class="min-h-full w-full max-w-4xl mx-auto space-y-6 p-6 pb-24">
      <div class="flex items-center gap-3">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-slate-900 text-white shadow-sm">
          <Settings2 class="h-5 w-5" />
        </div>
        <div>
          <h2 class="text-2xl font-bold text-slate-950">{{ t('settings.title') }}</h2>
          <p class="mt-0.5 text-sm text-slate-500">{{ t('settings.applicationSettingsDesc') }}</p>
        </div>
      </div>

      <section class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        <header class="flex items-center gap-3 border-b border-slate-200 bg-slate-50 px-6 py-4">
          <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-violet-100 text-violet-600">
            <Settings2 class="h-4 w-4" />
          </div>
          <h3 class="text-base font-semibold text-slate-700">{{ t('settings.startupOptions') }}</h3>
        </header>
        <div class="space-y-5 p-6">
          <label class="flex items-center justify-between gap-4" :title="t('settings.tooltip.launchAndAutoScan')">
            <span>
              <span class="block text-sm font-medium text-slate-700">{{ t('settings.launchAndAutoScan') }}</span>
              <span class="mt-1 block text-xs text-slate-400">{{ t('settings.launchAndAutoScanDesc') }}</span>
            </span>
            <span class="relative inline-flex shrink-0 cursor-pointer items-center">
              <input v-model="config.launch_and_auto_scan" type="checkbox" class="peer sr-only" @change="save">
              <span class="h-6 w-11 rounded-full bg-slate-200 after:absolute after:left-0.5 after:top-0.5 after:h-5 after:w-5 after:rounded-full after:border after:border-slate-300 after:bg-white after:transition-all peer-checked:bg-blue-600 peer-checked:after:translate-x-full peer-checked:after:border-white"></span>
            </span>
          </label>

          <label class="flex items-center justify-between gap-4" :title="t('settings.tooltip.launchAndAutoStartFileShare')">
            <span>
              <span class="block text-sm font-medium text-slate-700">{{ t('settings.launchAndAutoStartFileShare') }}</span>
              <span class="mt-1 block text-xs text-slate-400">{{ t('settings.launchAndAutoStartFileShareDesc') }}</span>
            </span>
            <span class="relative inline-flex shrink-0 cursor-pointer items-center">
              <input v-model="config.launch_and_auto_start_file_share" type="checkbox" class="peer sr-only" @change="save">
              <span class="h-6 w-11 rounded-full bg-slate-200 after:absolute after:left-0.5 after:top-0.5 after:h-5 after:w-5 after:rounded-full after:border after:border-slate-300 after:bg-white after:transition-all peer-checked:bg-blue-600 peer-checked:after:translate-x-full peer-checked:after:border-white"></span>
            </span>
          </label>

          <label class="flex items-center justify-between gap-4" :title="t('settings.tooltip.closeToTray')">
            <span>
              <span class="block text-sm font-medium text-slate-700">{{ t('settings.closeToTray') }}</span>
              <span class="mt-1 block text-xs text-slate-400">{{ t('settings.closeToTrayDesc') }}</span>
            </span>
            <span class="relative inline-flex shrink-0 cursor-pointer items-center">
              <input v-model="config.close_to_tray" type="checkbox" class="peer sr-only" @change="save">
              <span class="h-6 w-11 rounded-full bg-slate-200 after:absolute after:left-0.5 after:top-0.5 after:h-5 after:w-5 after:rounded-full after:border after:border-slate-300 after:bg-white after:transition-all peer-checked:bg-blue-600 peer-checked:after:translate-x-full peer-checked:after:border-white"></span>
            </span>
          </label>

          <div class="grid gap-4 border-t border-slate-100 pt-5 sm:grid-cols-2">
            <label for="settings-max-log-lines" class="space-y-2">
              <span class="block text-sm font-medium text-slate-700">{{ t('settings.maxLogLines') }}</span>
              <span class="block text-xs text-slate-400">{{ t('settings.maxLogLinesDesc') }}</span>
              <input id="settings-max-log-lines" v-model.number="config.max_log_lines" type="number" min="50" max="5000" step="50" class="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-200" @change="save">
            </label>
            <label for="settings-max-task-records" class="space-y-2">
              <span class="block text-sm font-medium text-slate-700">{{ t('settings.maxTaskRecords') }}</span>
              <span class="block text-xs text-slate-400">{{ t('settings.maxTaskRecordsDesc') }}</span>
              <input id="settings-max-task-records" v-model.number="config.max_task_records" type="number" min="10" max="500" step="10" class="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm focus:border-blue-500 focus:ring-2 focus:ring-blue-200" @change="save">
            </label>
          </div>
        </div>
      </section>

      <section class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        <header class="flex items-center gap-3 border-b border-slate-200 bg-slate-50 px-6 py-4">
          <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-sky-100 text-sky-600">
            <RefreshCw class="h-4 w-4" />
          </div>
          <h3 class="text-base font-semibold text-slate-700">{{ t('settings.update.section') }}</h3>
        </header>
        <div class="space-y-5 p-6">
          <label class="flex items-center justify-between gap-4" :title="t('settings.tooltip.notifyOnNewVersion')">
            <span>
              <span class="block text-sm font-medium text-slate-700">{{ t('settings.update.notifyToggle') }}</span>
              <span class="mt-1 block text-xs text-slate-400">{{ t('settings.update.notifyHelp') }}</span>
            </span>
            <span class="relative inline-flex shrink-0 cursor-pointer items-center">
              <input v-model="config.notify_on_new_version" type="checkbox" class="peer sr-only" @change="save">
              <span class="h-6 w-11 rounded-full bg-slate-200 after:absolute after:left-0.5 after:top-0.5 after:h-5 after:w-5 after:rounded-full after:border after:border-slate-300 after:bg-white after:transition-all peer-checked:bg-sky-600 peer-checked:after:translate-x-full peer-checked:after:border-white"></span>
            </span>
          </label>
          <label for="settings-update-server-url" class="block space-y-2">
            <span class="block text-sm font-medium text-slate-700">{{ t('settings.update.serverLabel') }}</span>
            <input id="settings-update-server-url" v-model.trim="config.update_server_url" type="text" :placeholder="t('settings.update.serverPlaceholder')" class="w-full rounded-xl border border-slate-300 px-3 py-2 text-sm text-slate-700 outline-none focus:border-sky-400 focus:ring-2 focus:ring-sky-200" @change="save">
            <span class="block text-xs text-slate-400">{{ t('settings.update.serverHint') }}</span>
          </label>
        </div>
      </section>

      <section class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        <header class="flex items-center gap-3 border-b border-slate-200 bg-slate-50 px-6 py-4">
          <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-blue-100 text-blue-600">
            <Globe class="h-4 w-4" />
          </div>
          <h3 class="text-base font-semibold text-slate-700">{{ t('settings.language') }}</h3>
        </header>
        <div class="flex gap-3 p-6">
          <button class="rounded-lg border px-4 py-2 text-sm transition-colors" :class="locale === 'zh' ? 'border-blue-500 bg-blue-50 font-medium text-blue-700' : 'border-slate-300 text-slate-600 hover:bg-slate-50'" @click="changeLanguage('zh')">{{ t('settings.languageChinese') }}</button>
          <button class="rounded-lg border px-4 py-2 text-sm transition-colors" :class="locale === 'en' ? 'border-blue-500 bg-blue-50 font-medium text-blue-700' : 'border-slate-300 text-slate-600 hover:bg-slate-50'" @click="changeLanguage('en')">English</button>
        </div>
      </section>

      <section class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        <header class="flex items-center gap-3 border-b border-slate-200 bg-slate-50 px-6 py-4">
          <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-slate-200 text-slate-600">
            <FileText class="h-4 w-4" />
          </div>
          <h3 class="text-base font-semibold text-slate-700">{{ t('settings.configPaths') }}</h3>
        </header>
        <div class="space-y-4 p-6">
          <div v-for="item in [{ label: t('settings.configFile'), path: configPath }, { label: t('settings.logFile'), path: logPath }]" :key="item.label">
            <span class="mb-1 block text-xs font-medium uppercase tracking-wider text-slate-500">{{ item.label }}</span>
            <div class="flex gap-2">
              <code class="min-w-0 flex-1 break-all rounded-lg border border-slate-200 bg-slate-50 p-2.5 font-mono text-xs text-slate-600">{{ item.path }}</code>
              <button class="rounded-lg p-2 text-slate-400 transition-colors hover:bg-blue-50 hover:text-blue-600" :title="t('settings.copyPath')" @click="copyToClipboard(item.path)"><Copy class="h-4 w-4" /></button>
              <button class="rounded-lg p-2 text-slate-400 transition-colors hover:bg-blue-50 hover:text-blue-600" :title="t('settings.openFolder')" @click="openParentFolder(item.path)"><FolderOpen class="h-4 w-4" /></button>
            </div>
          </div>

          <div class="flex items-center justify-between gap-3 border-t border-slate-100 pt-4">
            <span v-if="customDataDir" class="truncate text-xs text-emerald-600">{{ t('settings.customDataDirActive') }}: <code class="font-mono">{{ customDataDir }}</code></span>
            <span v-else class="text-xs text-slate-400">{{ t('settings.usingDefaultPaths') }}</span>
            <button class="flex shrink-0 items-center gap-1 rounded-lg bg-blue-50 px-2.5 py-1 text-xs font-medium text-blue-600 hover:bg-blue-100" @click="showDirEditor = !showDirEditor"><Edit class="h-3 w-3" />{{ t('settings.customizeLocation') }}</button>
          </div>
          <div v-if="showDirEditor" class="space-y-2 rounded-lg border border-slate-200 bg-slate-50 p-3">
            <label for="settings-custom-data-dir" class="block text-xs font-medium text-slate-500">{{ t('settings.customDataDir') }}</label>
            <p class="text-xs text-slate-500">{{ t('settings.customDataDirDesc') }}</p>
            <div class="flex gap-2">
              <input id="settings-custom-data-dir" v-model="customDataDirInput" class="min-w-0 flex-1 rounded-lg border border-slate-300 p-2 font-mono text-xs outline-none focus:ring-2 focus:ring-blue-500" :placeholder="t('settings.customDataDirPlaceholder')">
              <button class="rounded-lg bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-700 disabled:opacity-50" :disabled="customDataDirSaving" @click="saveCustomDataDir">{{ customDataDirSaving ? t('settings.saving') : t('settings.save') }}</button>
              <button v-if="customDataDir" class="rounded-lg border border-slate-200 px-3 py-1.5 text-xs font-medium text-slate-500 hover:bg-slate-100" @click="customDataDirInput = ''; saveCustomDataDir()">{{ t('settings.resetDefault') }}</button>
            </div>
          </div>
        </div>
      </section>

      <button class="fixed bottom-6 right-6 z-40 flex items-center gap-2 rounded-full bg-blue-600 px-5 py-3 font-medium text-white shadow-lg shadow-blue-200/70 transition hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50" :disabled="configStore.isSaving" :aria-busy="configStore.isSaving" @click="save">
        <RefreshCw v-if="configStore.isSaving" class="h-4 w-4 animate-spin" />
        <Save v-else class="h-4 w-4" />
        {{ configStore.isSaving ? t('settings.saving') : t('settings.save') }}
      </button>
    </div>
  </div>
</template>
