<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { Save, Plus, Trash2, FolderOpen, Globe } from 'lucide-vue-next';
import { getConfig, saveConfig, type AppConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';

const { t, locale } = useI18n();
const config = ref<AppConfig>({
  remote_paths: [],
  target_versions: [],
  local_path: '',
  interval_minutes: 10
});

const newRemotePath = ref('');
const newVersion = ref('');
const statusMsg = ref('');

async function load() {
  try {
    config.value = await getConfig();
  } catch (e) {
    console.error(e);
  }
}

async function save() {
  try {
    await saveConfig(config.value);
    statusMsg.value = t('settings.saved');
    setTimeout(() => statusMsg.value = '', 3000);
  } catch (e) {
    statusMsg.value = t('settings.saveError', { error: e });
  }
}

function addPath() {
  if (newRemotePath.value && !config.value.remote_paths.includes(newRemotePath.value)) {
    config.value.remote_paths.push(newRemotePath.value);
    newRemotePath.value = '';
    save(); // Auto save
  }
}

function removePath(index: number) {
  config.value.remote_paths.splice(index, 1);
  save(); // Auto save
}

function addVersion() {
  if (newVersion.value && !config.value.target_versions.includes(newVersion.value)) {
    config.value.target_versions.push(newVersion.value);
    newVersion.value = '';
    save(); // Auto save
  }
}

function removeVersion(index: number) {
  config.value.target_versions.splice(index, 1);
  save(); // Auto save
}

function changeLanguage(lang: string) {
  locale.value = lang;
  localStorage.setItem('locale', lang);
}

onMounted(load);
</script>

<template>
  <div class="p-6 max-w-4xl mx-auto space-y-8">
    <div class="flex justify-between items-center">
      <h2 class="text-2xl font-bold text-slate-800">{{ t('settings.title') }}</h2>
      <button 
        @click="save"
        class="bg-blue-600 hover:bg-blue-700 text-white px-6 py-2 rounded-lg font-medium flex items-center gap-2 transition-colors shadow-sm"
      >
        <Save class="w-4 h-4" />
        {{ t('settings.save') }}
      </button>
    </div>

    <div v-if="statusMsg" class="bg-green-100 text-green-700 p-3 rounded-lg text-sm font-medium">
      {{ statusMsg }}
    </div>

    <!-- Language Settings -->
    <div class="bg-white p-6 rounded-xl border border-slate-200 shadow-sm space-y-4">
      <h3 class="text-lg font-semibold text-slate-700 flex items-center gap-2">
        <Globe class="w-5 h-5" />
        {{ t('settings.language') }}
      </h3>
      <div class="flex gap-4">
        <button 
          @click="changeLanguage('zh')" 
          class="px-4 py-2 rounded-lg border transition-colors"
          :class="locale === 'zh' ? 'bg-blue-50 border-blue-500 text-blue-700 font-medium' : 'border-slate-300 text-slate-600 hover:bg-slate-50'"
        >
          中文
        </button>
        <button 
          @click="changeLanguage('en')" 
          class="px-4 py-2 rounded-lg border transition-colors"
          :class="locale === 'en' ? 'bg-blue-50 border-blue-500 text-blue-700 font-medium' : 'border-slate-300 text-slate-600 hover:bg-slate-50'"
        >
          English
        </button>
      </div>
    </div>

    <!-- Local Path -->
    <div class="bg-white p-6 rounded-xl border border-slate-200 shadow-sm space-y-4">
      <h3 class="text-lg font-semibold text-slate-700 flex items-center gap-2">
        <FolderOpen class="w-5 h-5" />
        {{ t('settings.localStorage') }}
      </h3>
      <div>
        <label class="block text-sm font-medium text-slate-600 mb-1">{{ t('settings.localPath') }}</label>
        <input 
          v-model="config.local_path"
          type="text"
          class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all"
        />
        <p class="text-xs text-slate-400 mt-1">{{ t('settings.localPathDesc') }}</p>
      </div>
    </div>

    <!-- Interval -->
    <div class="bg-white p-6 rounded-xl border border-slate-200 shadow-sm space-y-4">
      <h3 class="text-lg font-semibold text-slate-700">{{ t('settings.scanInterval') }}</h3>
      <div class="flex items-center gap-4">
        <input 
          v-model.number="config.interval_minutes"
          type="number"
          min="1"
          class="w-24 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none"
        />
        <span class="text-slate-600">{{ t('settings.minutes') }}</span>
      </div>
    </div>

    <!-- Remote Paths -->
    <div class="bg-white p-6 rounded-xl border border-slate-200 shadow-sm space-y-4">
      <h3 class="text-lg font-semibold text-slate-700">{{ t('settings.remotePaths') }}</h3>
      <div class="flex gap-2">
        <input 
          v-model="newRemotePath"
          @keyup.enter="addPath"
          :placeholder="t('settings.remotePathPlaceholder')"
          class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none"
        />
        <button @click="addPath" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600">
          <Plus class="w-5 h-5" />
        </button>
      </div>
      <ul class="space-y-2 max-h-48 overflow-y-auto">
        <li v-for="(path, i) in config.remote_paths" :key="i" class="flex justify-between items-center bg-slate-50 p-3 rounded-lg text-sm border border-slate-100">
          <span class="text-slate-700 font-mono break-all">{{ path }}</span>
          <button @click="removePath(i)" class="text-red-400 hover:text-red-600 p-1">
            <Trash2 class="w-4 h-4" />
          </button>
        </li>
        <li v-if="config.remote_paths.length === 0" class="text-slate-400 text-sm text-center py-4">{{ t('settings.noRemotePaths') }}</li>
      </ul>
    </div>

    <!-- Versions -->
    <div class="bg-white p-6 rounded-xl border border-slate-200 shadow-sm space-y-4">
      <h3 class="text-lg font-semibold text-slate-700">{{ t('settings.targetVersions') }}</h3>
      <div class="flex gap-2">
        <input 
          v-model="newVersion"
          @keyup.enter="addVersion"
          :placeholder="t('settings.versionPlaceholder')"
          class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none"
        />
        <button @click="addVersion" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600">
          <Plus class="w-5 h-5" />
        </button>
      </div>
      <div class="flex flex-wrap gap-2">
        <div v-for="(ver, i) in config.target_versions" :key="i" class="bg-blue-50 text-blue-700 px-3 py-1 rounded-full text-sm font-medium border border-blue-100 flex items-center gap-2">
          {{ ver }}
          <button @click="removeVersion(i)" class="hover:text-blue-900">
            <Trash2 class="w-3 h-3" />
          </button>
        </div>
        <div v-if="config.target_versions.length === 0" class="text-slate-400 text-sm w-full text-center py-4">{{ t('settings.noVersions') }}</div>
      </div>
    </div>
  </div>
</template>
