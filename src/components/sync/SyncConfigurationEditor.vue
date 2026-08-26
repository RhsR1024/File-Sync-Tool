<script setup lang="ts">
import { ref, onMounted, computed, watch, nextTick } from 'vue';
import { Save, Plus, Trash2, FolderOpen, Server, Terminal, Clock, UploadCloud, ListChecks, Edit, XCircle, FileText, Copy, Layers, ArrowUp, ArrowDown, X, RotateCcw, Cpu, Monitor, Check, Search, ShieldCheck } from 'lucide-vue-next';
import { preflightManualDeploy, testSshConnection, type AppConfig, type ScanTask, type DeployServer, type CommandGroup, type TaskServerBinding, type LocalCommandGroup, type OnFailure, type ManualDeployTransferPolicy, type ManualDeployExtractPolicy, type ManualDeployPreflightResult, type StartManualDeployTaskRequest } from '@/lib/tauri';
import { appStore } from '@/lib/store';
import { taskStateStore } from '@/lib/taskStateStore';
import { configStore } from '@/lib/configStore';
import DirectoryPathInput from '@/components/settings/DirectoryPathInput.vue';
import Empty from '@/components/Empty.vue';
import AppConfirmDialog from '@/components/AppConfirmDialog.vue';
import ManualDeployLogDialog from '@/components/sync/ManualDeployLogDialog.vue';
import { getDirectoryInputValue, getTaskLocalPathHint, getTaskLocalPathPlaceholder, toOptionalDirectoryValue } from '@/lib/settingsDirectoryPathState';
import { useI18n } from 'vue-i18n';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { pushToast, dismissToast } from '@/composables/useToast';

defineOptions({ name: 'SyncConfigurationEditor' });

export type SyncConfigurationSection = 'all' | 'tasks' | 'strategy' | 'tasks-strategy' | 'delivery';

const props = withDefaults(defineProps<{
  section?: SyncConfigurationSection;
}>(), {
  section: 'all',
});

const { t, locale } = useI18n();
const config = computed(() => configStore.config as AppConfig);

const newExt = ref('');
const newInclude = ref('');
const newTimeRange = ref('');
const isServerManagerOpen = ref(false);
const isSaving = ref(false);
const serverManagerCloseBtn = ref<HTMLButtonElement | null>(null);
let serverManagerOpenerEl: HTMLElement | null = null;

interface PendingConfirmation {
    title: string;
    description: string;
    confirmLabel: string;
    tone: 'danger' | 'warning';
    action: () => void | Promise<void>;
}

const pendingConfirmation = ref<PendingConfirmation | null>(null);
const confirmationBusy = ref(false);

function requestConfirmation(confirmation: PendingConfirmation) {
    pendingConfirmation.value = confirmation;
}

function closeConfirmation() {
    if (!confirmationBusy.value) pendingConfirmation.value = null;
}

async function confirmPendingAction() {
    const confirmation = pendingConfirmation.value;
    if (!confirmation || confirmationBusy.value) return;
    confirmationBusy.value = true;
    try {
        await confirmation.action();
        pendingConfirmation.value = null;
    } catch (error) {
        pushToast(String(error), 'error', { ttlMs: 5000 });
    } finally {
        confirmationBusy.value = false;
    }
}

const enabledServerCount = computed(() => config.value.servers.filter(server => server.enabled).length);
const intervalError = computed(() => config.value.interval_minutes < 5 ? t('settings.minIntervalError', { min: 5 }) : '');
const stabilityCheckError = computed(() => config.value.stability_check_secs < 60 ? t('settings.minStabilityCheckError', { min: 60 }) : '');
const recentFileGuardError = computed(() => config.value.recent_file_guard_mins < 3 ? t('settings.minRecentFileGuardError', { min: 3 }) : '');
const hasConfigErrors = computed(() => Boolean(intervalError.value || stabilityCheckError.value || recentFileGuardError.value));
function shows(section: 'tasks' | 'strategy' | 'delivery') {
    return props.section === 'all'
        || props.section === section
        || (props.section === 'tasks-strategy' && (section === 'tasks' || section === 'strategy'));
}

function serverDisplayName(server: DeployServer) {
    return server.name || server.host;
}

function rememberOpener() {
    const active = document.activeElement;
    serverManagerOpenerEl = active instanceof HTMLElement ? active : null;
}

function openServerManager() {
    rememberOpener();
    isServerManagerOpen.value = true;
    nextTick(() => {
        serverManagerCloseBtn.value?.focus();
    });
}

function closeServerManager() {
    isServerManagerOpen.value = false;
    nextTick(() => {
        serverManagerOpenerEl?.focus?.();
        serverManagerOpenerEl = null;
    });
}

function trapServerManagerFocus(event: KeyboardEvent) {
    if (event.key !== 'Tab') return;
    const dialog = document.getElementById('settings-server-manager-dialog');
    if (!dialog) return;
    const focusable = dialog.querySelectorAll<HTMLElement>(
        'a[href], area[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled]), [tabindex]:not([tabindex="-1"])'
    );
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
    }
}

function handleServerManagerKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
        event.preventDefault();
        closeServerManager();
        return;
    }
    trapServerManagerFocus(event);
}

// Transient operation feedback flows through the shared toast queue (M01).
// Inline field-level validation (e.g. intervalError, stabilityCheckError)
// continues to render directly under the offending input — see template.

// ── Command Group Management ──────────────────────────────────────────────────
// Built-in groups carry stable ids; their human-facing name is i18n-driven so
// language switches reflect immediately while config persistence stays stable.
type BuiltinCommandKey = 'unzip' | 'uninstall' | 'cleanup' | 'install';
const builtinCommandDescriptors: ReadonlyArray<{ id: string; key: BuiltinCommandKey; commands: string[] }> = [
    {
        id: '__builtin_extract__',
        key: 'unzip',
        commands: ['cd ${remote_target} && tar -zxvf ${filename}.tar.gz'],
    },
    {
        id: '__builtin_uninstall__',
        key: 'uninstall',
        commands: ['cd ${remote_target}/${filename} && echo y | ./integrated_uninstall.sh'],
    },
    {
        id: '__builtin_cleanup__',
        key: 'cleanup',
        commands: ['which omc_uninstall.sh > /dev/null 2>&1 && echo yes | omc_uninstall.sh || true; which hauninstall.sh > /dev/null 2>&1 && printf \'yes\\n\' | hauninstall.sh || true'],
    },
    {
        id: '__builtin_install__',
        key: 'install',
        commands: ['cd ${remote_target}/${filename} && printf \'yes\\ny\\n\' | ./update -f'],
    },
];

const builtinKeyById: Record<string, BuiltinCommandKey> = builtinCommandDescriptors.reduce(
    (acc, descriptor) => {
        acc[descriptor.id] = descriptor.key;
        return acc;
    },
    {} as Record<string, BuiltinCommandKey>,
);

function builtinDisplayName(group: CommandGroup): string {
    const key = builtinKeyById[group.id];
    return key ? t(`settings.builtinCommands.${key}.name`) : group.name;
}

function makeBuiltinGroup(descriptor: typeof builtinCommandDescriptors[number]): CommandGroup {
    return {
        id: descriptor.id,
        name: t(`settings.builtinCommands.${descriptor.key}.name`),
        commands: [...descriptor.commands],
    };
}

const builtinCommandGroups = computed<CommandGroup[]>(() =>
    builtinCommandDescriptors.map(makeBuiltinGroup),
);

const isEditingCommandGroup = ref(false);
const editingCommandGroupIndex = ref(-1);
const commandGroupForm = ref<CommandGroup>({ id: '', name: '', commands: [] });
const newGroupCommand = ref('');

function restoreBuiltinCommandGroups() {
    requestConfirmation({
        title: t('settings.restoreBuiltinTitle'),
        description: t('settings.confirmRestoreBuiltin'),
        confirmLabel: t('settings.restoreBuiltin'),
        tone: 'warning',
        action: async () => {
            const existing = config.value.command_groups;
            for (const builtin of builtinCommandGroups.value) {
                const idx = existing.findIndex(g => g.id === builtin.id);
                if (idx >= 0) {
                    existing[idx] = { ...builtin, commands: [...builtin.commands] };
                } else {
                    existing.push({ ...builtin, commands: [...builtin.commands] });
                }
            }
            await save();
        },
    });
}

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
    requestConfirmation({
        title: t('settings.deleteCommandGroupTitle'),
        description: t('settings.confirmDeleteCommandGroup'),
        confirmLabel: t('settings.deleteTitle'),
        tone: 'danger',
        action: async () => {
            config.value.command_groups.splice(index, 1);
            await save();
        },
    });
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
    const group = config.value.command_groups.find(g => g.id === id);
    if (!group) return id.substring(0, 8);
    return builtinDisplayName(group);
}

// ── Local Script Groups ──────────────────────────────────────────────────────
const isEditingLocalGroup = ref(false);
const editingLocalGroupIndex = ref(-1);
const localGroupForm = ref<LocalCommandGroup>({
  id: '',
  name: '',
  commands: [],
  on_failure: 'continue' as OnFailure,
});
const newLocalGroupCommand = ref('');

function resetLocalGroupForm() {
  localGroupForm.value = { id: '', name: '', commands: [], on_failure: 'continue' };
  newLocalGroupCommand.value = '';
  editingLocalGroupIndex.value = -1;
}

function addLocalGroup() {
  resetLocalGroupForm();
  isEditingLocalGroup.value = true;
}

function editLocalGroup(index: number) {
  const group = config.value.local_command_groups[index];
  localGroupForm.value = {
    id: group.id,
    name: group.name,
    commands: [...group.commands],
    on_failure: group.on_failure,
  };
  editingLocalGroupIndex.value = index;
  newLocalGroupCommand.value = '';
  isEditingLocalGroup.value = true;
}

async function saveLocalGroup() {
  const form = localGroupForm.value;
  if (!form.name.trim()) return;

  if (editingLocalGroupIndex.value >= 0) {
    config.value.local_command_groups[editingLocalGroupIndex.value] = { ...form };
  } else {
    config.value.local_command_groups.push({
      ...form,
      id: crypto.randomUUID(),
    });
  }
  isEditingLocalGroup.value = false;
  resetLocalGroupForm();
  await save();
}

function removeLocalGroup(index: number) {
  requestConfirmation({
    title: t('settings.deleteCommandGroupTitle'),
    description: t('settings.confirmDeleteCommandGroup'),
    confirmLabel: t('settings.deleteTitle'),
    tone: 'danger',
    action: async () => {
      config.value.local_command_groups.splice(index, 1);
      await save();
    },
  });
}

function addLocalGroupCommand() {
  if (newLocalGroupCommand.value.trim()) {
    localGroupForm.value.commands.push(newLocalGroupCommand.value.trim());
    newLocalGroupCommand.value = '';
  }
}

