<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import { Save, Plus, Trash2, FolderOpen, Globe, Server, Terminal, Clock, UploadCloud } from 'lucide-vue-next';
import { getConfig, saveConfig, testSshConnection, addSystemEvent, manualDeploy, type AppConfig } from '@/lib/tauri';
import { useI18n } from 'vue-i18n';

const { t, locale } = useI18n();
const config = ref<AppConfig>({
  remote_paths: [],
  target_versions: [],
  local_path: '',
  interval_minutes: 10,
  time_ranges: [],
  file_extensions: [],
  filename_includes: [],
  deploy_enabled: false,
  servers: [],
  ssh_host: '',
  ssh_port: 22,
  ssh_user: '',
  ssh_password: '',
  remote_linux_path: '',
  post_commands: []
});

const newRemotePath = ref('');
const newVersion = ref('');
const newExt = ref('');
const newInclude = ref('');
const newCommand = ref('');
const newTimeRange = ref(''); // "05:00-09:00"
const statusMsg = ref('');

// Server Management
const isEditingServer = ref(false);
const editingServerIndex = ref(-1);
const serverForm = ref({
    id: '',
    enabled: true,
    name: '',
    host: '',
    port: 22,
    user: '',
    password: '',
    remote_path: ''
});

function resetServerForm() {
    serverForm.value = {
        id: crypto.randomUUID(),
        enabled: true,
        name: '',
        host: '',
        port: 22,
        user: '',
        password: '',
        remote_path: ''
    };
    isEditingServer.value = false;
    editingServerIndex.value = -1;
}

function addServer() {
    resetServerForm();
    isEditingServer.value = true;
}

function editServer(index: number) {
    editingServerIndex.value = index;
    serverForm.value = { ...config.value.servers[index] };
    isEditingServer.value = true;
}

function saveServer() {
    if (editingServerIndex.value > -1) {
        config.value.servers[editingServerIndex.value] = { ...serverForm.value };
    } else {
        config.value.servers.push({ ...serverForm.value });
    }
    save();
    isEditingServer.value = false;
}

function removeServer(index: number) {
    if (confirm('Delete this server configuration?')) {
        config.value.servers.splice(index, 1);
        save();
    }
}

async function testServerConnection(index: number) {
    const server = config.value.servers[index];
    try {
        const res = await testSshConnection(server);
        alert(res);
    } catch (e) {
        alert(`Connection failed: ${e}`);
    }
}

async function testAllServers() {
    const results: string[] = [];
    statusMsg.value = 'Testing connections...';
    
    for (const server of config.value.servers) {
        if (!server.enabled) continue;
        try {
            await testSshConnection(server);
            results.push(`✅ ${server.name || server.host}: OK`);
        } catch (e) {
            results.push(`❌ ${server.name || server.host}: Failed (${e})`);
        }
    }
    alert(results.join('\n'));
    statusMsg.value = '';
}

// Manual Deploy
const manualLocalPath = ref('');
const manualRemotePath = ref('');
const selectedServerId = ref('');
const isDeploying = ref(false);
const deployMsg = ref('');

async function handleManualDeploy() {
    if (!manualLocalPath.value || !manualRemotePath.value || !selectedServerId.value) return;
    
    // Support "all" servers
    let targets = [];
    if (selectedServerId.value === 'all') {
        targets = config.value.servers.filter(s => s.enabled);
    } else {
        const server = config.value.servers.find(s => s.id === selectedServerId.value);
        if (server) targets.push(server);
    }
    
    if (targets.length === 0) return;

    isDeploying.value = true;
    deployMsg.value = '';
    
    try {
        // Run sequentially or parallel? Parallel is better.
        // But manualDeploy is one-shot.
        // We can loop here.
        let successCount = 0;
        let failCount = 0;
        
        for (const server of targets) {
             try {
                 await manualDeploy(server, config.value.post_commands, manualLocalPath.value, manualRemotePath.value);
                 successCount++;
             } catch (e) {
                 failCount++;
                 console.error(`Deploy to ${server.name} failed:`, e);
             }
        }
        
        if (failCount === 0) {
            deployMsg.value = `Deployment successful to ${successCount} servers!`;
            addSystemEvent('MANUAL_DEPLOY', `Deployed to ${successCount} servers successfully.`);
        } else {
            deployMsg.value = `Deployment finished. Success: ${successCount}, Failed: ${failCount}. Check console for details.`;
        }
        
    } catch (e) {
        deployMsg.value = `Deployment error: ${e}`;
    } finally {
        isDeploying.value = false;
    }
}

function addCommand() {
  if (newCommand.value) {
    config.value.post_commands.push(newCommand.value);
    newCommand.value = '';
    save();
  }
}

function removeCommand(index: number) {
  config.value.post_commands.splice(index, 1);
  save();
}

