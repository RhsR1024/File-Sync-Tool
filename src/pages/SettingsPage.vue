<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, watch } from 'vue';
import { Save, Plus, Trash2, FolderOpen, Globe, Server, Terminal, Clock, UploadCloud, ListChecks, Edit, XCircle, FileText, Copy, Layers, ArrowUp, ArrowDown } from 'lucide-vue-next';
import { getConfig, saveConfig, testSshConnection, addSystemEvent, manualDeploy, getAppPaths, openPathParent, type AppConfig, type ScanTask, type DeployServer, type CommandGroup, type TaskServerBinding } from '@/lib/tauri';
import { appStore } from '@/lib/store';
import { restartSchedulerInterval } from '@/lib/scheduler';
import { useI18n } from 'vue-i18n';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';

const { t, locale } = useI18n();
const configPath = ref('');
const logPath = ref('');
const config = ref<AppConfig>({
  tasks: [],
  local_path: '',
  interval_minutes: 10,
  time_ranges: [],
  file_extensions: [],
  filename_includes: [],
  deploy_enabled: false,
  servers: [],
  command_groups: [],
  stability_check_secs: 120,
  recent_file_guard_mins: 3,
  launch_and_auto_scan: false,
  close_to_tray: false,
  max_log_lines: 200,
  copy_buffer_size_kb: 4096,
  max_task_records: 100
});

const newExt = ref('');
const newInclude = ref('');
const newTimeRange = ref('');
const statusMsg = ref('');
const statusTone = ref<'success' | 'error' | 'info'>('success');
const isServerManagerOpen = ref(false);
let statusMsgTimer: ReturnType<typeof setTimeout> | null = null;

const enabledServerCount = computed(() => config.value.servers.filter(server => server.enabled).length);
const intervalError = computed(() => config.value.interval_minutes < 5 ? t('settings.minIntervalError', { min: 5 }) : '');
const stabilityCheckError = computed(() => config.value.stability_check_secs < 60 ? t('settings.minStabilityCheckError', { min: 60 }) : '');
const recentFileGuardError = computed(() => config.value.recent_file_guard_mins < 3 ? t('settings.minRecentFileGuardError', { min: 3 }) : '');
const hasConfigErrors = computed(() => Boolean(intervalError.value || stabilityCheckError.value || recentFileGuardError.value));

function serverDisplayName(server: DeployServer) {
    return server.name || server.host;
}

function openServerManager() {
    isServerManagerOpen.value = true;
}

function closeServerManager() {
    isServerManagerOpen.value = false;
}

function clearStatusMsg() {
    if (statusMsgTimer) {
        clearTimeout(statusMsgTimer);
        statusMsgTimer = null;
    }
    statusMsg.value = '';
}

function showStatusMsg(
    message: string,
    tone: 'success' | 'error' | 'info' = 'success',
    duration = 3000,
) {
    clearStatusMsg();
    statusMsg.value = message;
    statusTone.value = tone;
    if (duration > 0) {
        statusMsgTimer = setTimeout(() => {
            statusMsg.value = '';
            statusMsgTimer = null;
        }, duration);
    }
}

// ── Command Group Management ──────────────────────────────────────────────────
const isEditingCommandGroup = ref(false);
const editingCommandGroupIndex = ref(-1);
const commandGroupForm = ref<CommandGroup>({ id: '', name: '', commands: [] });
const newGroupCommand = ref('');

function resetCommandGroupForm() {
    commandGroupForm.value = { id: crypto.randomUUID(), name: '', commands: [] };
    newGroupCommand.value = '';
    editingCommandGroupIndex.value = -1;
}

function addCommandGroup() {
    resetCommandGroupForm();
    isEditingCommandGroup.value = true;
}

function editCommandGroup(index: number) {
    editingCommandGroupIndex.value = index;
    const g = config.value.command_groups[index];
    commandGroupForm.value = { ...g, commands: [...g.commands] };
    newGroupCommand.value = '';
    isEditingCommandGroup.value = true;
}

function saveCommandGroup() {
    if (!commandGroupForm.value.name.trim()) return;
    if (editingCommandGroupIndex.value > -1) {
        config.value.command_groups[editingCommandGroupIndex.value] = { ...commandGroupForm.value };
    } else {
        config.value.command_groups.push({ ...commandGroupForm.value });
    }
    save();
    isEditingCommandGroup.value = false;
}

function removeCommandGroup(index: number) {
    if (confirm(t('settings.confirmDeleteCommandGroup'))) {
        config.value.command_groups.splice(index, 1);
        save();
    }
}

function addGroupCommand() {
    if (newGroupCommand.value.trim()) {
        commandGroupForm.value.commands.push(newGroupCommand.value.trim());
        newGroupCommand.value = '';
    }
}

function removeGroupCommand(index: number) {
    commandGroupForm.value.commands.splice(index, 1);
}

function commandGroupName(id: string) {
    return config.value.command_groups.find(g => g.id === id)?.name || id.substring(0, 8);
}

// ── Task Management ───────────────────────────────────────────────────────────
const isEditingTask = ref(false);
const editingTaskIndex = ref(-1);
const taskForm = ref<ScanTask>({
    id: '',
    enabled: true,
    name: '',
    remote_path: '',
    local_path: null,
    rule: { type: 'VersionMatch', value: '' },
    server_bindings: [],
});

function resetTaskForm() {
    taskForm.value = {
        id: crypto.randomUUID(),
        enabled: true,
        name: '',
        remote_path: '',
        local_path: null,
        rule: { type: 'VersionMatch', value: '' },
        server_bindings: [],
    };
    isEditingTask.value = false;
    editingTaskIndex.value = -1;
}

function addTask() {
    resetTaskForm();
    isEditingTask.value = true;
}

function editTask(index: number) {
    editingTaskIndex.value = index;
    const task = config.value.tasks[index];
    taskForm.value = {
        ...task,
        rule: { ...task.rule },
        server_bindings: task.server_bindings.map(b => ({ ...b, command_group_ids: [...b.command_group_ids] })),
    };
    isEditingTask.value = true;
}

function saveTask() {
    // Trim rule value to remove any leading/trailing whitespace
    const trimmedTask = JSON.parse(JSON.stringify(taskForm.value));
    trimmedTask.rule.value = trimmedTask.rule.value.trim();

    if (editingTaskIndex.value > -1) {
        config.value.tasks[editingTaskIndex.value] = trimmedTask;
    } else {
        config.value.tasks.push(trimmedTask);
    }
    save();
    isEditingTask.value = false;
}

function removeTask(index: number) {
    if (confirm(t('settings.confirmDeleteTask'))) {
        config.value.tasks.splice(index, 1);
        save();
    }
}

// Server bindings management within task form
function addServerBinding() {
    taskForm.value.server_bindings.push({ server_id: '', command_group_ids: [] });
}

function removeServerBinding(index: number) {
    taskForm.value.server_bindings.splice(index, 1);
}

function toggleBindingGroup(binding: TaskServerBinding, groupId: string) {
    const idx = binding.command_group_ids.indexOf(groupId);
    if (idx > -1) {
        binding.command_group_ids.splice(idx, 1);
    } else {
        binding.command_group_ids.push(groupId);
    }
}

function bindingGroupOrder(binding: TaskServerBinding, groupId: string) {
    const idx = binding.command_group_ids.indexOf(groupId);
    return idx > -1 ? idx + 1 : null;
}

function moveBindingGroup(binding: TaskServerBinding, index: number, direction: -1 | 1) {
    const targetIndex = index + direction;
    if (targetIndex < 0 || targetIndex >= binding.command_group_ids.length) {
        return;
    }
    const [groupId] = binding.command_group_ids.splice(index, 1);
    binding.command_group_ids.splice(targetIndex, 0, groupId);
}

function removeBindingGroupById(binding: TaskServerBinding, groupId: string) {
    const idx = binding.command_group_ids.indexOf(groupId);
    if (idx > -1) {
        binding.command_group_ids.splice(idx, 1);
    }
}

// ── Server Management ─────────────────────────────────────────────────────────
const isEditingServer = ref(false);
const editingServerIndex = ref(-1);
const serverForm = ref({
    id: '',
    enabled: true,
    name: '',
    host: '',
    port: 22,
    user: 'root',
    password: 'admin_123',
    remote_path: ''
});