function removeLocalGroupCommand(cmdIndex: number) {
  localGroupForm.value.commands.splice(cmdIndex, 1);
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
    local_script_binding: null,
    post_copy_execution_order: 'local_first',
});

const taskLocalPathInput = computed({
    get: () => getDirectoryInputValue(taskForm.value.local_path),
    set: (value: string) => {
        taskForm.value.local_path = toOptionalDirectoryValue(value);
    },
});

const taskLocalPathPlaceholder = computed(() => getTaskLocalPathPlaceholder(config.value.local_path || ''));
const taskLocalPathHint = computed(() => getTaskLocalPathHint(t('settings.taskLocalPathHint')));

function resetTaskForm() {
    taskForm.value = {
        id: crypto.randomUUID(),
        enabled: true,
        name: '',
        remote_path: '',
        local_path: null,
        rule: { type: 'VersionMatch', value: '' },
        server_bindings: [],
        local_script_binding: null,
        post_copy_execution_order: 'local_first',
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
        local_script_binding: task.local_script_binding
            ? { command_group_ids: [...task.local_script_binding.command_group_ids] }
            : null,
        post_copy_execution_order: task.post_copy_execution_order || 'local_first',
    };
    isEditingTask.value = true;
}

function saveTask() {
    // Trim rule value to remove any leading/trailing whitespace
    const trimmedTask = JSON.parse(JSON.stringify(taskForm.value));
    trimmedTask.rule.value = trimmedTask.rule.value.trim();
    trimmedTask.local_path = toOptionalDirectoryValue(getDirectoryInputValue(trimmedTask.local_path));

    if (editingTaskIndex.value > -1) {
        config.value.tasks[editingTaskIndex.value] = trimmedTask;
    } else {
        config.value.tasks.push(trimmedTask);
    }
    save();
    isEditingTask.value = false;
}

function removeTask(index: number) {
    requestConfirmation({
        title: t('settings.deleteTaskTitle'),
        description: t('settings.confirmDeleteTask'),
        confirmLabel: t('settings.deleteTitle'),
        tone: 'danger',
        action: async () => {
            config.value.tasks.splice(index, 1);
            await save();
        },
    });
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

// --- Local Script Binding ---
function toggleLocalScriptGroup(groupId: string) {
    if (!taskForm.value.local_script_binding) {
        taskForm.value.local_script_binding = { command_group_ids: [] };
    }
    const ids = taskForm.value.local_script_binding.command_group_ids;
    const idx = ids.indexOf(groupId);
    if (idx >= 0) {
        ids.splice(idx, 1);
    } else {
        ids.push(groupId);
    }
    if (ids.length === 0) {
        taskForm.value.local_script_binding = null;
    }
}

function localScriptGroupOrder(groupId: string): number {
    if (!taskForm.value.local_script_binding) return 0;
    const idx = taskForm.value.local_script_binding.command_group_ids.indexOf(groupId);
    return idx >= 0 ? idx + 1 : 0;
}

function moveLocalScriptGroup(index: number, direction: -1 | 1) {
    if (!taskForm.value.local_script_binding) return;
    const ids = taskForm.value.local_script_binding.command_group_ids;
    const newIndex = index + direction;
    if (newIndex < 0 || newIndex >= ids.length) return;
    [ids[index], ids[newIndex]] = [ids[newIndex], ids[index]];
}

function removeLocalScriptGroupFromBinding(groupId: string) {
    if (!taskForm.value.local_script_binding) return;
    const ids = taskForm.value.local_script_binding.command_group_ids;
    const idx = ids.indexOf(groupId);
    if (idx >= 0) ids.splice(idx, 1);
    if (ids.length === 0) {
        taskForm.value.local_script_binding = null;
    }
}

// ── Server Management ─────────────────────────────────────────────────────────
const isEditingServer = ref(false);
const editingServerIndex = ref(-1);
const serverEditorDialog = ref<HTMLElement | null>(null);
const serverEditorCancelBtn = ref<HTMLButtonElement | null>(null);
const serverEditorTargetBindingIndex = ref<number | null>(null);
const reopenManualDeployLogAfterServerEdit = ref(false);
const serverFormError = ref('');
const serverFormTestStatus = ref<{ state: 'idle' | 'testing' | 'ok' | 'error'; message: string }>({
    state: 'idle',
    message: '',
});
let serverEditorOpenerEl: HTMLElement | null = null;
const serverForm = ref({
    id: '',
    enabled: true,
    name: '',
    host: '',
    port: 23333,
    user: 'root',
    password: 'admin_123',
    remote_path: '/root',
    ssh_timeout_secs: 5
});

function resetServerForm() {
    serverForm.value = {
        id: crypto.randomUUID(),
        enabled: true,
        name: '',
        host: '',
        port: 23333,
        user: 'root',
        password: 'admin_123',
        remote_path: '',
        ssh_timeout_secs: 5
    };
    isEditingServer.value = false;
    editingServerIndex.value = -1;
    serverFormError.value = '';
    serverFormTestStatus.value = { state: 'idle', message: '' };
}

function addServer() {
    serverEditorOpenerEl = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    serverEditorTargetBindingIndex.value = null;
    reopenManualDeployLogAfterServerEdit.value = false;
    resetServerForm();
    isServerManagerOpen.value = true;
    isEditingServer.value = true;
}

function editServer(index: number, targetBindingIndex: number | null = null) {
    serverEditorOpenerEl = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    editingServerIndex.value = index;
    serverForm.value = { ...config.value.servers[index] };
    serverEditorTargetBindingIndex.value = targetBindingIndex;
    serverFormError.value = '';
    serverFormTestStatus.value = { state: 'idle', message: '' };
    if (targetBindingIndex === null) isServerManagerOpen.value = true;
    isEditingServer.value = true;
}

function closeServerEditor() {
    if (isSaving.value || serverFormTestStatus.value.state === 'testing') return;
    isEditingServer.value = false;
    serverEditorTargetBindingIndex.value = null;
    serverFormError.value = '';
    if (reopenManualDeployLogAfterServerEdit.value) {
        reopenManualDeployLogAfterServerEdit.value = false;
        nextTick(() => { manualDeployDialogOpen.value = true; });
    }
}

function handleServerEditorKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
        event.preventDefault();
        closeServerEditor();
        return;
    }
    if (event.key !== 'Tab' || !serverEditorDialog.value) return;
    const focusable = serverEditorDialog.value.querySelectorAll<HTMLElement>(
        'input:not([disabled]), select:not([disabled]), button:not([disabled]), [tabindex]:not([tabindex="-1"])'
    );
    if (!focusable.length) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
    }
}

watch(isEditingServer, async open => {
    if (open) {
        await nextTick();
        serverEditorCancelBtn.value?.focus();
    } else {
        await nextTick();
        serverEditorOpenerEl?.focus?.();
        serverEditorOpenerEl = null;
    }
});

function validateServerForm() {
    if (!serverForm.value.host.trim()) {
        serverFormError.value = t('settings.hostRequired');
        pushToast(serverFormError.value, 'warning');
        return false;
    }
    if (!Number.isInteger(serverForm.value.port) || serverForm.value.port < 1 || serverForm.value.port > 65535) {
        serverFormError.value = t('settings.invalidServerPort');
        pushToast(serverFormError.value, 'warning');
        return false;
    }
    serverFormError.value = '';
    return true;
}

async function testServerFormConnection() {
    if (!validateServerForm() || serverFormTestStatus.value.state === 'testing') return;
    serverFormTestStatus.value = { state: 'testing', message: '' };
    try {
        const message = await testSshConnection({ ...serverForm.value });
        serverFormTestStatus.value = { state: 'ok', message };
    } catch (error) {
        serverFormTestStatus.value = { state: 'error', message: String(error) };
    }
}

async function saveServer() {
    if (!validateServerForm()) return;
    const previousServers = config.value.servers.map(server => ({ ...server }));
    const savedServer = { ...serverForm.value };
    if (editingServerIndex.value > -1) {
        config.value.servers[editingServerIndex.value] = savedServer;
    } else {
        config.value.servers.push(savedServer);
    }
    const saved = await save();
    if (!saved) {
        config.value.servers.splice(0, config.value.servers.length, ...previousServers);
        serverFormError.value = t('settings.serverSaveFailedInline');
        return;
    }
    const targetBindingIndex = serverEditorTargetBindingIndex.value;
    if (targetBindingIndex !== null && manualServerBindings.value[targetBindingIndex]) {
        manualServerBindings.value[targetBindingIndex].server_id = savedServer.id;
    }
    invalidateManualPreflight();
    pushToast(
        targetBindingIndex !== null ? t('settings.serverSavedAndSelected') : t('settings.serverSaved'),
        'success',
        { ttlMs: 2500 },
    );
    isEditingServer.value = false;
    serverEditorTargetBindingIndex.value = null;
    if (reopenManualDeployLogAfterServerEdit.value) {
        reopenManualDeployLogAfterServerEdit.value = false;
        nextTick(() => { manualDeployDialogOpen.value = true; });
    }
}

function removeServer(index: number) {
    requestConfirmation({
        title: t('settings.deleteServerTitle'),
        description: t('settings.confirmDeleteServer'),
        confirmLabel: t('settings.deleteTitle'),
        tone: 'danger',
        action: async () => {
            const serverId = config.value.servers[index]?.id;
            config.value.servers.splice(index, 1);
            if (serverId) {
                manualServerBindings.value = manualServerBindings.value.filter(binding => binding.server_id !== serverId);
            }
            await save();
        },
    });
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
    const id = pushToast(t('settings.testing'), 'info', { ttlMs: 0 });
    try {
        for (let i = 0; i < config.value.servers.length; i++) {
            const server = config.value.servers[i];
            if (!server.enabled) continue;
            await testServerConnection(i);
        }
    } finally {
        // Replace the persistent "testing" toast with a brief completion ping
        // so the user knows the run finished.
        dismissToast(id);
    }
}

// ── Manual Deploy ─────────────────────────────────────────────────────────────
interface ManualDeployBindingState extends TaskServerBinding {
    extract_command_group_id: string | null;
}

const manualLocalPath = ref('');
const manualRemotePath = ref('/root');
const manualTransferPolicies: ManualDeployTransferPolicy[] = ['smart', 'always', 'remote_only'];
const manualExtractPolicies: ManualDeployExtractPolicy[] = ['auto', 'force', 'skip'];
const manualTransferPolicy = ref<ManualDeployTransferPolicy>('smart');
const manualExtractPolicy = ref<ManualDeployExtractPolicy>('auto');
const manualExtractDir = ref('${remote_target}/${filename}');
const manualServerBindings = ref<ManualDeployBindingState[]>([]);
const manualDeployMsgType = ref<'info' | 'error' | ''>('');
const manualDeployDialogOpen = ref(false);
const isManualPreflighting = ref(false);
const manualPreflightResults = ref<ManualDeployPreflightResult[]>([]);