function addTimeRange() {
    // Basic validation regex for HH:MM-HH:MM
    const rangeRegex = /^([0-1]?[0-9]|2[0-3]):[0-5][0-9]-([0-1]?[0-9]|2[0-3]):[0-5][0-9]$/;
    if (newTimeRange.value && rangeRegex.test(newTimeRange.value) && !config.value.time_ranges.includes(newTimeRange.value)) {
        config.value.time_ranges.push(newTimeRange.value);
        newTimeRange.value = '';
        save();
    }
}

function removeTimeRange(index: number) {
    config.value.time_ranges.splice(index, 1);
    save();
}

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
    addSystemEvent('CONFIG_CHANGE', t('settings.saved'));
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

function addExt() {
  if (newExt.value && !config.value.file_extensions.includes(newExt.value)) {
    config.value.file_extensions.push(newExt.value);
    newExt.value = '';
    save();
  }
}

function removeExt(index: number) {
  config.value.file_extensions.splice(index, 1);
  save();
}

function addInclude() {
  if (newInclude.value && !config.value.filename_includes.includes(newInclude.value)) {
    config.value.filename_includes.push(newInclude.value);
    newInclude.value = '';
    save();
  }
}

function removeInclude(index: number) {
  config.value.filename_includes.splice(index, 1);
  save();
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
          min="5"
          class="w-24 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none"
        />
        <span class="text-slate-600">{{ t('settings.minutes') }}</span>
        <span class="text-xs text-amber-500 ml-2">{{ t('settings.minInterval') }}</span>
      </div>

      <!-- Time Ranges -->
      <div class="pt-4 border-t border-slate-100 space-y-3">
          <h4 class="text-md font-medium text-slate-700 flex items-center gap-2">
              <Clock class="w-4 h-4" />
              {{ t('settings.timeRanges') }}
          </h4>
          <p class="text-xs text-slate-400">{{ t('settings.timeRangesDesc') }}</p>
          
          <div class="flex gap-2">
            <input 
              v-model="newTimeRange"
              @keyup.enter="addTimeRange"
              placeholder="09:00-18:00"
              class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none"
            />
            <button @click="addTimeRange" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600">
              <Plus class="w-5 h-5" />
            </button>
          </div>
          <div class="flex flex-wrap gap-2">
            <div v-for="(range, i) in config.time_ranges" :key="i" class="bg-amber-50 text-amber-700 px-3 py-1 rounded-full text-sm font-medium border border-amber-100 flex items-center gap-2">
              {{ range }}
              <button @click="removeTimeRange(i)" class="hover:text-amber-900">
                <Trash2 class="w-3 h-3" />
              </button>
            </div>
          </div>
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

    <!-- File Filters -->
    <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
      <!-- File Extensions -->
      <div class="bg-white p-6 rounded-xl border border-slate-200 shadow-sm space-y-4">
        <h3 class="text-lg font-semibold text-slate-700">{{ t('settings.fileExtensions') }}</h3>
        <p class="text-xs text-slate-400">{{ t('settings.fileExtensionsDesc') }}</p>
        <div class="flex gap-2">
          <input 
            v-model="newExt"
            @keyup.enter="addExt"
            placeholder="exe"
            class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none"
          />
          <button @click="addExt" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600">
            <Plus class="w-5 h-5" />
          </button>
        </div>
        <div class="flex flex-wrap gap-2">
          <div v-for="(ext, i) in config.file_extensions" :key="i" class="bg-indigo-50 text-indigo-700 px-3 py-1 rounded-full text-sm font-medium border border-indigo-100 flex items-center gap-2">
            {{ ext }}
            <button @click="removeExt(i)" class="hover:text-indigo-900">
              <Trash2 class="w-3 h-3" />
            </button>
          </div>
        </div>
      </div>

      <!-- Filename Includes -->
      <div class="bg-white p-6 rounded-xl border border-slate-200 shadow-sm space-y-4">
        <h3 class="text-lg font-semibold text-slate-700">{{ t('settings.filenameKeywords') }}</h3>
        <p class="text-xs text-slate-400">{{ t('settings.filenameKeywordsDesc') }}</p>
        <div class="flex gap-2">
          <input 
            v-model="newInclude"
            @keyup.enter="addInclude"
            placeholder="UMS"
            class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none"
          />
          <button @click="addInclude" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600">
            <Plus class="w-5 h-5" />
          </button>
        </div>
        <div class="flex flex-wrap gap-2">
          <div v-for="(inc, i) in config.filename_includes" :key="i" class="bg-purple-50 text-purple-700 px-3 py-1 rounded-full text-sm font-medium border border-purple-100 flex items-center gap-2">
            {{ inc }}
            <button @click="removeInclude(i)" class="hover:text-purple-900">
              <Trash2 class="w-3 h-3" />
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Deploy Settings -->
    <div class="bg-white p-6 rounded-xl border border-slate-200 shadow-sm space-y-6">
      <div class="flex items-center justify-between">
         <h3 class="text-lg font-semibold text-slate-700 flex items-center gap-2">
           <Server class="w-5 h-5" />
           {{ t('settings.remoteDeployment') }}
         </h3>
         <div class="flex items-center gap-2">
             <label class="relative inline-flex items-center cursor-pointer">
               <input type="checkbox" v-model="config.deploy_enabled" class="sr-only peer">
               <div class="w-11 h-6 bg-slate-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
               <span class="ml-3 text-sm font-medium text-slate-700">{{ t('settings.enable') }}</span>
             </label>
         </div>
      </div>

      <div v-if="config.deploy_enabled" class="space-y-6">
          <!-- Server List -->
          <div>
              <div class="flex justify-between items-center mb-3">
                  <h4 class="font-medium text-slate-700">{{ t('settings.servers') }}</h4>
                  <div class="flex gap-2">
                      <button @click="testAllServers" class="text-xs text-slate-600 hover:text-slate-800 flex items-center gap-1 font-medium bg-slate-100 hover:bg-slate-200 px-3 py-1.5 rounded-lg transition-colors" v-if="config.servers.length > 0">
                           <Server class="w-3 h-3" /> {{ t('settings.testAll') }}
                      </button>
                      <button @click="addServer" class="text-xs text-blue-600 hover:text-blue-800 flex items-center gap-1 font-medium bg-blue-50 hover:bg-blue-100 px-3 py-1.5 rounded-lg transition-colors">
                          <Plus class="w-3 h-3" /> {{ t('settings.addServer') }}
                      </button>
                  </div>
              </div>
              
              <div v-if="config.servers.length === 0" class="text-center p-6 bg-slate-50 rounded-lg border border-dashed border-slate-300 text-slate-500 text-sm">
                  {{ t('settings.noServers') }}
              </div>
              
              <div v-else class="space-y-3">
                  <div v-for="(server, idx) in config.servers" :key="server.id" class="border border-slate-200 rounded-lg p-3 bg-white hover:shadow-sm transition-shadow flex items-center justify-between">
                      <div class="flex items-center gap-3 overflow-hidden">
                          <input type="checkbox" v-model="server.enabled" @change="save" class="rounded text-blue-600 focus:ring-blue-500 w-4 h-4 cursor-pointer">
                          <div class="truncate">
                              <div class="font-medium text-slate-800 flex items-center gap-2">
                                  {{ server.name || server.host }}
                                  <span v-if="!server.enabled" class="text-xs bg-slate-100 text-slate-500 px-1.5 py-0.5 rounded">Disabled</span>
                              </div>
                              <div class="text-xs text-slate-500 font-mono truncate">{{ server.user }}@{{ server.host }}:{{ server.port }} <span class="text-slate-300">|</span> {{ server.remote_path }}</div>
                          </div>
                      </div>
                      <div class="flex items-center gap-1 shrink-0">
                          <button @click="testServerConnection(idx)" class="p-1.5 text-slate-500 hover:text-blue-600 hover:bg-blue-50 rounded transition-colors" :title="t('settings.testConnection')">
                              <Server class="w-4 h-4" />
                          </button>
                          <button @click="editServer(idx)" class="p-1.5 text-slate-500 hover:text-amber-600 hover:bg-amber-50 rounded transition-colors" :title="t('settings.edit')">
                              <span class="text-xs font-bold">{{ t('settings.edit') }}</span>
                          </button>
                          <button @click="removeServer(idx)" class="p-1.5 text-slate-500 hover:text-red-600 hover:bg-red-50 rounded transition-colors" title="Delete">
                              <Trash2 class="w-4 h-4" />
                          </button>
                      </div>
                  </div>
              </div>
          </div>

          <!-- Server Edit Modal -->
          <div v-if="isEditingServer" class="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
              <div class="bg-white rounded-xl p-6 w-full max-w-lg shadow-2xl transform transition-all">
                  <h3 class="text-lg font-bold mb-6 text-slate-800">{{ editingServerIndex > -1 ? t('settings.editServer') : t('settings.addServer') }}</h3>
                  <div class="space-y-4">
                      <div>
                          <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.nameAlias') }}</label>
                          <input v-model="serverForm.name" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" placeholder="Production Server 1" />
                      </div>
                      <div class="grid grid-cols-3 gap-4">
                          <div class="col-span-2">
                              <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.host') }}</label>
                              <input v-model="serverForm.host" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" placeholder="192.168.1.100" />
                          </div>
                          <div>
                              <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.port') }}</label>
                              <input v-model.number="serverForm.port" type="number" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
                          </div>
                      </div>
                      <div class="grid grid-cols-2 gap-4">
                          <div>
                              <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.username') }}</label>
                              <input v-model="serverForm.user" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
                          </div>
                          <div>
                              <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.password') }}</label>
                              <input v-model="serverForm.password" type="password" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
                          </div>
                      </div>
                      <div>
                          <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.remoteTargetDir') }}</label>
                          <input v-model="serverForm.remote_path" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" placeholder="/opt/deploy" />
                      </div>
                  </div>
                  <div class="flex justify-end gap-3 mt-8 pt-4 border-t border-slate-100">
                      <button @click="isEditingServer = false" class="px-4 py-2 text-slate-600 hover:bg-slate-100 rounded-lg font-medium transition-colors">{{ t('console.cancel') }}</button>
                      <button @click="saveServer" class="px-6 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 font-medium transition-colors shadow-sm">{{ t('settings.save') }}</button>
                  </div>
              </div>
          </div>

          <!-- Post Commands -->
          <div>
              <label class="block text-sm font-medium text-slate-600 mb-1 flex items-center gap-2">
                 <Terminal class="w-4 h-4" />
                 {{ t('settings.postCommands') }} <span class="text-xs font-normal text-slate-400">{{ t('settings.executedOnAll') }}</span>
              </label>
              <div class="flex gap-2 mb-2">
                <input 
                  v-model="newCommand"
                  @keyup.enter="addCommand"
                  :placeholder="t('settings.commandPlaceholder')"
                  class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none font-mono text-sm"
                />
                <button @click="addCommand" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600">
                  <Plus class="w-5 h-5" />
                </button>
              </div>
              <ul class="space-y-2 bg-slate-900 rounded-lg p-3 max-h-48 overflow-y-auto">
                <li v-for="(cmd, i) in config.post_commands" :key="i" class="flex justify-between items-center text-slate-300 font-mono text-sm">
                  <span>$ {{ cmd }}</span>
                  <button @click="removeCommand(i)" class="text-slate-500 hover:text-red-400 p-1">
                    <Trash2 class="w-3 h-3" />
                  </button>
                </li>
                <li v-if="config.post_commands.length === 0" class="text-slate-600 text-sm italic text-center">{{ t('settings.noCommands') }}</li>
              </ul>
          </div>

          <!-- Manual Deploy Tool -->
          <div class="pt-6 border-t border-slate-100 space-y-4">
              <h4 class="text-md font-medium text-slate-700 flex items-center gap-2">
                  <UploadCloud class="w-4 h-4" />
                  {{ t('settings.manualDeploy') }}
              </h4>
              <p class="text-xs text-slate-400">{{ t('settings.manualDeployDesc') }}</p>
              
              <div>
                  <label class="block text-sm font-medium text-slate-600 mb-1">{{ t('settings.targetServer') }}</label>
                  <div class="relative">
                      <select v-model="selectedServerId" class="w-full p-2 pr-8 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none bg-white appearance-none">
                          <option value="" disabled>{{ t('settings.selectServer') }}</option>
                          <option value="all">{{ t('settings.deployAll') }}</option>
                          <option v-for="s in config.servers" :key="s.id" :value="s.id">
                              {{ s.name || s.host }} ({{ s.host }})
                          </option>
                      </select>
                      <div class="absolute inset-y-0 right-0 flex items-center px-2 pointer-events-none text-slate-500">
                          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"></path></svg>
                      </div>
                  </div>
              </div>

              <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                  <div>
                      <label class="block text-sm font-medium text-slate-600 mb-1">{{ t('settings.localPath') }}</label>
                      <input v-model="manualLocalPath" type="text" placeholder="C:\Path\To\Package.zip" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
                  </div>
                  <div>
                      <label class="block text-sm font-medium text-slate-600 mb-1">{{ t('settings.remotePath') }}</label>
                      <input v-model="manualRemotePath" type="text" placeholder="/opt/app/deploy" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
                  </div>
              </div>
              
              <div class="flex items-center gap-3">
                  <button 
                    @click="handleManualDeploy"
                    class="bg-indigo-600 text-white px-4 py-2 rounded-lg hover:bg-indigo-700 transition-colors flex items-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
                    :disabled="isDeploying || !selectedServerId || !manualLocalPath || !manualRemotePath"
                  >
                    <UploadCloud class="w-4 h-4" />
                    {{ isDeploying ? t('settings.deploying') : t('settings.deployNow') }}
                  </button>
                  <span v-if="deployMsg" :class="deployMsg.includes('successful') ? 'text-green-600' : 'text-red-500'" class="text-sm font-medium">
                      {{ deployMsg }}
                  </span>
              </div>
          </div>
      </div>
    </div>
  </div>
</template>
