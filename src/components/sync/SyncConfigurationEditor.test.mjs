import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const editorSource = readFileSync(new URL('./SyncConfigurationEditor.vue', import.meta.url), 'utf8');

test('sync sections share one editor backed by the sync-domain store action', () => {
  assert.match(editorSource, /configStore\.saveSync\(\)/);
  assert.match(editorSource, /section\?: SyncConfigurationSection/);
  assert.match(editorSource, /shows\('tasks'\)/);
  assert.match(editorSource, /shows\('strategy'\)/);
  assert.match(editorSource, /shows\('delivery'\)/);
  assert.doesNotMatch(editorSource, /saveConfig\(/);
  assert.doesNotMatch(editorSource, /getConfig\(/);
});

test('combined tasks and strategy section preserves every production configuration area', () => {
  assert.match(editorSource, /'tasks-strategy'/);
  assert.match(editorSource, /props\.section === 'tasks-strategy'/);
  assert.match(editorSource, /sync-tasks-strategy-stack/);
  assert.doesNotMatch(editorSource, /sync-tasks-strategy-grid/);
  assert.doesNotMatch(editorSource, /xl:grid-cols-\[minmax\(0,1\.15fr\)_minmax\(360px,1fr\)\]/);

  for (const feature of [
    'taskLocalPathInput',
    'server_bindings',
    'local_script_binding',
    'post_copy_execution_order',
    'config.time_ranges',
    'config.file_extensions',
    'config.filename_includes',
  ]) {
    assert.match(editorSource, new RegExp(feature.replaceAll('.', '\\.')));
  }
});

test('delivery workspace retains advanced server, manual deployment, command and local script controls', () => {
  assert.match(editorSource, /sync-delivery-stack/);
  assert.doesNotMatch(editorSource, /sync-delivery-grid/);
  assert.match(editorSource, /serverForm\.ssh_timeout_secs/);
  assert.match(editorSource, /testAllServers/);
  assert.match(editorSource, /manualServerBindings/);
  assert.match(editorSource, /commandGroupForm/);
  assert.match(editorSource, /localGroupForm/);
});

test('manual deployment exposes safe transfer, extraction and server preflight controls', () => {
  for (const feature of [
    'manualTransferPolicy',
    'manualExtractPolicy',
    'manualExtractDir',
    'extract_command_group_id',
    'preflightManualDeploy',
    'manualPreflightResults',
  ]) {
    assert.match(editorSource, new RegExp(feature));
  }
  assert.match(editorSource, /type="radio" name="manual-transfer-policy"/);
  assert.match(editorSource, /type="radio" name="manual-extract-policy"/);
  assert.match(editorSource, /manualTransferPolicy\.value === 'remote_only'/);
  assert.match(editorSource, /manualExtractPolicy\.value === 'skip'/);
});

test('manual deployment keeps a recoverable run-scoped log session', () => {
  assert.match(editorSource, /ManualDeployLogDialog/);
  assert.match(editorSource, /taskStateStore\.latestManualDeploy/);
  assert.match(editorSource, /manualDeployDialogOpen\.value = true/);
  assert.match(editorSource, /availableManualServers\(bidx\)/);
  assert.match(editorSource, /createAndSelectServer/);
  assert.match(editorSource, /editManualBindingServer/);
  assert.match(editorSource, /hasDuplicateManualServers/);
});

test('server changes use application UI instead of native browser dialogs', () => {
  assert.match(editorSource, /AppConfirmDialog/);
  assert.match(editorSource, /testServerFormConnection/);
  assert.match(editorSource, /serverGlobalEditNotice/);
  assert.doesNotMatch(editorSource, /\b(?:window\.)?alert\s*\(/);
  assert.doesNotMatch(editorSource, /\b(?:window\.)?confirm\s*\(/);
});

test('server editor only dismisses when pointer interaction starts on the backdrop', () => {
  assert.match(editorSource, /@pointerdown\.self="closeServerEditor"/);
  assert.doesNotMatch(editorSource, /@click\.self="closeServerEditor"/);
});

test('built-in deployment commands are two fixed workflows with migration support', () => {
  assert.match(editorSource, /BUILTIN_NORMAL_WORKFLOW_ID = '__builtin_normal_workflow__'/);
  assert.match(editorSource, /BUILTIN_FORCE_WORKFLOW_ID = '__builtin_force_workflow__'/);
  assert.match(editorSource, /commands: \[builtinExtractCommand, builtinUninstallCommand, builtinCleanupCommand, builtinInstallGuardCommand, builtinInstallCommand\]/);
  assert.match(editorSource, /commands: \[builtinExtractCommand, builtinCleanupResidualsCommandV2, builtinInstallGuardCommand, builtinInstallCommand\]/);
  assert.match(editorSource, /builtinInstallGuardCommand/);
  assert.match(editorSource, /builtinCleanupResidualsCommand/);
  assert.match(editorSource, /migrateBuiltinWorkflowBindings/);
  assert.match(editorSource, /function migrateBuiltinWorkflowBindings\(\): boolean/);
  assert.match(editorSource, /if \(migrateBuiltinWorkflowBindings\(\)\) \{\s*shouldSaveBuiltinMigration = true;/);
  assert.match(editorSource, /LEGACY_BUILTIN_COMMAND_IDS/);
  assert.match(editorSource, /trying integrated_uninstall\.sh first/);
  assert.match(editorSource, /integrated_uninstall\.sh failed; continuing with forced cleanup/);
  assert.match(editorSource, /printf "y\\\\n" \| sh \.\/integrated_uninstall\.sh/);
  assert.match(editorSource, /rm -f \/usr\/local\/bin\/integrated_uninstall\.sh/);
  assert.match(editorSource, /command -v omc_uninstall\.sh/);
  assert.match(editorSource, /command -v hauninstall\.sh/);
  assert.match(editorSource, /rm -rf \/opt\/common-database0\/data\/pgdata \/program\/omc\/ \/var\/log\/func-\* \/var\/log\/common-\* \/data \/var\/runtime\/cfg\/ha_maintenance_mode \/opt/);
  assert.match(editorSource, /rm -rf \/mnt\/BK \|\| result=1/);
  assert.match(editorSource, /\[ -e \/mnt\/BK \]/);
  assert.match(editorSource, /rm -f \/etc\/systemd\/system\/\{cfc,cfs,deployOps,openresty\}\.service \/etc\/systemd\/system\/func-\* \/etc\/systemd\/system\/common-\*/);
  assert.match(editorSource, /systemctl stop cfc\.service cfs\.service deployOps\.service openresty\.service/);
  assert.match(editorSource, /vendor OMC uninstaller failed; continuing with forced cleanup/);
  assert.match(editorSource, /HA uninstaller failed; continuing with forced cleanup/);
  assert.match(editorSource, /group\.commands\.includes\(builtinCleanupResidualsCommand\)/);
  assert.match(editorSource, /cleanup verification failed/);
  assert.match(editorSource, /\[PRECHECK\] framework ports: none/);
  assert.match(editorSource, /\[PRECHECK\] \/program\/omc: absent/);
  assert.match(editorSource, /\[PRECHECK\] \/opt\/package: absent/);
  assert.match(editorSource, /LC_ALL=C sort \| cksum/);
  assert.match(editorSource, /exit 42/);
});

test('force cleanup runs uninstallers before service and rm fallbacks', () => {
  const commandStart = editorSource.indexOf('const builtinCleanupResidualsCommandV2 = [');
  const commandEnd = editorSource.indexOf("].join(' ');", commandStart);
  assert.notEqual(commandStart, -1);
  assert.notEqual(commandEnd, -1);

  const commandSource = editorSource.slice(commandStart, commandEnd);
  const orderedMarkers = [
    'sh ./integrated_uninstall.sh',
    'rm -f /usr/local/bin/integrated_uninstall.sh',
    'command -v omc_uninstall.sh',
    'command -v hauninstall.sh',
    'systemctl stop cfc.service cfs.service deployOps.service openresty.service',
    'rm -f /etc/systemd/system/{cfc,cfs,deployOps,openresty}.service',
    'rm -rf /opt/common-database0/data/pgdata',
    'rm -rf /mnt/BK',
  ];
  let previousIndex = -1;
  for (const marker of orderedMarkers) {
    const markerIndex = commandSource.indexOf(marker);
    assert.ok(markerIndex > previousIndex, `${marker} must follow the previous cleanup stage`);
    previousIndex = markerIndex;
  }
});

test('command group command details are collapsed by default and can be expanded accessibly', () => {
  assert.match(editorSource, /expandedCommandGroupIds = ref<Set<string>>\(new Set\(\)\)/);
  assert.match(editorSource, /:aria-expanded="isCommandGroupExpanded\(group\.id\)"/);
  assert.match(editorSource, /:aria-controls="`command-group-commands-\$\{idx\}`"/);
  assert.match(editorSource, /v-if="isCommandGroupExpanded\(group\.id\)"/);
  assert.match(editorSource, /@click="toggleCommandGroup\(group\.id\)"/);
});