const latestManualDeployGroup = computed(() => {
    const session = taskStateStore.latestManualDeploy;
    return session ? taskStateStore.groupDetails[session.task_group_id] ?? null : null;
});

const latestManualDeployRun = computed(() => {
    const session = taskStateStore.latestManualDeploy;
    return latestManualDeployGroup.value?.runs.find(run => run.run_id === session?.run_id) ?? null;
});

const isLatestManualDeployRunning = computed(() => {
    const session = taskStateStore.latestManualDeploy;
    if (!session) return false;
    if (latestManualDeployRun.value) return !latestManualDeployRun.value.finished_at;
    const summary = taskStateStore.groups.find(group => group.task_group_id === session.task_group_id);
    return Boolean(summary && !summary.finished_at);
});

const hasDuplicateManualServers = computed(() => {
    const ids = manualServerBindings.value.map(binding => binding.server_id).filter(Boolean);
    return new Set(ids).size !== ids.length;
});

const hasUnavailableManualServer = computed(() => manualServerBindings.value.some(binding => {
    if (!binding.server_id) return false;
    return !config.value.servers.some(server => server.id === binding.server_id && server.enabled);
}));

function addManualBinding() {
    manualServerBindings.value.push({ server_id: '', command_group_ids: [], extract_command_group_id: null });
    invalidateManualPreflight();
}

function invalidateManualPreflight() {
    manualPreflightResults.value = [];
}

function addManualServer() {
    const targetBindingIndex = manualServerBindings.value.length;
    addManualBinding();
    serverEditorOpenerEl = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    resetServerForm();
    serverEditorTargetBindingIndex.value = targetBindingIndex;
    isEditingServer.value = true;
}

function editManualBindingServer(binding: TaskServerBinding, bindingIndex: number) {
    const serverIndex = config.value.servers.findIndex(server => server.id === binding.server_id);
    if (serverIndex >= 0) editServer(serverIndex, bindingIndex);
}

async function editManualDeployServer(serverId: string) {
    manualDeployDialogOpen.value = false;
    await nextTick();
    const serverIndex = config.value.servers.findIndex(server => server.id === serverId);
    const bindingIndex = manualServerBindings.value.findIndex(binding => binding.server_id === serverId);
    if (serverIndex >= 0) {
        reopenManualDeployLogAfterServerEdit.value = true;
        editServer(serverIndex, bindingIndex >= 0 ? bindingIndex : -1);
    }
}

function availableManualServers(bindingIndex: number) {
    const currentId = manualServerBindings.value[bindingIndex]?.server_id;
    const selectedElsewhere = new Set(
        manualServerBindings.value
            .filter((_, index) => index !== bindingIndex)
            .map(binding => binding.server_id)
            .filter(Boolean),
    );
    return config.value.servers.filter(server => (
        (server.enabled || server.id === currentId)
        && (server.id === currentId || !selectedElsewhere.has(server.id))
    ));
}

function removeManualBinding(index: number) {
    manualServerBindings.value.splice(index, 1);
    invalidateManualPreflight();
}

function toggleManualBindingGroup(binding: ManualDeployBindingState, groupId: string) {
    const idx = binding.command_group_ids.indexOf(groupId);
    if (idx > -1) {
        binding.command_group_ids.splice(idx, 1);
        if (binding.extract_command_group_id === groupId) binding.extract_command_group_id = null;
    } else {
        binding.command_group_ids.push(groupId);
        if (!binding.extract_command_group_id) binding.extract_command_group_id = groupId;
    }
    invalidateManualPreflight();
}

function manualBindingGroupOrder(binding: TaskServerBinding, groupId: string) {
    const idx = binding.command_group_ids.indexOf(groupId);
    return idx > -1 ? idx + 1 : null;
}

function moveManualBindingGroup(binding: ManualDeployBindingState, index: number, direction: -1 | 1) {
    const targetIndex = index + direction;
    if (targetIndex < 0 || targetIndex >= binding.command_group_ids.length) return;
    const [groupId] = binding.command_group_ids.splice(index, 1);
    binding.command_group_ids.splice(targetIndex, 0, groupId);
    invalidateManualPreflight();
}

function removeManualBindingGroupById(binding: ManualDeployBindingState, groupId: string) {
    const idx = binding.command_group_ids.indexOf(groupId);
    if (idx > -1) binding.command_group_ids.splice(idx, 1);
    if (binding.extract_command_group_id === groupId) binding.extract_command_group_id = null;
    invalidateManualPreflight();
}

const manualExtractConfigurationValid = computed(() => manualExtractPolicy.value === 'skip'
    || (Boolean(manualExtractDir.value.trim())
        && manualServerBindings.value.every(binding => Boolean(binding.extract_command_group_id)
            && binding.command_group_ids.includes(binding.extract_command_group_id!))));

const manualDeployInputValid = computed(() => {
    const localPathValid = manualTransferPolicy.value === 'remote_only' || Boolean(manualLocalPath.value.trim());
    return localPathValid
        && Boolean(manualRemotePath.value.trim())
        && manualServerBindings.value.length > 0
        && manualServerBindings.value.every(binding => Boolean(binding.server_id))
        && !hasDuplicateManualServers.value
        && !hasUnavailableManualServer.value
        && Boolean(manualExtractConfigurationValid.value);
});

const canManualDeploy = computed(() => {
    return !appStore.isManualDeploying
        && !isManualPreflighting.value
        && manualDeployInputValid.value
        && !isLatestManualDeployRunning.value;
});

function buildManualDeployRequest(): StartManualDeployTaskRequest {
    const identityPath = manualLocalPath.value.trim() || manualRemotePath.value.trim();
    const folderName = identityPath.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || 'manual';
    const bindings = manualServerBindings.value
        .filter(binding => config.value.servers.some(server => server.id === binding.server_id && server.enabled))
        .map(binding => ({
            server_id: binding.server_id,
            command_group_ids: [...binding.command_group_ids],
            extract_command_group_id: manualExtractPolicy.value === 'skip'
                ? null
                : binding.extract_command_group_id,
        }));
    return {
        task_group_id: null,
        display_name: folderName,
        folder_name: folderName,
        local_path: manualLocalPath.value.trim(),
        remote_path: manualRemotePath.value.trim(),
        transfer_policy: manualTransferPolicy.value,
        extract_policy: manualExtractPolicy.value,
        extract_dir: manualExtractDir.value.trim(),
        bindings,
    };
}

async function handleManualPreflight() {
    if (!manualDeployInputValid.value || appStore.isManualDeploying || isLatestManualDeployRunning.value) return;
    isManualPreflighting.value = true;
    appStore.manualDeployMsg = '';
    manualDeployMsgType.value = '';
    try {
        manualPreflightResults.value = await preflightManualDeploy(buildManualDeployRequest());
        appStore.manualDeployMsg = t('settings.manualPreflightComplete', { count: manualPreflightResults.value.length });
        manualDeployMsgType.value = 'info';
    } catch (error) {
        manualPreflightResults.value = [];
        appStore.manualDeployMsg = t('settings.manualPreflightFailed', { error: String(error) });
        manualDeployMsgType.value = 'error';
    } finally {
        isManualPreflighting.value = false;
    }
}