function resetServerForm() {
    serverForm.value = {
        id: crypto.randomUUID(),
        enabled: true,
        name: '',
        host: '',
        port: 22,
        user: 'root',
        password: 'admin_123',
        remote_path: ''
    };
    isEditingServer.value = false;
    editingServerIndex.value = -1;
}

function addServer() {
    resetServerForm();
    isServerManagerOpen.value = true;
    isEditingServer.value = true;
}

function editServer(index: number) {
    editingServerIndex.value = index;
    serverForm.value = { ...config.value.servers[index] };
    isServerManagerOpen.value = true;
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
    if (confirm(t('settings.confirmDeleteServer'))) {
        config.value.servers.splice(index, 1);
        save();
    }
}

const serverTestStatus = ref<Record<string, { state: 'idle' | 'testing' | 'ok' | 'error'; msg: string }>>({});

function getServerStatus(id: string) {
    return serverTestStatus.value[id] ?? { state: 'idle', msg: '' };
}

async function testServerConnection(index: number) {
    const server = config.value.servers[index];
    serverTestStatus.value[server.id] = { state: 'testing', msg: '' };
    try {
        const res = await testSshConnection(server);
        serverTestStatus.value[server.id] = { state: 'ok', msg: res };
    } catch (e) {
        serverTestStatus.value[server.id] = { state: 'error', msg: String(e) };
    }
}

async function testAllServers() {
    showStatusMsg(t('settings.testing'), 'info', 0);
    for (let i = 0; i < config.value.servers.length; i++) {
        const server = config.value.servers[i];
        if (!server.enabled) continue;
        await testServerConnection(i);
    }
    clearStatusMsg();
}

// ── Manual Deploy ─────────────────────────────────────────────────────────────
const manualLocalPath = ref('');
const manualRemotePath = ref('/tmp/upload');
const manualServerBindings = ref<TaskServerBinding[]>([]);
const manualDeployMsgType = ref<'success' | 'error' | ''>('');

function addManualBinding() {
    manualServerBindings.value.push({ server_id: '', command_group_ids: [] });
}

function removeManualBinding(index: number) {
    manualServerBindings.value.splice(index, 1);
}

function toggleManualBindingGroup(binding: TaskServerBinding, groupId: string) {
    const idx = binding.command_group_ids.indexOf(groupId);
    if (idx > -1) {
        binding.command_group_ids.splice(idx, 1);
    } else {
        binding.command_group_ids.push(groupId);
    }
}

function manualBindingGroupOrder(binding: TaskServerBinding, groupId: string) {
    const idx = binding.command_group_ids.indexOf(groupId);
    return idx > -1 ? idx + 1 : null;
}

function moveManualBindingGroup(binding: TaskServerBinding, index: number, direction: -1 | 1) {
    const targetIndex = index + direction;
    if (targetIndex < 0 || targetIndex >= binding.command_group_ids.length) return;
    const [groupId] = binding.command_group_ids.splice(index, 1);
    binding.command_group_ids.splice(targetIndex, 0, groupId);
}

function removeManualBindingGroupById(binding: TaskServerBinding, groupId: string) {
    const idx = binding.command_group_ids.indexOf(groupId);
    if (idx > -1) binding.command_group_ids.splice(idx, 1);
}

const canManualDeploy = computed(() => {
    return !appStore.isManualDeploying
        && manualLocalPath.value.trim()
        && manualRemotePath.value.trim()
        && manualServerBindings.value.length > 0
        && manualServerBindings.value.every(b => b.server_id);
});

async function handleManualDeploy() {
    if (!canManualDeploy.value) return;

    appStore.isManualDeploying = true;
    appStore.manualDeployMsg = '';
    manualDeployMsgType.value = '';

    try {
        let successCount = 0;
        let failCount = 0;
        let lastError = '';

        for (const binding of manualServerBindings.value) {
            const server = config.value.servers.find(s => s.id === binding.server_id);
            if (!server) continue;

            // Resolve commands from all bound command groups in order
            const postCommands: string[] = [];
            for (const gid of binding.command_group_ids) {
                const group = config.value.command_groups.find(g => g.id === gid);
                if (group) postCommands.push(...group.commands);
            }

            try {
                await manualDeploy(server, postCommands, manualLocalPath.value, manualRemotePath.value);
                successCount++;
            } catch (e) {
                failCount++;
                lastError = String(e);
                console.error(`Deploy to ${server.name} failed:`, e);
            }
        }

        if (failCount === 0) {
            appStore.manualDeployMsg = t('settings.deploySuccess', { count: successCount });
            manualDeployMsgType.value = 'success';
            addSystemEvent('MANUAL_DEPLOY', t('settings.deploySuccessEvent', { count: successCount }));
        } else {
            appStore.manualDeployMsg = t('settings.deployFinished', { success: successCount, failed: failCount, error: lastError });
            manualDeployMsgType.value = 'error';
        }
    } catch (e) {
        appStore.manualDeployMsg = t('settings.deployError', { error: String(e) });
        manualDeployMsgType.value = 'error';
    } finally {
        appStore.isManualDeploying = false;
    }
}