async function handleManualDeploy() {
    if (!canManualDeploy.value) return;

    appStore.isManualDeploying = true;
    appStore.manualDeployMsg = '';
    manualDeployMsgType.value = '';

    try {
        const request = buildManualDeployRequest();
        await taskStateStore.startManualDeploy(request);

        // The command only queues the run; success or failure shows up in the
        // execution log, so never claim the deployment already succeeded here.
        appStore.manualDeployMsg = t('settings.deployStarted', { count: request.bindings.length });
        manualDeployMsgType.value = 'info';
        manualDeployDialogOpen.value = true;
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

async function copyToClipboard(text: string) {
    try {
        await writeText(text);
        pushToast(t('settings.pathCopied'), 'success', { ttlMs: 2000 });
    } catch (e) {
        console.error('Failed to copy', e);
    }
}

function handleDirectoryPickError(error: string, mode: 'directory' | 'file' = 'directory') {
    const message = mode === 'file'
        ? t('settings.selectFileFailed', { error })
        : t('settings.selectDirectoryFailed', { error });
    pushToast(message, 'error', { ttlMs: 4000 });
}

async function load() {
    try {
        await configStore.ensureLoaded();
        // Auto-populate built-in groups when none are configured (e.g. fresh install)
        if (config.value.command_groups.length === 0) {
            config.value.command_groups = builtinCommandGroups.value.map(g => ({ ...g, commands: [...g.commands] }));
        }
    } catch (e) {
        console.error(e);
    }
}

async function save(): Promise<boolean> {
    if (hasConfigErrors.value) {
        pushToast(t('settings.toast.invalid'), 'warning');
        return false;
    }
    if (isSaving.value) return false;
    isSaving.value = true;
    try {
        await configStore.saveSync();
        pushToast(t('settings.toast.saved'), 'success');
        return true;
    } catch (e) {
        pushToast(t('settings.toast.saveError', { error: String(e) }), 'error', { ttlMs: 5000 });
        return false;
    } finally {
        isSaving.value = false;
    }
}

// Watch for rule type changes and auto-fill default value for DateMatch
watch(() => taskForm.value.rule.type, (newType) => {
    if (newType === 'DateMatch' && !taskForm.value.rule.value) {
        taskForm.value.rule.value = '%y%m%d';
    }
});

onMounted(load);
</script>

<template>
  <div class="h-full min-h-0 overflow-y-auto overscroll-y-none bg-slate-50">
  <div
    v-if="config"
    class="sync-console-workspace min-h-full w-full p-6 pb-24"
    :class="props.section === 'tasks-strategy'
      ? 'sync-tasks-strategy-stack space-y-4'
      : props.section === 'delivery'
        ? 'sync-delivery-stack space-y-4'
        : 'space-y-6'"
  >
    <!-- Local Path -->
    <div
      v-if="shows('strategy')"
      class="sync-strategy-card overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm"
    >
      <div class="flex items-center gap-3 border-b border-slate-200 bg-slate-50 px-5 py-4">
        <div class="w-8 h-8 rounded-lg bg-amber-100 text-amber-600 flex items-center justify-center shrink-0">
          <FolderOpen class="w-4 h-4" />
        </div>
        <h3 class="text-base font-semibold text-slate-800">{{ t('sync.tabs.strategy') }}</h3>
      </div>
      <div class="space-y-4 p-5">
      <div>
        <span class="block text-sm font-medium text-slate-600 mb-1">
          {{ t('settings.localPath') }}
          <span class="text-rose-500" :aria-label="t('settings.required.indicator')">*</span>
        </span>
        <DirectoryPathInput
          v-model="config.local_path"
          :title="t('settings.selectDirectory')"
          @pick-error="handleDirectoryPickError"
        />
        <p class="text-xs text-slate-400 mt-1">{{ t('settings.localPathDesc') }}</p>
      </div>
      </div>
    </div>

    <!-- Tasks Management -->
    <div
      v-if="shows('tasks')"
      class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm"
    >
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
        <Empty
          v-if="config.tasks.length === 0"
          :icon="ListChecks"
          :description="t('settings.noTasks')"
          :action-label="t('settings.addTask')"
          @action="addTask"
        />

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
              <button @click="editTask(idx)" class="p-1.5 text-slate-500 hover:text-amber-600 hover:bg-amber-50 rounded transition-colors" :title="t('settings.edit')" :aria-label="t('settings.edit')">
                <Edit class="w-4 h-4" />
              </button>
              <button @click="removeTask(idx)" class="p-1.5 text-slate-500 hover:text-red-600 hover:bg-red-50 rounded transition-colors" :title="t('settings.deleteTitle')" :aria-label="t('settings.deleteTitle')">
                <Trash2 class="w-4 h-4" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Task Edit Modal -->
    <div v-if="shows('tasks') && isEditingTask" class="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
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
            <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.taskLocalPath') }}</label>
            <DirectoryPathInput
              v-model="taskLocalPathInput"
              :placeholder="taskLocalPathPlaceholder"
              :title="t('settings.selectDirectory')"
              @pick-error="handleDirectoryPickError"
            />
            <p class="text-xs text-slate-400 mt-1">{{ taskLocalPathHint }}</p>
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

          <!-- Post-Copy Execution Order & Local Script Binding -->
          <div v-if="config.local_command_groups.length > 0" class="space-y-4 pt-4 border-t border-slate-200">
            <div>
              <label class="block text-sm font-medium text-slate-700 mb-2">{{ t('settings.postCopyExecutionOrder') }}</label>
              <div class="flex gap-2">
                <button v-for="order in (['local_first', 'remote_first', 'parallel'] as const)" :key="order"
                  type="button"
                  @click="taskForm.post_copy_execution_order = order"
                  class="flex-1 px-3 py-2 rounded-lg text-sm font-medium border transition-colors"
                  :class="taskForm.post_copy_execution_order === order
                    ? 'bg-teal-50 border-teal-300 text-teal-700'
                    : 'bg-white border-slate-200 text-slate-600 hover:border-slate-300'">
                  {{ order === 'local_first' ? t('settings.localFirst') : order === 'remote_first' ? t('settings.remoteFirst') : t('settings.parallel') }}
                </button>
              </div>
            </div>

            <!-- Local Script Binding -->
            <div>
              <label class="block text-sm font-medium text-slate-700 mb-2">{{ t('settings.localScriptBinding') }}</label>
              <div class="flex flex-wrap gap-2">
                <button v-for="group in config.local_command_groups" :key="group.id"
                  type="button"
                  @click="toggleLocalScriptGroup(group.id)"
                  class="px-3 py-1.5 rounded-lg text-sm font-medium border transition-colors"
                  :class="localScriptGroupOrder(group.id) > 0
                    ? 'bg-teal-50 border-teal-300 text-teal-700'
                    : 'bg-white border-slate-200 text-slate-600 hover:border-slate-300'">
                  <span v-if="localScriptGroupOrder(group.id) > 0" class="text-xs bg-teal-600 text-white rounded-full w-4 h-4 inline-flex items-center justify-center mr-1">
                    {{ localScriptGroupOrder(group.id) }}
                  </span>
                  {{ group.name }}
                </button>
              </div>

              <!-- Bound groups execution order list -->
              <div v-if="taskForm.local_script_binding && taskForm.local_script_binding.command_group_ids.length > 0" class="mt-3 space-y-1.5">
                <div v-for="(gid, idx) in taskForm.local_script_binding.command_group_ids" :key="gid"
                     class="flex items-center gap-2 px-3 py-2 rounded-lg bg-slate-50 border border-slate-200 text-sm">
                  <span class="w-5 h-5 rounded-full bg-teal-600 text-white text-xs flex items-center justify-center shrink-0">{{ idx + 1 }}</span>
                  <span class="flex-1 text-slate-700">{{ config.local_command_groups.find(g => g.id === gid)?.name ?? gid }}</span>
                  <button type="button" @click="moveLocalScriptGroup(idx, -1)" :disabled="idx === 0" class="p-1 text-slate-400 hover:text-slate-600 disabled:opacity-30">
                    <ArrowUp class="w-3.5 h-3.5" />
                  </button>
                  <button type="button" @click="moveLocalScriptGroup(idx, 1)" :disabled="idx === taskForm.local_script_binding!.command_group_ids.length - 1" class="p-1 text-slate-400 hover:text-slate-600 disabled:opacity-30">
                    <ArrowDown class="w-3.5 h-3.5" />
                  </button>
                  <button type="button" @click="removeLocalScriptGroupFromBinding(gid)" class="p-1 text-red-400 hover:text-red-500">
                    <X class="w-3.5 h-3.5" />
                  </button>
                </div>
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
    <div
      v-if="shows('strategy')"
      class="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm"
    >
      <div class="flex items-center gap-3 border-b border-slate-200 bg-white px-5 py-3">
        <div class="w-8 h-8 rounded-lg bg-emerald-100 text-emerald-600 flex items-center justify-center shrink-0">
          <Clock class="w-4 h-4" />
        </div>
        <h3 class="text-sm font-semibold text-slate-700">{{ t('settings.scanTime') }}</h3>
      </div>
      <div class="sync-scan-timing-stack space-y-5 p-5">
      <div class="space-y-3">
        <label for="settings-scan-interval" class="block text-sm font-semibold text-slate-700">
          {{ t('settings.scanInterval') }}
          <span class="text-rose-500" :aria-label="t('settings.required.indicator')">*</span>
        </label>
        <div class="flex flex-wrap items-center gap-2">
          <input
            id="settings-scan-interval"
            v-model.number="config.interval_minutes" type="number" min="5"
            class="w-28 h-10 px-3 border rounded-lg text-slate-700 focus:ring-2 outline-none"
            :class="intervalError ? 'border-red-400 focus:ring-red-200 focus:border-red-400 bg-red-50' : 'border-slate-300 focus:ring-blue-500 focus:border-blue-500'"
            :aria-invalid="intervalError ? 'true' : 'false'"
            aria-describedby="settings-scan-interval-help" />
          <span class="text-sm font-medium text-slate-500">{{ t('settings.minutes') }}</span>
          <span id="settings-scan-interval-help" class="text-xs leading-5 text-slate-400">{{ t('settings.field.interval.helpMin') }}</span>
        </div>
        <p v-if="intervalError" class="text-xs leading-5 text-red-500" role="alert">{{ intervalError }}</p>
      </div>

      <div class="space-y-4 border-t border-slate-100 pt-5">
        <div>
          <h4 class="text-sm font-semibold text-slate-700">{{ t('settings.stabilityCheck') }}</h4>
          <p class="mt-1 text-xs leading-5 text-slate-500">{{ t('settings.stabilityCheckDesc') }}</p>
        </div>
        <div class="grid grid-cols-[repeat(auto-fit,minmax(220px,1fr))] gap-5">
          <div class="min-w-0 space-y-3">
            <label for="settings-recent-file-guard" class="block text-sm font-medium text-slate-700">{{ t('settings.recentFileGuard') }}</label>
            <div class="flex flex-wrap items-center gap-2">
              <input
                id="settings-recent-file-guard"
                v-model.number="config.recent_file_guard_mins" type="number" min="3"
                class="w-28 h-10 px-3 border rounded-lg text-slate-700 focus:ring-2 outline-none"
                :class="recentFileGuardError ? 'border-red-400 focus:ring-red-200 focus:border-red-400 bg-red-50' : 'border-slate-300 focus:ring-blue-500 focus:border-blue-500'"
                :aria-invalid="recentFileGuardError ? 'true' : 'false'"
                aria-describedby="settings-recent-file-guard-help" />
              <span class="text-sm font-medium text-slate-500">{{ t('settings.minutes') }}</span>
              <span class="text-xs leading-5 text-slate-400">{{ t('settings.field.guard.helpMin') }}</span>
            </div>
            <p v-if="recentFileGuardError" class="text-xs leading-5 text-red-500" role="alert">{{ recentFileGuardError }}</p>
            <p id="settings-recent-file-guard-help" class="text-xs leading-5 text-slate-500">{{ t('settings.recentFileGuardDesc') }}</p>
            <p class="text-xs leading-5 text-slate-400">{{ t('settings.recentFileGuardHint') }}</p>
          </div>
          <div class="min-w-0 space-y-3">
            <label for="settings-stability-check-secs" class="block text-sm font-medium text-slate-700">{{ t('settings.stabilityCheckSeconds') }}</label>
            <div class="flex flex-wrap items-center gap-2">
              <input
                id="settings-stability-check-secs"
                v-model.number="config.stability_check_secs" type="number" min="60"
                class="w-28 h-10 px-3 border rounded-lg text-slate-700 focus:ring-2 outline-none"
                :class="stabilityCheckError ? 'border-red-400 focus:ring-red-200 focus:border-red-400 bg-red-50' : 'border-slate-300 focus:ring-blue-500 focus:border-blue-500'"
                :aria-invalid="stabilityCheckError ? 'true' : 'false'"
                aria-describedby="settings-stability-check-secs-help" />
              <span class="text-sm font-medium text-slate-500">{{ t('settings.seconds') }}</span>
              <span class="text-xs leading-5 text-slate-400">{{ t('settings.field.stability.helpMin') }}</span>
            </div>
            <p v-if="stabilityCheckError" class="text-xs leading-5 text-red-500" role="alert">{{ stabilityCheckError }}</p>
            <p id="settings-stability-check-secs-help" class="text-xs leading-5 text-slate-400">{{ t('settings.stabilityCheckHint') }}</p>
          </div>
        </div>
      </div>

      <div class="flex items-start justify-between gap-5 border-t border-slate-100 pt-5">
        <div class="min-w-0">
          <label for="settings-fallback-recent-package" class="text-sm font-semibold text-slate-700">
            {{ t('settings.fallbackRecentPackage') }}
          </label>
          <p id="settings-fallback-recent-package-desc" class="mt-1 max-w-3xl text-xs leading-5 text-slate-500">
            {{ t('settings.fallbackRecentPackageDesc') }}
          </p>
        </div>
        <label class="relative inline-flex h-11 shrink-0 cursor-pointer items-center">
          <input
            id="settings-fallback-recent-package"
            v-model="config.fallback_recent_package_enabled"
            type="checkbox"
            class="peer sr-only"
            :aria-label="t('settings.fallbackRecentPackage')"
            aria-describedby="settings-fallback-recent-package-desc"
          >
          <span class="h-6 w-11 rounded-full bg-slate-200 peer-focus-visible:ring-4 peer-focus-visible:ring-blue-200 peer-checked:bg-blue-600 after:absolute after:left-[2px] after:top-[10px] after:h-5 after:w-5 after:rounded-full after:border after:border-slate-300 after:bg-white after:transition-transform after:content-[''] peer-checked:after:translate-x-full motion-reduce:after:transition-none"></span>
        </label>
      </div>

      <fieldset class="space-y-3 border-t border-slate-100 pt-5" aria-describedby="settings-copy-mode-desc">
        <legend class="text-sm font-semibold text-slate-700">{{ t('settings.copyMode') }}</legend>
        <p id="settings-copy-mode-desc" class="text-xs leading-5 text-slate-500">{{ t('settings.copyModeDesc') }}</p>
        <div class="grid gap-3 sm:grid-cols-2">
          <label
            class="relative flex min-h-24 cursor-pointer items-start gap-3 rounded-xl border p-4 transition-colors"
            :class="config.copy_mode === 'built_in' ? 'border-blue-500 bg-blue-50/70' : 'border-slate-200 bg-white hover:border-slate-300 hover:bg-slate-50'"
          >
            <input v-model="config.copy_mode" type="radio" name="copy-mode" value="built_in" class="peer sr-only">
            <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-blue-100 text-blue-600" aria-hidden="true">
              <Cpu class="h-4 w-4" />
            </span>
            <span class="min-w-0">
              <span class="flex flex-wrap items-center gap-2 text-sm font-semibold text-slate-800">
                {{ t('settings.copyModeBuiltIn') }}
                <span class="rounded-full bg-blue-100 px-2 py-0.5 text-[10px] font-semibold text-blue-700">{{ t('settings.copyModeRecommended') }}</span>
              </span>
              <span class="mt-1 block text-xs leading-5 text-slate-500">{{ t('settings.copyModeBuiltInDesc') }}</span>
            </span>
            <span class="ml-auto flex h-5 w-5 shrink-0 items-center justify-center rounded-full border" :class="config.copy_mode === 'built_in' ? 'border-blue-600 bg-blue-600 text-white' : 'border-slate-300 text-transparent'" aria-hidden="true">
              <Check class="h-3 w-3" />
            </span>
            <span class="pointer-events-none absolute inset-0 rounded-xl peer-focus-visible:ring-2 peer-focus-visible:ring-blue-500 peer-focus-visible:ring-offset-2"></span>
          </label>

          <label
            class="relative flex min-h-24 cursor-pointer items-start gap-3 rounded-xl border p-4 transition-colors"
            :class="config.copy_mode === 'windows_shell' ? 'border-blue-500 bg-blue-50/70' : 'border-slate-200 bg-white hover:border-slate-300 hover:bg-slate-50'"
          >
            <input v-model="config.copy_mode" type="radio" name="copy-mode" value="windows_shell" class="peer sr-only">
            <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-slate-100 text-slate-600" aria-hidden="true">
              <Monitor class="h-4 w-4" />
            </span>
            <span class="min-w-0">
              <span class="block text-sm font-semibold text-slate-800">{{ t('settings.copyModeWindows') }}</span>
              <span class="mt-1 block text-xs leading-5 text-slate-500">{{ t('settings.copyModeWindowsDesc') }}</span>
            </span>
            <span class="ml-auto flex h-5 w-5 shrink-0 items-center justify-center rounded-full border" :class="config.copy_mode === 'windows_shell' ? 'border-blue-600 bg-blue-600 text-white' : 'border-slate-300 text-transparent'" aria-hidden="true">
              <Check class="h-3 w-3" />
            </span>
            <span class="pointer-events-none absolute inset-0 rounded-xl peer-focus-visible:ring-2 peer-focus-visible:ring-blue-500 peer-focus-visible:ring-offset-2"></span>
          </label>
        </div>
        <p v-if="config.copy_mode === 'windows_shell'" class="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs leading-5 text-amber-800">
          {{ t('settings.copyModeWindowsNote') }}
        </p>
      </fieldset>

      <div class="space-y-3 border-t border-slate-100 pt-5" :class="config.copy_mode === 'windows_shell' ? 'opacity-60' : ''">
        <div>
          <label for="settings-copy-buffer-size" class="block text-sm font-semibold text-slate-700">{{ t('settings.copyBufferSize') }}</label>
          <p class="mt-1 text-xs leading-5 text-slate-500">{{ t('settings.copyBufferSizeDesc') }}</p>
          <p v-if="config.copy_mode === 'windows_shell'" class="mt-1 text-xs leading-5 text-slate-500">{{ t('settings.copyBufferSizeBuiltInOnly') }}</p>
        </div>
        <select id="settings-copy-buffer-size" v-model.number="config.copy_buffer_size_kb"
                :disabled="config.copy_mode === 'windows_shell'"
                class="w-44 h-10 px-3 border border-slate-300 rounded-lg text-slate-700 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none bg-white disabled:cursor-not-allowed">
          <option :value="64">64 KB</option>
          <option :value="256">256 KB</option>
          <option :value="1024">1 MB</option>
          <option :value="4096">4 MB{{ locale === 'zh' ? '（推荐）' : ' (recommended)' }}</option>
          <option :value="8192">8 MB</option>
          <option :value="16384">16 MB</option>
        </select>
      </div>

      <div class="space-y-4 border-t border-slate-100 pt-5">
        <h4 class="text-sm font-semibold text-slate-700">{{ t('settings.timeRanges') }}</h4>
        <p class="text-xs leading-5 text-slate-500">{{ t('settings.timeRangesDesc') }}</p>
        <div class="flex items-center gap-3">
          <label for="settings-new-time-range" class="sr-only">{{ t('settings.timeRanges') }}</label>
          <input id="settings-new-time-range" v-model="newTimeRange" @keyup.enter="addTimeRange" placeholder="09:00-18:00"
            class="flex-1 h-10 px-3 border border-slate-300 rounded-lg text-slate-700 placeholder:text-slate-400 focus:ring-2 focus:ring-blue-500 outline-none" />
          <button @click="addTimeRange" class="h-10 w-10 shrink-0 bg-slate-100 hover:bg-slate-200 rounded-lg text-slate-600 flex items-center justify-center transition-colors" :aria-label="t('settings.addTimeRange')" :title="t('settings.addTimeRange')">
            <Plus class="w-5 h-5" />
          </button>
        </div>
        <Empty
          v-if="config.time_ranges.length === 0"
          :icon="Clock"
          :description="t('settings.timeRangesDesc')"
        />
        <div v-else class="flex flex-wrap gap-2">
          <div v-for="(range, i) in config.time_ranges" :key="i"
            class="bg-amber-50 text-amber-700 px-3 py-1.5 rounded-full text-sm font-medium border border-amber-100 flex items-center gap-2">
            {{ range }}
            <button @click="removeTimeRange(i)" class="hover:text-amber-900" :aria-label="t('settings.deleteTitle')" :title="t('settings.deleteTitle')"><Trash2 class="w-3 h-3" /></button>
          </div>
        </div>
      </div>
      </div>
    </div>

    <!-- File Filters -->
    <div
      v-if="shows('strategy')"
      class="grid grid-cols-1 gap-4 md:grid-cols-2"
    >
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
            <label for="settings-new-ext" class="sr-only">{{ t('settings.fileExtensions') }}</label>
            <input id="settings-new-ext" v-model="newExt" @keyup.enter="addExt" placeholder="exe"
              class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
            <button @click="addExt" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600" :aria-label="t('settings.fileExtensions')" :title="t('settings.fileExtensions')"><Plus class="w-5 h-5" /></button>
          </div>
          <div class="flex flex-wrap gap-2">
            <div v-for="(ext, i) in config.file_extensions" :key="i"
              class="bg-indigo-50 text-indigo-700 px-3 py-1 rounded-full text-sm font-medium border border-indigo-100 flex items-center gap-2">
              {{ ext }}
              <button @click="removeExt(i)" class="hover:text-indigo-900" :aria-label="t('settings.deleteTitle')" :title="t('settings.deleteTitle')"><Trash2 class="w-3 h-3" /></button>
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
            <label for="settings-new-include" class="sr-only">{{ t('settings.filenameKeywords') }}</label>
            <input id="settings-new-include" v-model="newInclude" @keyup.enter="addInclude" placeholder="UMS"
              class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
            <button @click="addInclude" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600" :aria-label="t('settings.filenameKeywords')" :title="t('settings.filenameKeywords')"><Plus class="w-5 h-5" /></button>
          </div>
          <div class="flex flex-wrap gap-2">
            <div v-for="(inc, i) in config.filename_includes" :key="i"
              class="bg-purple-50 text-purple-700 px-3 py-1 rounded-full text-sm font-medium border border-purple-100 flex items-center gap-2">
              {{ inc }}
              <button @click="removeInclude(i)" class="hover:text-purple-900" :aria-label="t('settings.deleteTitle')" :title="t('settings.deleteTitle')"><Trash2 class="w-3 h-3" /></button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Deploy Settings -->
    <div v-if="shows('delivery')" class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
      <div class="px-6 py-4 bg-slate-50 border-b border-slate-200 flex items-center justify-between gap-3">
        <div class="flex items-center gap-3">
          <div class="w-8 h-8 rounded-lg bg-rose-100 text-rose-600 flex items-center justify-center shrink-0">
            <Server class="w-4 h-4" />
          </div>
          <h3 class="text-base font-semibold text-slate-700">{{ t('settings.remoteDeployment') }}</h3>
        </div>
        <label class="relative inline-flex items-center cursor-pointer" :title="t('settings.tooltip.deployEnabled')">
          <input type="checkbox" v-model="config.deploy_enabled" class="sr-only peer">
          <div class="w-11 h-6 bg-slate-200 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-blue-300 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600 motion-reduce:after:transition-none"></div>
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

          <Empty
            v-if="config.servers.length === 0"
            :icon="Server"
            :description="t('settings.noServers')"
            :action-label="t('settings.addServer')"
            @action="addServer"
          />

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
        <div
          v-if="isServerManagerOpen"
          class="fixed inset-0 bg-black/50 flex items-center justify-center z-[55] p-4"
          @click.self="closeServerManager"
        >
          <div
            id="settings-server-manager-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="settings-server-manager-title"
            aria-describedby="settings-server-manager-desc"
            class="bg-white rounded-2xl p-6 w-full max-w-5xl shadow-[0_20px_70px_rgba(15,23,42,0.18)] max-h-[86vh] overflow-hidden flex flex-col"
            @keydown="handleServerManagerKeydown"
          >
            <div class="flex items-center justify-between gap-4 mb-4">
              <div>
                <h3 id="settings-server-manager-title" class="text-lg font-bold text-slate-950">{{ t('settings.serverDetailsTitle') }}</h3>
                <p id="settings-server-manager-desc" class="text-sm text-slate-500 mt-1">{{ t('settings.serverDetailsDesc') }}</p>
              </div>
              <div class="flex items-center gap-2">
                <button @click="testAllServers" v-if="config.servers.length > 0"
                  class="text-xs text-slate-600 hover:text-slate-800 flex items-center gap-1 font-medium bg-slate-100 hover:bg-slate-200 px-3 py-1.5 rounded-lg transition-colors">
                  <Server class="w-3 h-3" /> {{ t('settings.testAll') }}
                </button>
                <button @click="addServer"
                  class="text-xs text-blue-600 hover:text-blue-800 flex items-center gap-1 font-medium bg-blue-50 hover:bg-blue-100 px-3 py-1.5 rounded-lg transition-colors">
                  <Plus class="w-3 h-3" /> {{ t('settings.addServer') }}
                </button>
                <button
                  ref="serverManagerCloseBtn"
                  @click="closeServerManager"
                  class="px-3 py-1.5 text-slate-500 hover:bg-slate-100 rounded-lg transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2"
                >{{ t('settings.close') }}</button>
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
        <Teleport to="body">
          <div v-if="isEditingServer" class="fixed inset-0 bg-slate-950/55 flex items-center justify-center z-[80] p-4" @click.self="closeServerEditor">
          <div
            ref="serverEditorDialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="settings-server-editor-title"
            aria-describedby="settings-server-editor-description"
            class="bg-white rounded-2xl p-6 w-full max-w-lg shadow-[0_24px_80px_rgba(15,23,42,0.28)] max-h-[88vh] overflow-y-auto"
            @keydown.stop="handleServerEditorKeydown"
          >
            <h3 id="settings-server-editor-title" class="text-lg font-bold text-slate-950">{{ editingServerIndex > -1 ? t('settings.editServer') : t('settings.addServer') }}</h3>
            <p id="settings-server-editor-description" class="mt-1 mb-5 text-sm leading-6 text-slate-600">
              {{ editingServerIndex > -1 ? t('settings.serverGlobalEditNotice') : t('settings.serverCreateNotice') }}
            </p>
            <div class="space-y-4">
              <div class="grid grid-cols-3 gap-4">
                <div class="col-span-2">
                  <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.host') }} <span class="text-red-500">*</span></label>
                  <input v-model="serverForm.host" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" placeholder="192.168.1.100" />
                </div>
                <div>
                  <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.port') }}</label>
                  <input v-model.number="serverForm.port" type="number" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" />
                </div>
              </div>
              <div>
                <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.serverName') }}</label>
                <input v-model="serverForm.name" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" :placeholder="serverForm.host || t('settings.serverNamePlaceholder')" />
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
                <input v-model="serverForm.remote_path" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" placeholder="/root" />
                <p v-if="serverEditorTargetBindingIndex !== null" class="mt-1 text-xs leading-5 text-slate-500">{{ t('settings.manualRemotePathOverrideHint') }}</p>
              </div>
              <div>
                <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.sshTimeout') }}</label>
                <select v-model.number="serverForm.ssh_timeout_secs" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none bg-white">
                  <option :value="1">1 {{ t('settings.seconds') }}</option>
                  <option :value="3">3 {{ t('settings.seconds') }}</option>
                  <option :value="5">5 {{ t('settings.seconds') }}</option>
                  <option :value="10">10 {{ t('settings.seconds') }}</option>
                  <option :value="30">30 {{ t('settings.seconds') }}</option>
                  <option :value="60">60 {{ t('settings.seconds') }}</option>
                </select>
              </div>
              <p v-if="serverFormError" role="alert" class="rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-sm text-rose-700">{{ serverFormError }}</p>
              <p
                v-if="serverFormTestStatus.state !== 'idle'"
                :role="serverFormTestStatus.state === 'error' ? 'alert' : 'status'"
                class="rounded-lg border px-3 py-2 text-sm break-words"
                :class="serverFormTestStatus.state === 'ok'
                  ? 'border-emerald-200 bg-emerald-50 text-emerald-700'
                  : serverFormTestStatus.state === 'error'
                    ? 'border-rose-200 bg-rose-50 text-rose-700'
                    : 'border-blue-200 bg-blue-50 text-blue-700'"
              >
                {{ serverFormTestStatus.state === 'testing' ? t('settings.testing') : serverFormTestStatus.message }}
              </p>
            </div>
            <div class="flex flex-wrap justify-end gap-3 mt-8 pt-4 border-t border-slate-100">
              <button ref="serverEditorCancelBtn" type="button" @click="closeServerEditor" :disabled="isSaving || serverFormTestStatus.state === 'testing'" class="min-h-11 cursor-pointer px-4 py-2 text-slate-600 hover:bg-slate-100 rounded-lg font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-500/40 disabled:cursor-not-allowed disabled:opacity-60">{{ t('console.cancel') }}</button>
              <button type="button" @click="testServerFormConnection" :disabled="isSaving || serverFormTestStatus.state === 'testing'" class="min-h-11 cursor-pointer px-4 py-2 border border-blue-200 bg-blue-50 text-blue-700 rounded-lg hover:bg-blue-100 font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50 disabled:cursor-not-allowed disabled:opacity-60">
                {{ serverFormTestStatus.state === 'testing' ? t('settings.testing') : t('settings.testConnection') }}
              </button>
              <button type="button" @click="saveServer" :disabled="isSaving || serverFormTestStatus.state === 'testing'" class="min-h-11 cursor-pointer px-6 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 font-medium transition-colors shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-60">
                {{ isSaving ? t('settings.saving') : serverEditorTargetBindingIndex !== null ? t('settings.saveAndUse') : t('settings.save') }}
              </button>
            </div>
          </div>
          </div>
        </Teleport>

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
            <div class="flex items-center gap-2 shrink-0">
              <button @click="restoreBuiltinCommandGroups" class="text-xs text-amber-600 hover:text-amber-800 flex items-center gap-1 font-medium bg-amber-50 hover:bg-amber-100 px-3 py-1.5 rounded-lg transition-colors">
                <RotateCcw class="w-3 h-3" /> {{ t('settings.restoreBuiltin') }}
              </button>
              <button @click="addCommandGroup" class="text-xs text-sky-600 hover:text-sky-800 flex items-center gap-1 font-medium bg-sky-50 hover:bg-sky-100 px-3 py-1.5 rounded-lg transition-colors">
                <Plus class="w-3 h-3" /> {{ t('settings.addCommandGroup') }}
              </button>
            </div>
          </div>

          <Empty
            v-if="config.command_groups.length === 0"
            :icon="Layers"
            :description="t('settings.noCommandGroups')"
            :action-label="t('settings.addCommandGroup')"
            @action="addCommandGroup"
          />

          <div v-else class="space-y-2">
            <div v-for="(group, idx) in config.command_groups" :key="group.id"
              class="border border-slate-200 rounded-lg p-3 bg-white hover:shadow-sm transition-shadow flex items-start justify-between gap-3">
              <div class="flex items-start gap-3 flex-1 min-w-0">
                <div class="w-7 h-7 rounded-lg bg-sky-100 text-sky-600 flex items-center justify-center shrink-0">
                  <Terminal class="w-3.5 h-3.5" />
                </div>
                <div class="min-w-0 flex-1">
                  <div class="font-medium text-slate-800 text-sm flex items-center gap-1.5">
                    {{ builtinDisplayName(group) }}
                    <span v-if="group.id.startsWith('__builtin_')" class="text-[10px] text-amber-600 bg-amber-50 border border-amber-200 px-1.5 py-0.5 rounded-full font-normal leading-none">{{ t('settings.builtinBadge') }}</span>
                  </div>
                  <div class="text-xs text-slate-400">{{ group.commands.length }} {{ group.commands.length === 1 ? 'command' : 'commands' }}</div>
                  <div class="mt-1.5 flex flex-col gap-1">
                    <code v-for="(cmd, ci) in group.commands" :key="ci"
                      class="text-[10px] bg-slate-100 text-slate-500 px-1.5 py-1 rounded font-mono whitespace-pre-wrap break-all cursor-default">{{ cmd }}</code>
                  </div>
                </div>
              </div>
              <div class="flex items-center gap-1 shrink-0">
                <button @click="editCommandGroup(idx)" class="p-1.5 text-slate-500 hover:text-amber-600 hover:bg-amber-50 rounded transition-colors" :title="t('settings.edit')" :aria-label="t('settings.edit')">
                  <Edit class="w-4 h-4" />
                </button>
                <button @click="removeCommandGroup(idx)" class="p-1.5 text-slate-500 hover:text-red-600 hover:bg-red-50 rounded transition-colors" :title="t('settings.deleteTitle')" :aria-label="t('settings.deleteTitle')">
                  <Trash2 class="w-4 h-4" />
                </button>
              </div>
            </div>
          </div>
          <p class="mt-2 text-xs text-slate-400 leading-relaxed whitespace-pre-line">{{ t('settings.postCommandsHint') }}</p>
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
                    class="flex justify-between items-start text-sky-300 font-mono text-xs group/cmd">
                    <span class="flex-1 min-w-0 mr-2 whitespace-pre-wrap break-all">$ {{ cmd }}</span>
                    <div class="flex items-center gap-0.5 shrink-0">
                      <button @click="copyToClipboard(cmd)" type="button"
                        class="text-slate-600 hover:text-sky-400 p-1 opacity-0 group-hover/cmd:opacity-100 transition-opacity"
                        :title="t('settings.copy')">
                        <Copy class="w-3 h-3" />
                      </button>
                      <button @click="removeGroupCommand(i)" type="button" class="text-slate-500 hover:text-red-400 p-1 shrink-0">
                        <Trash2 class="w-3 h-3" />
                      </button>
                    </div>
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

          <div class="grid grid-cols-1 gap-4 rounded-xl border border-slate-200 bg-slate-50/70 p-4 xl:grid-cols-2">
            <fieldset>
              <legend class="text-sm font-semibold text-slate-700">{{ t('settings.manualTransferPolicy') }}</legend>
              <p id="manual-transfer-policy-hint" class="mt-1 text-xs text-slate-500">{{ t('settings.manualTransferPolicyHint') }}</p>
              <div class="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-3" aria-describedby="manual-transfer-policy-hint">
                <label
                  v-for="policy in manualTransferPolicies"
                  :key="policy"
                  class="min-h-11 cursor-pointer rounded-lg border px-3 py-2 transition-colors focus-within:ring-2 focus-within:ring-indigo-500/50"
                  :class="manualTransferPolicy === policy ? 'border-indigo-300 bg-indigo-50 text-indigo-800' : 'border-slate-200 bg-white text-slate-600 hover:bg-slate-50'"
                >
                  <input v-model="manualTransferPolicy" class="sr-only" type="radio" name="manual-transfer-policy" :value="policy" @change="invalidateManualPreflight" />
                  <span class="block text-sm font-semibold">{{ t(`settings.manualTransferPolicy_${policy}`) }}</span>
                  <span class="mt-0.5 block text-[11px] leading-4 opacity-80">{{ t(`settings.manualTransferPolicy_${policy}Desc`) }}</span>
                </label>
              </div>
            </fieldset>

            <fieldset>
              <legend class="text-sm font-semibold text-slate-700">{{ t('settings.manualExtractPolicy') }}</legend>
              <p id="manual-extract-policy-hint" class="mt-1 text-xs text-slate-500">{{ t('settings.manualExtractPolicyHint') }}</p>
              <div class="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-3" aria-describedby="manual-extract-policy-hint">
                <label
                  v-for="policy in manualExtractPolicies"
                  :key="policy"
                  class="min-h-11 cursor-pointer rounded-lg border px-3 py-2 transition-colors focus-within:ring-2 focus-within:ring-emerald-500/50"
                  :class="manualExtractPolicy === policy ? 'border-emerald-300 bg-emerald-50 text-emerald-800' : 'border-slate-200 bg-white text-slate-600 hover:bg-slate-50'"
                >
                  <input v-model="manualExtractPolicy" class="sr-only" type="radio" name="manual-extract-policy" :value="policy" @change="invalidateManualPreflight" />
                  <span class="block text-sm font-semibold">{{ t(`settings.manualExtractPolicy_${policy}`) }}</span>
                  <span class="mt-0.5 block text-[11px] leading-4 opacity-80">{{ t(`settings.manualExtractPolicy_${policy}Desc`) }}</span>
                </label>
              </div>
            </fieldset>
          </div>

          <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <label class="block text-sm font-medium text-slate-600 mb-1">
                {{ t('settings.manualLocalPath') }}
                <span v-if="manualTransferPolicy === 'remote_only'" class="font-normal text-slate-400">{{ t('settings.manualLocalOptional') }}</span>
              </label>
              <DirectoryPathInput
                v-model="manualLocalPath"
                :placeholder="t('settings.manualLocalPlaceholder')"
                :title="t('settings.selectDirectory')"
                allow-file
                :file-title="t('settings.selectFile')"
                @pick-error="handleDirectoryPickError"
                @update:model-value="invalidateManualPreflight"
              />
              <p v-if="manualTransferPolicy === 'remote_only'" class="mt-1 text-xs text-amber-700">{{ t('settings.manualRemoteOnlyLocalHint') }}</p>
            </div>
            <div>
              <label for="manual-remote-path" class="block text-sm font-medium text-slate-600 mb-1">
                {{ manualTransferPolicy === 'remote_only' ? t('settings.manualRemotePackagePath') : t('settings.remotePath') }}
              </label>
              <input id="manual-remote-path" v-model="manualRemotePath" type="text" :placeholder="manualTransferPolicy === 'remote_only' ? t('settings.manualRemotePackagePlaceholder') : t('settings.manualRemotePlaceholder')" class="w-full min-h-11 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 outline-none" @input="invalidateManualPreflight" />
              <p class="mt-1 text-xs text-slate-500">{{ manualTransferPolicy === 'remote_only' ? t('settings.manualRemotePackageHint') : t('settings.manualRemotePathHint') }}</p>
            </div>
          </div>

          <div v-if="manualExtractPolicy !== 'skip'" class="rounded-lg border border-emerald-200 bg-emerald-50/60 p-3">
            <label for="manual-extract-dir" class="block text-sm font-medium text-emerald-900">{{ t('settings.manualExtractDir') }}</label>
            <input id="manual-extract-dir" v-model="manualExtractDir" type="text" class="mt-1 min-h-11 w-full rounded-lg border border-emerald-200 bg-white px-3 py-2 font-mono text-sm text-slate-700 outline-none focus:ring-2 focus:ring-emerald-500/50" :placeholder="t('settings.manualExtractDirPlaceholder')" @input="invalidateManualPreflight" />
            <p class="mt-1 text-xs text-emerald-800">{{ t('settings.manualExtractDirHint') }}</p>
          </div>

          <!-- Server Bindings -->
          <div>
            <div class="flex items-center justify-between mb-2">
              <label class="text-sm font-medium text-slate-600">{{ t('settings.manualDeployServerBindings') }}</label>
              <div class="flex flex-wrap justify-end gap-2">
                <button @click="addManualBinding" type="button"
                  class="min-h-11 cursor-pointer text-xs text-blue-700 hover:text-blue-900 flex items-center gap-1 font-medium border border-blue-200 bg-blue-50 hover:bg-blue-100 px-3 py-2 rounded-lg transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/50">
                  <Server class="w-3.5 h-3.5" /> {{ t('settings.selectExistingServer') }}
                </button>
                <button @click="addManualServer" type="button"
                  class="min-h-11 cursor-pointer text-xs text-emerald-700 hover:text-emerald-900 flex items-center gap-1 font-medium border border-emerald-200 bg-emerald-50 hover:bg-emerald-100 px-3 py-2 rounded-lg transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500/50">
                  <Plus class="w-3.5 h-3.5" /> {{ t('settings.createAndSelectServer') }}
                </button>
              </div>
            </div>

            <div v-if="manualServerBindings.length === 0" class="text-xs text-slate-400 italic text-center py-3 bg-slate-50 rounded-lg border border-dashed border-slate-200">
              {{ t('settings.manualDeployNoBindings') }}
            </div>

            <div v-else class="space-y-2">
              <div v-for="(binding, bidx) in manualServerBindings" :key="bidx"
                class="bg-slate-50 border border-slate-200 rounded-lg p-3 space-y-2">
                <div class="flex items-center gap-2">
                  <select v-model="binding.server_id" @change="invalidateManualPreflight"
                    class="min-h-11 flex-1 p-2 border border-slate-300 rounded-lg text-sm focus:ring-2 focus:ring-blue-500 outline-none bg-white">
                    <option value="" disabled>{{ t('settings.selectServer') }}</option>
                    <option v-for="s in availableManualServers(bidx)" :key="s.id" :value="s.id">
                      {{ serverDisplayName(s) }} ({{ s.host }}:{{ s.port }}){{ s.enabled ? '' : ` - ${t('settings.disabled')}` }}
                    </option>
                  </select>
                  <button
                    v-if="binding.server_id"
                    @click="editManualBindingServer(binding, bidx)"
                    type="button"
                    class="inline-flex h-11 w-11 cursor-pointer items-center justify-center rounded-lg text-amber-600 transition-colors hover:bg-amber-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-500/50"
                    :aria-label="t('settings.editSelectedServer')"
                    :title="t('settings.editSelectedServer')"
                  >
                    <Edit class="w-4 h-4" />
                  </button>
                  <button @click="removeManualBinding(bidx)" type="button"
                    class="inline-flex h-11 w-11 cursor-pointer items-center justify-center p-1.5 text-slate-400 hover:text-red-500 hover:bg-red-50 rounded transition-colors shrink-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500/50"
                    :aria-label="t('settings.removeTargetServer')" :title="t('settings.removeTargetServer')">
                    <Trash2 class="w-3.5 h-3.5" />
                  </button>
                </div>
                <div v-if="binding.server_id" class="text-xs text-slate-500 font-mono">
                  {{ config.servers.find(server => server.id === binding.server_id)?.user }}@{{ config.servers.find(server => server.id === binding.server_id)?.host }}:{{ config.servers.find(server => server.id === binding.server_id)?.port }}
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
                  <div v-if="manualExtractPolicy !== 'skip' && binding.command_group_ids.length > 0" class="mt-3 rounded-lg border border-emerald-200 bg-emerald-50/70 p-3">
                    <label :for="`manual-extract-group-${bidx}`" class="block text-xs font-semibold text-emerald-900">{{ t('settings.manualExtractCommandGroup') }}</label>
                    <select
                      :id="`manual-extract-group-${bidx}`"
                      v-model="binding.extract_command_group_id"
                      class="mt-1 min-h-11 w-full rounded-lg border border-emerald-200 bg-white px-3 py-2 text-sm text-slate-700 outline-none focus:ring-2 focus:ring-emerald-500/50"
                      @change="invalidateManualPreflight"
                    >
                      <option :value="null" disabled>{{ t('settings.manualSelectExtractCommandGroup') }}</option>
                      <option v-for="groupId in binding.command_group_ids" :key="groupId" :value="groupId">{{ commandGroupName(groupId) }}</option>
                    </select>
                    <p class="mt-1 text-[11px] leading-4 text-emerald-800">{{ t('settings.manualExtractCommandGroupHint') }}</p>
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
            <p v-if="hasDuplicateManualServers" role="alert" class="mt-2 rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-sm text-rose-700">
              {{ t('settings.duplicateManualServer') }}
            </p>
            <p v-if="hasUnavailableManualServer" role="alert" class="mt-2 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800">
              {{ t('settings.unavailableManualServer') }}
            </p>
          </div>

          <div class="rounded-lg border border-indigo-200 bg-indigo-50/60 px-4 py-3" aria-live="polite">
            <div class="flex items-start gap-3">
              <ShieldCheck class="mt-0.5 h-4 w-4 shrink-0 text-indigo-600" />
              <div>
                <div class="text-sm font-semibold text-indigo-900">{{ t('settings.manualExecutionPlan') }}</div>
                <p class="mt-0.5 text-xs leading-5 text-indigo-800">
                  {{ t(`settings.manualTransferPolicy_${manualTransferPolicy}`) }} ·
                  {{ t(`settings.manualExtractPolicy_${manualExtractPolicy}`) }} ·
                  {{ t('settings.manualExecutionPlanServers', { count: manualServerBindings.length }) }}
                </p>
              </div>
            </div>
          </div>

          <div v-if="manualPreflightResults.length > 0" class="space-y-2" aria-live="polite">
            <div class="text-sm font-semibold text-slate-700">{{ t('settings.manualPreflightResults') }}</div>
            <div
              v-for="result in manualPreflightResults"
              :key="result.server_id"
              class="grid grid-cols-1 gap-2 rounded-lg border border-slate-200 bg-white px-4 py-3 text-sm md:grid-cols-[minmax(160px,0.8fr)_minmax(0,1.7fr)_auto] md:items-center"
            >
              <div class="font-semibold text-slate-700">{{ result.server_name }}</div>
              <div class="min-w-0">
                <div class="truncate font-mono text-xs text-slate-600" :title="result.remote_package_path">{{ result.remote_package_path }}</div>
                <div class="mt-1 text-xs text-slate-500">{{ t('settings.manualExtractTarget') }}: {{ result.extract_dir }}</div>
              </div>
              <div class="flex flex-wrap gap-1.5 md:justify-end">
                <span class="rounded-full border border-sky-200 bg-sky-50 px-2 py-1 text-xs font-medium text-sky-700">{{ t(`settings.manualTransferAction_${result.transfer_action}`) }}</span>
                <span class="rounded-full border border-emerald-200 bg-emerald-50 px-2 py-1 text-xs font-medium text-emerald-700">{{ t(`settings.manualExtractAction_${result.extract_action}`) }}</span>
              </div>
            </div>
          </div>

          <div class="flex flex-wrap items-center gap-3">
            <button
              type="button"
              class="min-h-11 cursor-pointer rounded-lg border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-500/40"
              :disabled="!manualDeployInputValid || appStore.isManualDeploying || isLatestManualDeployRunning || isManualPreflighting"
              @click="handleManualPreflight"
            >
              <span class="inline-flex items-center gap-2">
                <Search class="h-4 w-4" />
                {{ isManualPreflighting ? t('settings.manualPreflighting') : t('settings.manualPreflight') }}
              </span>
            </button>
            <button @click="handleManualDeploy"
              class="min-h-11 cursor-pointer bg-indigo-600 text-white px-4 py-2 rounded-lg hover:bg-indigo-700 transition-colors flex items-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2"
              :disabled="!canManualDeploy">
              <UploadCloud class="w-4 h-4" />
              {{ appStore.isManualDeploying ? t('settings.deploying') : t('settings.deployNow') }}
            </button>
            <button
              v-if="taskStateStore.latestManualDeploy"
              type="button"
              class="min-h-11 cursor-pointer inline-flex items-center gap-2 rounded-lg border border-indigo-200 bg-indigo-50 px-4 py-2 text-sm font-medium text-indigo-700 transition-colors hover:bg-indigo-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50"
              @click="manualDeployDialogOpen = true"
            >
              <FileText class="h-4 w-4" />
              {{ isLatestManualDeployRunning ? t('settings.viewDeployLog') : t('settings.viewLastDeployLog') }}
            </button>
            <span v-if="appStore.manualDeployMsg" :class="manualDeployMsgType === 'error' ? 'text-red-500' : 'text-sky-600'" class="text-sm font-medium">
              {{ appStore.manualDeployMsg }}
            </span>
          </div>
        </div>
      </div>
    </div>

    <!-- Local Post-Copy Scripts (independent of remote deploy) -->
    <div v-if="shows('delivery')" class="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
      <div class="px-6 py-4 bg-slate-50 border-b border-slate-200 flex items-center justify-between gap-3">
        <div class="flex items-center gap-3">
          <div class="w-8 h-8 rounded-lg bg-teal-100 text-teal-600 flex items-center justify-center shrink-0">
            <Terminal class="w-4 h-4" />
          </div>
          <div>
            <h3 class="text-base font-semibold text-slate-700">{{ t('settings.localPostCopyScripts') }}</h3>
            <p class="text-xs text-slate-400 mt-0.5">{{ t('settings.localPostCopyScriptsDesc') }}</p>
          </div>
        </div>
      </div>
      <div class="p-6 space-y-4">
        <!-- Script group list -->
        <div class="flex justify-between items-center mb-2">
          <div class="text-sm font-medium text-slate-700">{{ t('settings.localScriptGroups') }}</div>
          <button @click="addLocalGroup" class="text-xs text-teal-600 hover:text-teal-800 flex items-center gap-1 font-medium bg-teal-50 hover:bg-teal-100 px-3 py-1.5 rounded-lg transition-colors">
            <Plus class="w-3 h-3" /> {{ t('settings.addLocalScriptGroup') }}
          </button>
        </div>

        <Empty
          v-if="config.local_command_groups.length === 0"
          :icon="Terminal"
          :description="t('settings.noLocalScriptGroups')"
          :action-label="t('settings.addLocalScriptGroup')"
          @action="addLocalGroup"
        />

        <div v-else class="space-y-2">
          <div v-for="(group, gi) in config.local_command_groups" :key="group.id"
            class="border border-slate-200 rounded-lg p-3 bg-white hover:shadow-sm transition-shadow flex items-start justify-between gap-3">
            <div class="flex items-start gap-3 flex-1 min-w-0">
              <div class="w-7 h-7 rounded-lg bg-teal-100 text-teal-600 flex items-center justify-center shrink-0">
                <Terminal class="w-3.5 h-3.5" />
              </div>
              <div class="min-w-0 flex-1">
                <div class="font-medium text-slate-800 text-sm flex items-center gap-2">
                  {{ group.name }}
                  <span class="text-xs px-2 py-0.5 rounded-full"
                        :class="group.on_failure === 'abort' ? 'bg-red-100 text-red-600' : 'bg-slate-100 text-slate-600'">
                    {{ group.on_failure === 'abort' ? t('settings.onFailureAbort') : t('settings.onFailureContinue') }}
                  </span>
                </div>
                <div class="text-xs text-slate-400">{{ group.commands.length }} {{ group.commands.length === 1 ? 'command' : 'commands' }}</div>
                <div class="mt-1.5 flex flex-col gap-1">
                  <code v-for="(cmd, ci) in group.commands" :key="ci"
                    class="text-[10px] bg-slate-100 text-slate-500 px-1.5 py-1 rounded font-mono whitespace-pre-wrap break-all">{{ cmd }}</code>
                </div>
              </div>
            </div>
            <div class="flex items-center gap-1 shrink-0">
              <button @click="editLocalGroup(gi)" class="p-1.5 text-slate-500 hover:text-amber-600 hover:bg-amber-50 rounded transition-colors" :title="t('settings.edit')" :aria-label="t('settings.edit')">
                <Edit class="w-4 h-4" />
              </button>
              <button @click="removeLocalGroup(gi)" class="p-1.5 text-slate-500 hover:text-red-600 hover:bg-red-50 rounded transition-colors" :title="t('settings.deleteTitle')" :aria-label="t('settings.deleteTitle')">
                <Trash2 class="w-4 h-4" />
              </button>
            </div>
          </div>
        </div>

        <p class="text-xs text-slate-400 leading-relaxed whitespace-pre-line">{{ t('settings.localScriptVariableHint') }}</p>
      </div>
    </div>

    <!-- Local Script Group Edit Modal -->
    <div v-if="shows('delivery') && isEditingLocalGroup" class="fixed inset-0 bg-black/50 flex items-center justify-center z-[65] p-4">
      <div class="bg-white rounded-xl p-6 w-full max-w-lg shadow-2xl transform transition-all max-h-[80vh] flex flex-col">
        <h3 class="text-lg font-bold mb-4 text-slate-800 shrink-0">{{ editingLocalGroupIndex >= 0 ? t('settings.editLocalScriptGroup') : t('settings.addLocalScriptGroup') }}</h3>
        <div class="space-y-4 flex-1 overflow-y-auto pr-1">
          <div>
            <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.nameAlias') }}</label>
            <input v-model="localGroupForm.name" class="w-full p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-teal-500 outline-none" />
          </div>
          <div>
            <label class="block text-sm font-medium mb-1 text-slate-700">{{ t('settings.onFailure') }}</label>
            <select v-model="localGroupForm.on_failure" class="w-full p-2.5 border border-slate-300 rounded-lg text-sm bg-white focus:ring-2 focus:ring-teal-500 outline-none">
              <option value="continue">{{ t('settings.onFailureContinue') }}</option>
              <option value="abort">{{ t('settings.onFailureAbort') }}</option>
            </select>
          </div>
          <div>
            <label class="block text-sm font-medium mb-2 text-slate-700">{{ t('settings.postCommands') }}</label>
            <div class="flex gap-2 mb-2">
              <input v-model="newLocalGroupCommand" @keyup.enter="addLocalGroupCommand"
                :placeholder="t('settings.commandPlaceholder')"
                class="flex-1 p-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-teal-500 outline-none font-mono text-sm" />
              <button @click="addLocalGroupCommand" type="button" class="bg-slate-100 hover:bg-slate-200 p-2 rounded-lg text-slate-600">
                <Plus class="w-5 h-5" />
              </button>
            </div>
            <ul class="space-y-1.5 bg-slate-900 rounded-lg p-3 max-h-48 overflow-y-auto">
              <li v-for="(cmd, ci) in localGroupForm.commands" :key="ci"
                class="flex justify-between items-start text-green-400 font-mono text-xs">
                <span class="flex-1 min-w-0 mr-2 whitespace-pre-wrap break-all">$ {{ cmd }}</span>
                <button @click="removeLocalGroupCommand(ci)" type="button" class="text-slate-500 hover:text-red-400 p-1 shrink-0">
                  <Trash2 class="w-3 h-3" />
                </button>
              </li>
              <li v-if="!localGroupForm.commands.length" class="text-slate-600 text-xs italic text-center">{{ t('settings.commandGroupNoCommands') }}</li>
            </ul>
          </div>
        </div>
        <div class="flex justify-end gap-3 mt-6 pt-4 border-t border-slate-100 shrink-0">
          <button @click="isEditingLocalGroup = false" class="px-4 py-2 text-slate-600 hover:bg-slate-100 rounded-lg font-medium transition-colors">{{ t('console.cancel') }}</button>
          <button @click="saveLocalGroup" :disabled="!localGroupForm.name.trim()"
            class="px-6 py-2 bg-teal-600 text-white rounded-lg hover:bg-teal-700 font-medium transition-colors shadow-sm disabled:opacity-50 disabled:cursor-not-allowed">{{ t('settings.save') }}</button>
        </div>
      </div>
    </div>

    <button
      @click="save"
      :disabled="hasConfigErrors || isSaving"
      :aria-busy="isSaving ? 'true' : 'false'"
      class="fixed right-6 bottom-6 z-40 bg-blue-600 hover:bg-blue-700 text-white px-5 py-3 rounded-full font-medium flex items-center gap-2 transition-all shadow-lg shadow-blue-200/70 motion-reduce:transition-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-slate-50"
      :class="hasConfigErrors ? 'opacity-50 cursor-not-allowed hover:bg-blue-600 shadow-none' : 'hover:-translate-y-0.5 motion-reduce:hover:translate-y-0'"
    >
      <svg
        v-if="isSaving"
        class="w-4 h-4 animate-spin motion-reduce:animate-none"
        fill="none"
        viewBox="0 0 24 24"
        aria-hidden="true"
      >
        <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="3" />
        <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z" />
      </svg>
      <Save v-else class="w-4 h-4" aria-hidden="true" />
      {{ isSaving ? t('settings.saving') : t('settings.save') }}
    </button>

    <ManualDeployLogDialog
      :open="manualDeployDialogOpen"
      :session="taskStateStore.latestManualDeploy"
      :group="latestManualDeployGroup"
      :logs="taskStateStore.taskLogs"
      :servers="config.servers"
      @close="manualDeployDialogOpen = false"
      @edit-server="editManualDeployServer"
    />

    <AppConfirmDialog
      :open="Boolean(pendingConfirmation)"
      :title="pendingConfirmation?.title ?? ''"
      :description="pendingConfirmation?.description ?? ''"
      :confirm-label="pendingConfirmation?.confirmLabel ?? t('settings.confirm')"
      :cancel-label="t('console.cancel')"
      :tone="pendingConfirmation?.tone ?? 'danger'"
      :busy="confirmationBusy"
      @confirm="confirmPendingAction"
      @cancel="closeConfirmation"
    />
  </div>
  </div>
</template>