// ── Misc ──────────────────────────────────────────────────────────────────────
function addTimeRange() {
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

async function copyToClipboard(text: string) {
    try {
        await writeText(text);
        showStatusMsg(t('settings.pathCopied'), 'success', 2000);
    } catch (e) {
        console.error('Failed to copy', e);
    }
}

async function openParentFolder(path: string) {
    if (!path) return;
    try {
        await openPathParent(path);
    } catch (e) {
        console.error('Failed to open folder', e);
    }
}

async function load() {
    try {
        config.value = await getConfig();
        const [cfg, log] = await getAppPaths();
        configPath.value = cfg;
        logPath.value = log;
    } catch (e) {
        console.error(e);
    }
}

async function save() {
    if (hasConfigErrors.value) return;
    try {
        await saveConfig(config.value);
        if (config.value.max_log_lines > 0) appStore.maxLogLines = config.value.max_log_lines;
        if (config.value.max_task_records > 0) appStore.maxTaskRecords = config.value.max_task_records;
        showStatusMsg(t('settings.saved'));
        addSystemEvent('CONFIG_CHANGE', t('settings.saved'));
        await restartSchedulerInterval();
    } catch (e) {
        showStatusMsg(t('settings.saveError', { error: e }), 'error', 5000);
    }
}

// Watch for rule type changes and auto-fill default value for DateMatch
watch(() => taskForm.value.rule.type, (newType) => {
    if (newType === 'DateMatch' && !taskForm.value.rule.value) {
        taskForm.value.rule.value = '%y%m%d';
    }
});

onMounted(load);
onUnmounted(clearStatusMsg);
</script>

<template>
  <div class="p-6 max-w-4xl mx-auto space-y-8 pb-28">
    <div class="flex justify-between items-center">
      <h2 class="text-2xl font-bold text-slate-800">{{ t('settings.title') }}</h2>
    </div>

    <div
      v-if="statusMsg"
      class="fixed left-1/2 top-6 z-[70] w-[min(calc(100vw-2rem),28rem)] -translate-x-1/2 rounded-2xl border px-4 py-3 text-sm font-medium shadow-2xl backdrop-blur"
      :class="{
        'border-emerald-200 bg-emerald-50/95 text-emerald-700 shadow-emerald-200/70': statusTone === 'success',
        'border-rose-200 bg-rose-50/95 text-rose-700 shadow-rose-200/70': statusTone === 'error',
        'border-sky-200 bg-sky-50/95 text-sky-700 shadow-sky-200/70': statusTone === 'info',
      }"
    >
      <div class="flex items-start gap-2.5">
        <span
          class="mt-1 h-2.5 w-2.5 shrink-0 rounded-full"
          :class="{
            'bg-emerald-500': statusTone === 'success',
            'bg-rose-500': statusTone === 'error',
            'bg-sky-500': statusTone === 'info',
          }"
        ></span>
        <span class="leading-5">{{ statusMsg }}</span>
      </div>
    </div>

    <!-- Startup Behavior -->
    <div class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
      <div class="px-6 py-4 bg-slate-50 border-b border-slate-200 flex items-center gap-3">
        <div class="w-8 h-8 rounded-lg bg-violet-100 text-violet-600 flex items-center justify-center shrink-0">
          <Clock class="w-4 h-4" />
        </div>
        <h3 class="text-base font-semibold text-slate-700">{{ t('settings.startupOptions') }}</h3>
      </div>
      <div class="p-6 space-y-4">
      <label class="flex items-center justify-between gap-3">
        <div>
          <div class="text-sm font-medium text-slate-700">{{ t('settings.launchAndAutoScan') }}</div>
          <p class="text-xs text-slate-400 mt-1">{{ t('settings.launchAndAutoScanDesc') }}</p>
        </div>
        <div class="shrink-0 relative inline-flex items-center cursor-pointer">
          <input type="checkbox" v-model="config.launch_and_auto_scan" @change="save" class="sr-only peer">
          <div class="w-11 h-6 bg-slate-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
        </div>
      </label>
      <label class="flex items-center justify-between gap-3">
        <div>
          <div class="text-sm font-medium text-slate-700">{{ t('settings.closeToTray') }}</div>
          <p class="text-xs text-slate-400 mt-1">{{ t('settings.closeToTrayDesc') }}</p>
        </div>
        <div class="shrink-0 relative inline-flex items-center cursor-pointer">
          <input type="checkbox" v-model="config.close_to_tray" @change="save" class="sr-only peer">
          <div class="w-11 h-6 bg-slate-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
        </div>
      </label>
      <div class="flex items-center justify-between gap-3">
        <div>
          <div class="text-sm font-medium text-slate-700">{{ t('settings.maxLogLines') }}</div>
          <p class="text-xs text-slate-400 mt-1">{{ t('settings.maxLogLinesDesc') }}</p>
        </div>
        <div class="flex items-center gap-2">
          <input type="number" v-model.number="config.max_log_lines" @change="save" min="50" max="5000" step="50"
                 class="w-24 rounded-lg border border-slate-300 px-3 py-1.5 text-sm text-right focus:ring-2 focus:ring-blue-500 focus:border-blue-500" />
          <span class="text-xs text-slate-500">{{ t('settings.lines') }}</span>
        </div>
      </div>
      <div class="flex items-center justify-between gap-3">
        <div>
          <div class="text-sm font-medium text-slate-700">{{ t('settings.maxTaskRecords') }}</div>
          <p class="text-xs text-slate-400 mt-1">{{ t('settings.maxTaskRecordsDesc') }}</p>
        </div>
        <div class="flex items-center gap-2">
          <input type="number" v-model.number="config.max_task_records" @change="save" min="10" max="500" step="10"
                 class="w-24 rounded-lg border border-slate-300 px-3 py-1.5 text-sm text-right focus:ring-2 focus:ring-blue-500 focus:border-blue-500" />
          <span class="text-xs text-slate-500">{{ t('settings.lines') }}</span>
        </div>
      </div>
      </div>
    </div>

    <!-- Language Settings -->
    <div class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
      <div class="px-6 py-4 bg-slate-50 border-b border-slate-200 flex items-center gap-3">
        <div class="w-8 h-8 rounded-lg bg-blue-100 text-blue-600 flex items-center justify-center shrink-0">
          <Globe class="w-4 h-4" />
        </div>
        <h3 class="text-base font-semibold text-slate-700">{{ t('settings.language') }}</h3>
      </div>
      <div class="p-6 space-y-4">
      <div class="flex gap-4">
        <button @click="changeLanguage('zh')" class="px-4 py-2 rounded-lg border transition-colors"
          :class="locale === 'zh' ? 'bg-blue-50 border-blue-500 text-blue-700 font-medium' : 'border-slate-300 text-slate-600 hover:bg-slate-50'">
          {{ t('settings.languageChinese') }}
        </button>
        <button @click="changeLanguage('en')" class="px-4 py-2 rounded-lg border transition-colors"
          :class="locale === 'en' ? 'bg-blue-50 border-blue-500 text-blue-700 font-medium' : 'border-slate-300 text-slate-600 hover:bg-slate-50'">
          English
        </button>
      </div>
      </div>
    </div>

    <!-- App Data Paths -->
    <div class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
      <div class="px-6 py-4 bg-slate-50 border-b border-slate-200 flex items-center gap-3">
        <div class="w-8 h-8 rounded-lg bg-slate-100 text-slate-600 flex items-center justify-center shrink-0">
          <FileText class="w-4 h-4" />
        </div>
        <h3 class="text-base font-semibold text-slate-700">{{ t('settings.configPaths') }}</h3>
      </div>
      <div class="p-6 space-y-4">
      <div class="space-y-3">
         <div>
            <label class="block text-xs font-medium text-slate-500 mb-1 uppercase tracking-wider">{{ t('settings.configFile') }}</label>
            <div class="flex gap-2">
               <code class="flex-1 p-2.5 bg-slate-50 border border-slate-200 rounded-lg text-xs font-mono text-slate-600 break-all">{{ configPath }}</code>
               <button @click="copyToClipboard(configPath)" class="p-2 text-slate-400 hover:text-blue-600 hover:bg-blue-50 rounded-lg transition-colors border border-transparent hover:border-blue-100" :title="t('settings.copyPath')">
                  <Copy class="w-4 h-4" />
               </button>
               <button @click="openParentFolder(configPath)" class="p-2 text-slate-400 hover:text-blue-600 hover:bg-blue-50 rounded-lg transition-colors border border-transparent hover:border-blue-100" :title="t('settings.openFolder')">
                  <FolderOpen class="w-4 h-4" />
               </button>
            </div>
         </div>
         <div>
            <label class="block text-xs font-medium text-slate-500 mb-1 uppercase tracking-wider">{{ t('settings.logFile') }}</label>
            <div class="flex gap-2">
               <code class="flex-1 p-2.5 bg-slate-50 border border-slate-200 rounded-lg text-xs font-mono text-slate-600 break-all">{{ logPath }}</code>
               <button @click="copyToClipboard(logPath)" class="p-2 text-slate-400 hover:text-blue-600 hover:bg-blue-50 rounded-lg transition-colors border border-transparent hover:border-blue-100" :title="t('settings.copyPath')">
                  <Copy class="w-4 h-4" />
               </button>
               <button @click="openParentFolder(logPath)" class="p-2 text-slate-400 hover:text-blue-600 hover:bg-blue-50 rounded-lg transition-colors border border-transparent hover:border-blue-100" :title="t('settings.openFolder')">
                  <FolderOpen class="w-4 h-4" />
               </button>
            </div>
         </div>
      </div>
      </div>
    </div>

    <!-- Local Path -->
    <div class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
      <div class="px-6 py-4 bg-slate-50 border-b border-slate-200 flex items-center gap-3">
        <div class="w-8 h-8 rounded-lg bg-amber-100 text-amber-600 flex items-center justify-center shrink-0">
          <FolderOpen class="w-4 h-4" />
        </div>
        <h3 class="text-base font-semibold text-slate-700">{{ t('settings.localStorage') }}</h3>
      </div>
      <div class="p-6 space-y-4">
      <div>
        <label class="block text-sm font-medium text-slate-600 mb-1">{{ t('settings.localPath') }}</label>
        <input v-model="config.local_path" type="text"
          class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none transition-all" />
        <p class="text-xs text-slate-400 mt-1">{{ t('settings.localPathDesc') }}</p>
      </div>
      </div>
    </div>

    <!-- Tasks Management -->
    <div class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
      <div class="px-6 py-4 bg-slate-50 border-b border-slate-200 flex items-center justify-between gap-3">
        <div class="flex items-center gap-3">
          <div class="w-8 h-8 rounded-lg bg-blue-100 text-blue-600 flex items-center justify-center shrink-0">
            <ListChecks class="w-4 h-4" />
          </div>
          <h3 class="text-base font-semibold text-slate-700">{{ t('settings.scanTasks') }}</h3>
        </div>
        <button @click="addTask" class="text-xs text-blue-600 hover:text-blue-800 flex items-center gap-1 font-medium bg-blue-50 hover:bg-blue-100 px-3 py-1.5 rounded-lg transition-colors">
          <Plus class="w-3 h-3" /> {{ t('settings.addTask') }}
        </button>
      </div>
      <div class="p-6 space-y-3">
        <div v-if="config.tasks.length === 0" class="text-center p-6 bg-slate-50 rounded-lg border border-dashed border-slate-300 text-slate-500 text-sm">
          {{ t('settings.noTasks') }}
        </div>

        <div v-else class="space-y-3">
          <div v-for="(task, idx) in config.tasks" :key="task.id"
            class="border border-slate-200 rounded-lg p-3 bg-white hover:shadow-sm transition-shadow flex items-center justify-between gap-4">
            <div class="flex items-center gap-3 overflow-hidden flex-1">
              <div class="shrink-0" :title="t('settings.enableToggle')">
                <input type="checkbox" v-model="task.enabled" @change="save" class="rounded text-blue-600 focus:ring-blue-500 w-4 h-4 cursor-pointer">
              </div>
              <div class="flex-1 min-w-0">
                <div class="font-medium text-slate-800 flex items-center gap-2 flex-wrap">
                  {{ task.name }}
                  <span class="text-xs px-2 py-0.5 rounded-full border"
                    :class="task.rule.type === 'VersionMatch' ? 'bg-purple-50 text-purple-700 border-purple-100' : 'bg-orange-50 text-orange-700 border-orange-100'">
                    {{ task.rule.type === 'VersionMatch' ? t('settings.ruleVersionShort') : t('settings.ruleDateShort') }}: {{ task.rule.value }}
                  </span>
                  <!-- Server binding badges -->
                  <template v-if="task.server_bindings && task.server_bindings.length > 0">
                    <span v-for="binding in task.server_bindings" :key="binding.server_id"
                      class="text-xs px-2 py-0.5 rounded-full bg-blue-50 text-blue-600 border border-blue-100 flex items-center gap-1">
                      <Server class="w-2.5 h-2.5" />
                      {{ config.servers.find(s => s.id === binding.server_id)?.name || binding.server_id.substring(0, 8) }}
                      <template v-if="binding.command_group_ids.length > 0">
                        <span class="text-blue-400">·</span>
                        {{ binding.command_group_ids.map(id => commandGroupName(id)).join('+') }}
                      </template>
                    </span>
                  </template>
                  <span v-else-if="config.deploy_enabled" class="text-xs px-2 py-0.5 rounded-full bg-slate-50 text-slate-400 border border-slate-200">
                    {{ t('settings.taskDeployNone') }}
                  </span>
                </div>
                <div class="text-xs text-slate-500 font-mono truncate" :title="task.remote_path">
                  {{ task.remote_path }}
                </div>
              </div>
            </div>
            <div class="flex items-center gap-1 shrink-0">
              <button @click="editTask(idx)" class="p-1.5 text-slate-500 hover:text-amber-600 hover:bg-amber-50 rounded transition-colors" :title="t('settings.edit')">
                <Edit class="w-4 h-4" />
              </button>
              <button @click="removeTask(idx)" class="p-1.5 text-slate-500 hover:text-red-600 hover:bg-red-50 rounded transition-colors" :title="t('settings.deleteTitle')">
                <Trash2 class="w-4 h-4" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Task Edit Modal -->
    <div v-if="isEditingTask" class="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
      <div class="bg-white rounded-xl p-6 w-full max-w-lg shadow-2xl transform transition-all max-h-[90vh] flex flex-col">
        <h3 class="text-lg font-bold mb-4 text-slate-800 shrink-0">{{ editingTaskIndex > -1 ? t('settings.editTask') : t('settings.addTask') }}</h3>
        <div class="space-y-4 overflow-y-auto flex-1 pr-1">
          <div>
            <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.taskName') }}</label>
            <input v-model="taskForm.name" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" :placeholder="t('settings.taskNamePlaceholder')" />
          </div>
          <div>
            <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.remotePath') }}</label>
            <input v-model="taskForm.remote_path" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" placeholder="\\server\share\path" />
          </div>
          <div>
            <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.localPathOverride') }}</label>
            <input v-model="taskForm.local_path" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" :placeholder="t('settings.localPathDesc')" />
          </div>

          <div class="grid grid-cols-2 gap-4">
            <div>
              <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.taskRuleType') }}</label>
              <select v-model="taskForm.rule.type" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none bg-white">
                <option value="VersionMatch">{{ t('settings.ruleVersion') }}</option>
                <option value="DateMatch">{{ t('settings.ruleDate') }}</option>
              </select>
            </div>
            <div>
              <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.taskRuleValue') }}</label>
              <input v-model="taskForm.rule.value" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none"
                :placeholder="taskForm.rule.type === 'VersionMatch' ? t('settings.ruleValuePlaceholderVersion') : t('settings.ruleValuePlaceholderDate')" />
              <p class="text-xs text-slate-400 mt-1" v-if="taskForm.rule.type === 'DateMatch'">{{ t('settings.ruleDateHint') }}</p>
            </div>
          </div>

          <!-- Server Bindings (only shown when deploy is enabled and servers exist) -->
          <div v-if="config.deploy_enabled && config.servers.length > 0" class="pt-3 border-t border-slate-100">
            <div class="flex items-center justify-between mb-2">
              <label class="block text-sm font-medium text-slate-700 flex items-center gap-1.5">
                <Server class="w-4 h-4 text-blue-500" />
                {{ t('settings.taskServerBindings') }}
              </label>
              <button @click="addServerBinding" type="button"
                class="text-xs text-blue-600 hover:text-blue-800 flex items-center gap-1 font-medium bg-blue-50 hover:bg-blue-100 px-2.5 py-1 rounded-lg transition-colors">
                <Plus class="w-3 h-3" /> {{ t('settings.addServerBinding') }}
              </button>
            </div>
            <p class="text-xs text-slate-400 mb-3">{{ t('settings.taskServerBindingsDesc') }}</p>

            <div v-if="taskForm.server_bindings.length === 0" class="text-xs text-slate-400 italic text-center py-3 bg-slate-50 rounded-lg border border-dashed border-slate-200">
              {{ t('settings.noServerBindings') }}
            </div>

            <div v-else class="space-y-2">
              <div v-for="(binding, bidx) in taskForm.server_bindings" :key="bidx"
                class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-2">
                <div class="flex items-center gap-2">
                  <select v-model="binding.server_id"
                    class="flex-1 p-1.5 border border-slate-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 outline-none bg-white">
                    <option value="" disabled>{{ t('settings.selectServer') }}</option>
                    <option v-for="s in config.servers" :key="s.id" :value="s.id">
                      {{ serverDisplayName(s) }} ({{ s.host }})
                    </option>
                  </select>
                  <button @click="removeServerBinding(bidx)" type="button"
                    class="p-1.5 text-slate-400 hover:text-red-500 hover:bg-red-50 rounded transition-colors shrink-0">
                    <Trash2 class="w-3.5 h-3.5" />
                  </button>
                </div>
                <!-- Command group selection -->
                <div v-if="config.command_groups.length > 0">
                  <div class="text-xs text-slate-500 mb-1.5">{{ t('settings.bindingCommandGroups') }}:</div>
                  <div class="flex flex-wrap gap-1.5">
                    <button v-for="group in config.command_groups" :key="group.id"
                      type="button"
                      @click="toggleBindingGroup(binding, group.id)"
                      class="text-xs px-2.5 py-1 rounded-full border font-medium transition-colors"
                      :class="binding.command_group_ids.includes(group.id)
                        ? 'bg-sky-100 text-sky-700 border-sky-200'
                        : 'bg-white text-slate-500 border-slate-200 hover:bg-slate-100'">
                      <span v-if="bindingGroupOrder(binding, group.id)" class="mr-1 text-[10px] font-bold">
                        #{{ bindingGroupOrder(binding, group.id) }}
                      </span>
                      {{ group.name }}
                    </button>
                  </div>
                  <div v-if="binding.command_group_ids.length === 0" class="text-xs text-slate-400 italic mt-1">{{ t('settings.bindingNoGroups') }}</div>
                  <div v-else class="mt-3 space-y-2">
                    <div class="flex items-center justify-between gap-3">
                      <div class="text-xs text-slate-500">{{ t('settings.bindingExecutionOrder') }}</div>
                      <div class="text-[11px] text-slate-400">{{ t('settings.bindingExecutionHint') }}</div>
                    </div>
                    <div
                      v-for="(groupId, groupIndex) in binding.command_group_ids"
                      :key="`${binding.server_id}-${groupId}-${groupIndex}`"
                      class="flex items-center justify-between gap-3 rounded-lg border border-slate-200 bg-white px-3 py-2"
                    >
                      <div class="flex min-w-0 items-center gap-3">
                        <span class="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-sky-100 text-xs font-semibold text-sky-700">
                          {{ groupIndex + 1 }}
                        </span>
                        <div class="min-w-0">
                          <div class="truncate text-sm font-medium text-slate-700">{{ commandGroupName(groupId) }}</div>
                          <div class="text-[11px] text-slate-400">{{ t('settings.bindingCommandGroups') }}</div>
                        </div>
                      </div>
                      <div class="flex items-center gap-1 shrink-0">
                        <button
                          type="button"
                          @click="moveBindingGroup(binding, groupIndex, -1)"
                          :disabled="groupIndex === 0"
                          class="rounded p-1.5 text-slate-400 transition-colors"
                          :class="groupIndex === 0 ? 'cursor-not-allowed opacity-40' : 'hover:bg-slate-100 hover:text-slate-600'"
                          :title="t('settings.moveUp')"
                        >
                          <ArrowUp class="h-3.5 w-3.5" />
                        </button>
                        <button
                          type="button"
                          @click="moveBindingGroup(binding, groupIndex, 1)"
                          :disabled="groupIndex === binding.command_group_ids.length - 1"
                          class="rounded p-1.5 text-slate-400 transition-colors"
                          :class="groupIndex === binding.command_group_ids.length - 1 ? 'cursor-not-allowed opacity-40' : 'hover:bg-slate-100 hover:text-slate-600'"
                          :title="t('settings.moveDown')"
                        >
                          <ArrowDown class="h-3.5 w-3.5" />
                        </button>
                        <button
                          type="button"
                          @click="removeBindingGroupById(binding, groupId)"
                          class="rounded p-1.5 text-slate-400 transition-colors hover:bg-red-50 hover:text-red-500"
                          :title="t('settings.deleteTitle')"
                        >
                          <Trash2 class="h-3.5 w-3.5" />
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
                <div v-else class="text-xs text-slate-400 italic">{{ t('settings.noCommandGroups') }}</div>
              </div>
            </div>
          </div>
        </div>
        <div class="flex justify-end gap-3 mt-8 pt-4 border-t border-slate-100">
          <button @click="isEditingTask = false" class="px-4 py-2 text-slate-600 hover:bg-slate-100 rounded-lg font-medium transition-colors">{{ t('console.cancel') }}</button>
          <button @click="saveTask" :disabled="!taskForm.rule.value" class="px-6 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 font-medium transition-colors shadow-sm disabled:opacity-50 disabled:cursor-not-allowed">{{ t('settings.save') }}</button>
        </div>
      </div>
    </div>

    <!-- Scan Timing -->
    <div class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
      <div class="px-6 py-4 bg-slate-50 border-b border-slate-200 flex items-center gap-3">
        <div class="w-8 h-8 rounded-lg bg-emerald-100 text-emerald-600 flex items-center justify-center shrink-0">
          <Clock class="w-4 h-4" />
        </div>
        <h3 class="text-base font-semibold text-slate-700">{{ t('settings.scanTime') }}</h3>
      </div>
      <div class="p-6 space-y-5">
      <div class="space-y-3">
        <label class="block text-base font-medium text-slate-700">{{ t('settings.scanInterval') }}</label>
        <div class="flex items-center gap-3">
          <input v-model.number="config.interval_minutes" type="number" min="5"
            class="w-28 h-10 px-3 border rounded-lg text-slate-700 focus:ring-2 outline-none"
            :class="intervalError ? 'border-red-400 focus:ring-red-200 focus:border-red-400 bg-red-50' : 'border-slate-300 focus:ring-blue-500 focus:border-blue-500'" />
          <span class="text-sm font-medium text-slate-500">{{ t('settings.minutes') }}</span>
          <span class="text-xs leading-5 text-slate-400">{{ t('settings.minInterval') }}</span>
        </div>
        <p v-if="intervalError" class="text-xs leading-5 text-red-500">{{ intervalError }}</p>
      </div>

      <div class="pt-5 border-t border-slate-100 space-y-4">
        <h4 class="text-base font-medium text-slate-700">{{ t('settings.stabilityCheck') }}</h4>
        <p class="text-sm leading-6 text-slate-500">{{ t('settings.stabilityCheckDesc') }}</p>
        <div class="space-y-4">
          <div class="space-y-3">
            <label class="block text-base font-medium text-slate-700">{{ t('settings.recentFileGuard') }}</label>
            <div class="flex items-center gap-3">
              <input v-model.number="config.recent_file_guard_mins" type="number" min="3"
                class="w-28 h-10 px-3 border rounded-lg text-slate-700 focus:ring-2 outline-none"
                :class="recentFileGuardError ? 'border-red-400 focus:ring-red-200 focus:border-red-400 bg-red-50' : 'border-slate-300 focus:ring-blue-500 focus:border-blue-500'" />
              <span class="text-sm font-medium text-slate-500">{{ t('settings.minutes') }}</span>
            </div>
            <p v-if="recentFileGuardError" class="text-xs leading-5 text-red-500">{{ recentFileGuardError }}</p>
            <p class="text-sm leading-6 text-slate-500">{{ t('settings.recentFileGuardDesc') }}</p>
            <p class="text-xs leading-5 text-slate-400">{{ t('settings.recentFileGuardHint') }}</p>
          </div>
          <div class="space-y-3">
            <label class="block text-base font-medium text-slate-700">{{ t('settings.stabilityCheckSeconds') }}</label>
            <div class="flex items-center gap-3">
              <input v-model.number="config.stability_check_secs" type="number" min="60"
                class="w-28 h-10 px-3 border rounded-lg text-slate-700 focus:ring-2 outline-none"
                :class="stabilityCheckError ? 'border-red-400 focus:ring-red-200 focus:border-red-400 bg-red-50' : 'border-slate-300 focus:ring-blue-500 focus:border-blue-500'" />
              <span class="text-sm font-medium text-slate-500">{{ t('settings.seconds') }}</span>
            </div>
            <p v-if="stabilityCheckError" class="text-xs leading-5 text-red-500">{{ stabilityCheckError }}</p>
            <p class="text-xs leading-5 text-slate-400">{{ t('settings.stabilityCheckHint') }}</p>
          </div>
        </div>
      </div>

      <div class="pt-5 border-t border-slate-100 space-y-3">
        <div>
          <label class="block text-base font-medium text-slate-700">{{ t('settings.copyBufferSize') }}</label>
          <p class="text-sm leading-6 text-slate-500 mt-1">{{ t('settings.copyBufferSizeDesc') }}</p>
        </div>
        <select v-model.number="config.copy_buffer_size_kb"
                class="w-44 h-10 px-3 border border-slate-300 rounded-lg text-slate-700 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none bg-white">
          <option :value="64">64 KB</option>
          <option :value="256">256 KB</option>
          <option :value="1024">1 MB</option>
          <option :value="4096">4 MB{{ locale === 'zh' ? '（推荐）' : ' (recommended)' }}</option>
          <option :value="8192">8 MB</option>
          <option :value="16384">16 MB</option>
        </select>
      </div>

      <div class="pt-5 border-t border-slate-100 space-y-4">
        <h4 class="text-base font-medium text-slate-700">{{ t('settings.timeRanges') }}</h4>
        <p class="text-sm leading-6 text-slate-500">{{ t('settings.timeRangesDesc') }}</p>
        <div class="flex items-center gap-3">
          <input v-model="newTimeRange" @keyup.enter="addTimeRange" placeholder="09:00-18:00"
            class="flex-1 h-10 px-3 border border-slate-300 rounded-lg text-slate-700 placeholder:text-slate-400 focus:ring-2 focus:ring-blue-500 outline-none" />
          <button @click="addTimeRange" class="h-10 w-10 shrink-0 bg-slate-100 hover:bg-slate-200 rounded-lg text-slate-600 flex items-center justify-center transition-colors">
            <Plus class="w-5 h-5" />
          </button>
        </div>
        <div class="flex flex-wrap gap-2">
          <div v-for="(range, i) in config.time_ranges" :key="i"
            class="bg-amber-50 text-amber-700 px-3 py-1.5 rounded-full text-sm font-medium border border-amber-100 flex items-center gap-2">
            {{ range }}
            <button @click="removeTimeRange(i)" class="hover:text-amber-900"><Trash2 class="w-3 h-3" /></button>
          </div>
        </div>
      </div>
      </div>
    </div>

    <!-- File Filters -->
    <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
      <div class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
        <div class="px-6 py-4 bg-slate-50 border-b border-slate-200 flex items-center gap-3">
          <div class="w-8 h-8 rounded-lg bg-indigo-100 text-indigo-600 flex items-center justify-center shrink-0">
            <FileText class="w-4 h-4" />
          </div>
          <h3 class="text-base font-semibold text-slate-700">{{ t('settings.fileExtensions') }}</h3>
        </div>
        <div class="p-6 space-y-4">
          <p class="text-xs text-slate-400">{{ t('settings.fileExtensionsDesc') }}</p>
          <div class="flex gap-2">
            <input v-model="newExt" @keyup.enter="addExt" placeholder="exe"
              class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
            <button @click="addExt" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600"><Plus class="w-5 h-5" /></button>
          </div>
          <div class="flex flex-wrap gap-2">
            <div v-for="(ext, i) in config.file_extensions" :key="i"
              class="bg-indigo-50 text-indigo-700 px-3 py-1 rounded-full text-sm font-medium border border-indigo-100 flex items-center gap-2">
              {{ ext }}
              <button @click="removeExt(i)" class="hover:text-indigo-900"><Trash2 class="w-3 h-3" /></button>
            </div>
          </div>
        </div>
      </div>

      <div class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
        <div class="px-6 py-4 bg-slate-50 border-b border-slate-200 flex items-center gap-3">
          <div class="w-8 h-8 rounded-lg bg-purple-100 text-purple-600 flex items-center justify-center shrink-0">
            <ListChecks class="w-4 h-4" />
          </div>
          <h3 class="text-base font-semibold text-slate-700">{{ t('settings.filenameKeywords') }}</h3>
        </div>
        <div class="p-6 space-y-4">
          <p class="text-xs text-slate-400">{{ t('settings.filenameKeywordsDesc') }}</p>
          <div class="flex gap-2">
            <input v-model="newInclude" @keyup.enter="addInclude" placeholder="UMS"
              class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
            <button @click="addInclude" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600"><Plus class="w-5 h-5" /></button>
          </div>
          <div class="flex flex-wrap gap-2">
            <div v-for="(inc, i) in config.filename_includes" :key="i"
              class="bg-purple-50 text-purple-700 px-3 py-1 rounded-full text-sm font-medium border border-purple-100 flex items-center gap-2">
              {{ inc }}
              <button @click="removeInclude(i)" class="hover:text-purple-900"><Trash2 class="w-3 h-3" /></button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Deploy Settings -->
    <div class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
      <div class="px-6 py-4 bg-slate-50 border-b border-slate-200 flex items-center justify-between gap-3">
        <div class="flex items-center gap-3">
          <div class="w-8 h-8 rounded-lg bg-rose-100 text-rose-600 flex items-center justify-center shrink-0">
            <Server class="w-4 h-4" />
          </div>
          <h3 class="text-base font-semibold text-slate-700">{{ t('settings.remoteDeployment') }}</h3>
        </div>
        <label class="relative inline-flex items-center cursor-pointer">
          <input type="checkbox" v-model="config.deploy_enabled" class="sr-only peer">
          <div class="w-11 h-6 bg-slate-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600"></div>
          <span class="ml-3 text-sm font-medium text-slate-700">{{ t('settings.enable') }}</span>
        </label>
      </div>

      <div v-if="config.deploy_enabled" class="p-6 space-y-6">
        <!-- Server List -->
        <div>
          <div class="flex justify-between items-start gap-4 mb-3">
            <div>
              <h4 class="font-medium text-slate-700">{{ t('settings.servers') }}</h4>
              <p class="text-xs text-slate-400 mt-1">{{ t('settings.manageModeDesc') }}</p>
            </div>
            <div class="flex gap-2">
              <button @click="openServerManager" v-if="config.servers.length > 0"
                class="text-xs text-slate-600 hover:text-slate-800 flex items-center gap-1 font-medium bg-slate-100 hover:bg-slate-200 px-3 py-1.5 rounded-lg transition-colors">
                <Server class="w-3 h-3" /> {{ t('settings.detailsList') }}
              </button>
              <button @click="addServer" class="text-xs text-blue-600 hover:text-blue-800 flex items-center gap-1 font-medium bg-blue-50 hover:bg-blue-100 px-3 py-1.5 rounded-lg transition-colors">
                <Plus class="w-3 h-3" /> {{ t('settings.addServer') }}
              </button>
            </div>
          </div>

          <div v-if="config.servers.length === 0" class="text-center p-6 bg-slate-50 rounded-lg border border-dashed border-slate-300 text-slate-500 text-sm">
            {{ t('settings.noServers') }}
          </div>

          <div v-else class="rounded-xl border border-slate-200 bg-slate-50 p-4 space-y-4">
            <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
              <div class="rounded-xl bg-white border border-slate-200 p-4">
                <div class="text-xs text-slate-400">{{ t('settings.serverCount') }}</div>
                <div class="text-2xl font-bold text-slate-800 mt-1">{{ config.servers.length }}</div>
              </div>
              <div class="rounded-xl bg-white border border-slate-200 p-4">
                <div class="text-xs text-slate-400">{{ t('settings.enabledCount') }}</div>
                <div class="text-2xl font-bold text-emerald-600 mt-1">{{ enabledServerCount }}</div>
              </div>
            </div>
            <div class="flex flex-wrap gap-2">
              <span v-for="server in config.servers.slice(0, 3)" :key="server.id"
                class="text-xs px-2.5 py-1 rounded-full bg-white border border-slate-200 text-slate-700">
                {{ serverDisplayName(server) }}
              </span>
              <span v-if="config.servers.length > 3" class="text-xs px-2.5 py-1 rounded-full bg-slate-200 text-slate-600">+{{ config.servers.length - 3 }}</span>
            </div>
          </div>
        </div>

        <!-- Server Manager Modal -->
        <div v-if="isServerManagerOpen" class="fixed inset-0 bg-black/50 flex items-center justify-center z-[55] p-4">
          <div class="bg-white rounded-xl p-6 w-full max-w-5xl shadow-2xl max-h-[86vh] overflow-hidden flex flex-col">
            <div class="flex items-center justify-between gap-4 mb-4">
              <div>
                <h3 class="text-lg font-bold text-slate-800">{{ t('settings.serverDetailsTitle') }}</h3>
                <p class="text-sm text-slate-400 mt-1">{{ t('settings.serverDetailsDesc') }}</p>
              </div>
              <div class="flex items-center gap-2">
                <button @click="testAllServers" v-if="config.servers.length > 0"
                  class="text-xs text-slate-600 hover:text-slate-800 flex items-center gap-1 font-medium bg-slate-100 hover:bg-slate-200 px-3 py-1.5 rounded-lg transition-colors">
                  <Server class="w-3 h-3" /> {{ t('settings.testAll') }}
                </button>
                <button @click="closeServerManager" class="px-3 py-1.5 text-slate-500 hover:bg-slate-100 rounded-lg transition-colors">{{ t('settings.close') }}</button>
              </div>
            </div>

            <div class="flex-1 overflow-y-auto pr-1 space-y-3">
              <div v-for="(server, idx) in config.servers" :key="server.id"
                class="border border-slate-200 rounded-xl p-4 bg-white hover:shadow-sm transition-shadow">
                <div class="flex items-start justify-between gap-4 flex-wrap">
                  <div class="flex items-start gap-3 flex-1 min-w-[280px]">
                    <input type="checkbox" v-model="server.enabled" @change="save" class="rounded text-blue-600 focus:ring-blue-500 w-4 h-4 cursor-pointer mt-1">
                    <div class="flex-1 min-w-0">
                      <div class="font-medium text-slate-800 flex items-center gap-2 flex-wrap">
                        {{ serverDisplayName(server) }}
                        <span v-if="!server.enabled" class="text-xs bg-slate-100 text-slate-500 px-1.5 py-0.5 rounded">{{ t('settings.disabled') }}</span>
                      </div>
                      <div class="text-xs text-slate-500 font-mono break-all mt-1">{{ server.user }}@{{ server.host }}:{{ server.port }}</div>
                      <div class="text-xs text-slate-400 font-mono break-all mt-1">{{ server.remote_path }}</div>
                    </div>
                  </div>
                  <div class="flex items-center gap-1 shrink-0">
                    <button @click="testServerConnection(idx)"
                      class="flex items-center gap-1.5 px-2.5 py-1.5 text-xs rounded-lg transition-colors border font-medium"
                      :class="{
                        'text-slate-500 border-slate-200 hover:text-blue-600 hover:bg-blue-50 hover:border-blue-200': getServerStatus(server.id).state === 'idle',
                        'text-blue-500 border-blue-200 bg-blue-50 cursor-not-allowed': getServerStatus(server.id).state === 'testing',
                        'text-emerald-600 border-emerald-200 bg-emerald-50': getServerStatus(server.id).state === 'ok',
                        'text-red-600 border-red-200 bg-red-50': getServerStatus(server.id).state === 'error',
                      }"
                      :disabled="getServerStatus(server.id).state === 'testing'"
                      :title="getServerStatus(server.id).msg || t('settings.testConnection')">
                      <Server v-if="getServerStatus(server.id).state === 'idle'" class="w-3.5 h-3.5" />
                      <svg v-else-if="getServerStatus(server.id).state === 'testing'" class="w-3.5 h-3.5 animate-spin" fill="none" viewBox="0 0 24 24">
                        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="3"/>
                        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z"/>
                      </svg>
                      <span v-else-if="getServerStatus(server.id).state === 'ok'" class="relative flex h-2 w-2">
                        <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
                        <span class="relative inline-flex rounded-full h-2 w-2 bg-emerald-500"></span>
                      </span>
                      <XCircle v-else class="w-3.5 h-3.5" />
                      <span>{{
                        getServerStatus(server.id).state === 'testing' ? t('settings.testing') :
                        getServerStatus(server.id).state === 'ok' ? t('settings.connected') :
                        getServerStatus(server.id).state === 'error' ? t('settings.failed') :
                        t('settings.testConnection')
                      }}</span>
                    </button>
                    <button @click="editServer(idx)" class="p-1.5 text-slate-500 hover:text-amber-600 hover:bg-amber-50 rounded transition-colors" :title="t('settings.edit')">
                      <Edit class="w-4 h-4" />
                    </button>
                    <button @click="removeServer(idx)" class="p-1.5 text-slate-500 hover:text-red-600 hover:bg-red-50 rounded transition-colors" :title="t('settings.deleteTitle')">
                      <Trash2 class="w-4 h-4" />
                    </button>
                  </div>
                </div>
                <div v-if="getServerStatus(server.id).state === 'error'" class="mt-3 text-xs text-red-600 bg-red-50 border border-red-100 rounded px-2.5 py-1.5 font-mono break-all">
                  {{ getServerStatus(server.id).msg }}
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Server Edit Modal -->
        <div v-if="isEditingServer" class="fixed inset-0 bg-black/50 flex items-center justify-center z-[65] p-4">
          <div class="bg-white rounded-xl p-6 w-full max-w-lg shadow-2xl transform transition-all">
            <h3 class="text-lg font-bold mb-6 text-slate-800">{{ editingServerIndex > -1 ? t('settings.editServer') : t('settings.addServer') }}</h3>
            <div class="space-y-4">
              <div>
                <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.nameAlias') }}</label>
                <input v-model="serverForm.name" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" :placeholder="t('settings.serverNamePlaceholder')" />
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

        <!-- Command Groups -->
        <div class="pt-6 border-t border-slate-100">
          <div class="flex justify-between items-start gap-4 mb-3">
            <div>
              <h4 class="font-medium text-slate-700 flex items-center gap-2">
                <Layers class="w-4 h-4 text-sky-500" />
                {{ t('settings.commandGroups') }}
              </h4>
              <p class="text-xs text-slate-400 mt-1">{{ t('settings.commandGroupsDesc') }}</p>
            </div>
            <button @click="addCommandGroup" class="text-xs text-sky-600 hover:text-sky-800 flex items-center gap-1 font-medium bg-sky-50 hover:bg-sky-100 px-3 py-1.5 rounded-lg transition-colors">
              <Plus class="w-3 h-3" /> {{ t('settings.addCommandGroup') }}
            </button>
          </div>

          <div v-if="config.command_groups.length === 0" class="text-center p-6 bg-slate-50 rounded-lg border border-dashed border-slate-300 text-slate-500 text-sm">
            {{ t('settings.noCommandGroups') }}
          </div>

          <div v-else class="space-y-2">
            <div v-for="(group, idx) in config.command_groups" :key="group.id"
              class="border border-slate-200 rounded-lg p-3 bg-white hover:shadow-sm transition-shadow flex items-center justify-between gap-3">
              <div class="flex items-center gap-3 flex-1 min-w-0">
                <div class="w-7 h-7 rounded-lg bg-sky-100 text-sky-600 flex items-center justify-center shrink-0">
                  <Terminal class="w-3.5 h-3.5" />
                </div>
                <div class="min-w-0">
                  <div class="font-medium text-slate-800 text-sm">{{ group.name }}</div>
                  <div class="text-xs text-slate-400">{{ group.commands.length }} {{ group.commands.length === 1 ? 'command' : 'commands' }}</div>
                </div>
                <div class="flex flex-wrap gap-1 ml-2">
                  <code v-for="(cmd, ci) in group.commands.slice(0, 2)" :key="ci"
                    class="text-[10px] bg-slate-100 text-slate-500 px-1.5 py-0.5 rounded font-mono truncate max-w-[120px]">{{ cmd }}</code>
                  <span v-if="group.commands.length > 2" class="text-[10px] text-slate-400">+{{ group.commands.length - 2 }}</span>
                </div>
              </div>
              <div class="flex items-center gap-1 shrink-0">
                <button @click="editCommandGroup(idx)" class="p-1.5 text-slate-500 hover:text-amber-600 hover:bg-amber-50 rounded transition-colors" :title="t('settings.edit')">
                  <Edit class="w-4 h-4" />
                </button>
                <button @click="removeCommandGroup(idx)" class="p-1.5 text-slate-500 hover:text-red-600 hover:bg-red-50 rounded transition-colors" :title="t('settings.deleteTitle')">
                  <Trash2 class="w-4 h-4" />
                </button>
              </div>
            </div>
          </div>
          <p class="mt-2 text-xs text-slate-400 leading-relaxed">{{ t('settings.postCommandsHint') }}</p>
        </div>

        <!-- Command Group Edit Modal -->
        <div v-if="isEditingCommandGroup" class="fixed inset-0 bg-black/50 flex items-center justify-center z-[65] p-4">
          <div class="bg-white rounded-xl p-6 w-full max-w-lg shadow-2xl transform transition-all max-h-[80vh] flex flex-col">
            <h3 class="text-lg font-bold mb-4 text-slate-800 shrink-0">{{ editingCommandGroupIndex > -1 ? t('settings.editCommandGroup') : t('settings.addCommandGroup') }}</h3>
            <div class="space-y-4 flex-1 overflow-y-auto pr-1">
              <div>
                <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.commandGroupName') }}</label>
                <input v-model="commandGroupForm.name" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" :placeholder="t('settings.commandGroupNamePlaceholder')" />
              </div>
              <div>
                <label class="block text-sm font-medium mb-2 text-slate-700">{{ t('settings.commandGroupCommands') }}</label>
                <div class="flex gap-2 mb-2">
                  <input v-model="newGroupCommand" @keyup.enter="addGroupCommand"
                    :placeholder="t('settings.commandGroupCommandPlaceholder')"
                    class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-sky-500 outline-none font-mono text-sm" />
                  <button @click="addGroupCommand" type="button" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600">
                    <Plus class="w-5 h-5" />
                  </button>
                </div>
                <ul class="space-y-1.5 bg-slate-900 rounded-lg p-3 max-h-48 overflow-y-auto">
                  <li v-for="(cmd, i) in commandGroupForm.commands" :key="i"
                    class="flex justify-between items-center text-sky-300 font-mono text-xs">
                    <span class="truncate mr-2">$ {{ cmd }}</span>
                    <button @click="removeGroupCommand(i)" type="button" class="text-slate-500 hover:text-red-400 p-1 shrink-0">
                      <Trash2 class="w-3 h-3" />
                    </button>
                  </li>
                  <li v-if="!commandGroupForm.commands.length" class="text-slate-600 text-xs italic text-center">{{ t('settings.commandGroupNoCommands') }}</li>
                </ul>
              </div>
            </div>
            <div class="flex justify-end gap-3 mt-6 pt-4 border-t border-slate-100 shrink-0">
              <button @click="isEditingCommandGroup = false" class="px-4 py-2 text-slate-600 hover:bg-slate-100 rounded-lg font-medium transition-colors">{{ t('console.cancel') }}</button>
              <button @click="saveCommandGroup" :disabled="!commandGroupForm.name.trim()"
                class="px-6 py-2 bg-sky-600 text-white rounded-lg hover:bg-sky-700 font-medium transition-colors shadow-sm disabled:opacity-50 disabled:cursor-not-allowed">{{ t('settings.save') }}</button>
            </div>
          </div>
        </div>

        <!-- Manual Deploy Tool -->
        <div class="pt-6 border-t border-slate-100 space-y-4">
          <h4 class="text-md font-medium text-slate-700 flex items-center gap-2">
            <UploadCloud class="w-4 h-4" />
            {{ t('settings.manualDeploy') }}
          </h4>
          <p class="text-xs text-slate-400">{{ t('settings.manualDeployDesc') }}</p>

          <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <label class="block text-sm font-medium text-slate-600 mb-1">{{ t('settings.localPath') }}</label>
              <input v-model="manualLocalPath" type="text" :placeholder="t('settings.manualLocalPlaceholder')" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
            </div>
            <div>
              <label class="block text-sm font-medium text-slate-600 mb-1">{{ t('settings.remotePath') }}</label>
              <input v-model="manualRemotePath" type="text" :placeholder="t('settings.manualRemotePlaceholder')" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
            </div>
          </div>

          <!-- Server Bindings -->
          <div>
            <div class="flex items-center justify-between mb-2">
              <label class="text-sm font-medium text-slate-600">{{ t('settings.manualDeployServerBindings') }}</label>
              <button @click="addManualBinding" type="button"
                class="text-xs text-blue-600 hover:text-blue-800 flex items-center gap-1 font-medium bg-blue-50 hover:bg-blue-100 px-2.5 py-1 rounded-lg transition-colors">
                <Plus class="w-3 h-3" /> {{ t('settings.addServerBinding') }}
              </button>
            </div>

            <div v-if="manualServerBindings.length === 0" class="text-xs text-slate-400 italic text-center py-3 bg-slate-50 rounded-lg border border-dashed border-slate-200">
              {{ t('settings.manualDeployNoBindings') }}
            </div>

            <div v-else class="space-y-2">
              <div v-for="(binding, bidx) in manualServerBindings" :key="bidx"
                class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-2">
                <div class="flex items-center gap-2">
                  <select v-model="binding.server_id"
                    class="flex-1 p-1.5 border border-slate-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 outline-none bg-white">
                    <option value="" disabled>{{ t('settings.selectServer') }}</option>
                    <option v-for="s in config.servers" :key="s.id" :value="s.id">
                      {{ serverDisplayName(s) }} ({{ s.host }})
                    </option>
                  </select>
                  <button @click="removeManualBinding(bidx)" type="button"
                    class="p-1.5 text-slate-400 hover:text-red-500 hover:bg-red-50 rounded transition-colors shrink-0">
                    <Trash2 class="w-3.5 h-3.5" />
                  </button>
                </div>
                <!-- Command group selection -->
                <div v-if="config.command_groups.length > 0">
                  <div class="text-xs text-slate-500 mb-1.5">{{ t('settings.bindingCommandGroups') }}:</div>
                  <div class="flex flex-wrap gap-1.5">
                    <button v-for="group in config.command_groups" :key="group.id"
                      type="button"
                      @click="toggleManualBindingGroup(binding, group.id)"
                      class="text-xs px-2.5 py-1 rounded-full border font-medium transition-colors"
                      :class="binding.command_group_ids.includes(group.id)
                        ? 'bg-sky-100 text-sky-700 border-sky-200'
                        : 'bg-white text-slate-500 border-slate-200 hover:bg-slate-100'">
                      <span v-if="manualBindingGroupOrder(binding, group.id)" class="mr-1 text-[10px] font-bold">
                        #{{ manualBindingGroupOrder(binding, group.id) }}
                      </span>
                      {{ group.name }}
                    </button>
                  </div>
                  <div v-if="binding.command_group_ids.length === 0" class="text-xs text-slate-400 italic mt-1">{{ t('settings.bindingNoGroups') }}</div>
                  <div v-else class="mt-3 space-y-2">
                    <div class="flex items-center justify-between gap-3">
                      <div class="text-xs text-slate-500">{{ t('settings.bindingExecutionOrder') }}</div>
                      <div class="text-[11px] text-slate-400">{{ t('settings.bindingExecutionHint') }}</div>
                    </div>
                    <div
                      v-for="(groupId, groupIndex) in binding.command_group_ids"
                      :key="`manual-${binding.server_id}-${groupId}-${groupIndex}`"
                      class="flex items-center justify-between gap-3 rounded-lg border border-slate-200 bg-white px-3 py-2"
                    >
                      <div class="flex min-w-0 items-center gap-3">
                        <span class="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-sky-100 text-xs font-semibold text-sky-700">
                          {{ groupIndex + 1 }}
                        </span>
                        <div class="min-w-0">
                          <div class="truncate text-sm font-medium text-slate-700">{{ commandGroupName(groupId) }}</div>
                          <div class="text-[11px] text-slate-400">{{ t('settings.bindingCommandGroups') }}</div>
                        </div>
                      </div>
                      <div class="flex items-center gap-1 shrink-0">
                        <button
                          type="button"
                          @click="moveManualBindingGroup(binding, groupIndex, -1)"
                          :disabled="groupIndex === 0"
                          class="rounded p-1.5 text-slate-400 transition-colors"
                          :class="groupIndex === 0 ? 'cursor-not-allowed opacity-40' : 'hover:bg-slate-100 hover:text-slate-600'"
                        >
                          <ArrowUp class="h-3.5 w-3.5" />
                        </button>
                        <button
                          type="button"
                          @click="moveManualBindingGroup(binding, groupIndex, 1)"
                          :disabled="groupIndex === binding.command_group_ids.length - 1"
                          class="rounded p-1.5 text-slate-400 transition-colors"
                          :class="groupIndex === binding.command_group_ids.length - 1 ? 'cursor-not-allowed opacity-40' : 'hover:bg-slate-100 hover:text-slate-600'"
                        >
                          <ArrowDown class="h-3.5 w-3.5" />
                        </button>
                        <button
                          type="button"
                          @click="removeManualBindingGroupById(binding, groupId)"
                          class="rounded p-1.5 text-slate-400 transition-colors hover:bg-red-50 hover:text-red-500"
                        >
                          <Trash2 class="h-3.5 w-3.5" />
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
                <div v-else class="text-xs text-slate-400 italic">{{ t('settings.noCommandGroups') }}</div>
              </div>
            </div>
          </div>

          <div class="flex items-center gap-3">
            <button @click="handleManualDeploy"
              class="bg-indigo-600 text-white px-4 py-2 rounded-lg hover:bg-indigo-700 transition-colors flex items-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
              :disabled="!canManualDeploy">
              <UploadCloud class="w-4 h-4" />
              {{ appStore.isManualDeploying ? t('settings.deploying') : t('settings.deployNow') }}
            </button>
            <span v-if="appStore.manualDeployMsg" :class="manualDeployMsgType === 'success' ? 'text-green-600' : 'text-red-500'" class="text-sm font-medium">
              {{ appStore.manualDeployMsg }}
              <span v-if="appStore.isManualDeploying && appStore.progress" class="ml-2 text-blue-600">
                ({{ appStore.progress.percentage.toFixed(0) }}%)
              </span>
            </span>
          </div>
        </div>
      </div>
    </div>

    <button
      @click="save"
      :disabled="hasConfigErrors"
      class="fixed right-6 bottom-6 z-40 bg-blue-600 hover:bg-blue-700 text-white px-5 py-3 rounded-full font-medium flex items-center gap-2 transition-all shadow-lg shadow-blue-200/70"
      :class="hasConfigErrors ? 'opacity-50 cursor-not-allowed hover:bg-blue-600 shadow-none' : 'hover:-translate-y-0.5'"
    >
      <Save class="w-4 h-4" />
      {{ t('settings.save') }}
    </button>
  </div>
</template>
