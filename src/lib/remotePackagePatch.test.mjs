import assert from 'node:assert/strict';

import {
  REMOTE_PACKAGE_PATCH_DEFAULT_SSH_PORT,
  REMOTE_PACKAGE_PATCH_DEFAULT_PASSWORD,
  buildRemotePackagePatchEnableSshRequest,
  composeInternalTargetPath,
  defaultPatchedPath,
  formatBytes,
  layerKey,
  replacementName,
  resolveRemotePackagePatchSshPort,
  shouldAttemptRemotePackagePatchAutoEnable,
  targetCandidates,
  updateRemotePackagePatchHostHistory,
  validateInternalTargetPath,
  visibleStages,
} from './remotePackagePatch.ts';

const zstLayer = { kind: 'zst', zstPath: 'pkg/app/demo.tar.zst' };
const inventory = {
  packagePath: '/tmp/VMS.tar.gz',
  middleTarPath: 'VMS/VMS.tar',
  entries: [
    {
      layer: zstLayer,
      path: './app/demo/bin/libdemo.so',
      kind: 'file',
      size: 7,
      permsText: '-rw-r--r--',
      ownerText: 'root/root',
      mtimeText: '2026-01-02 03:04',
    },
    {
      layer: { kind: 'middle' },
      path: 'pkg/app/demo.tar.zst',
      kind: 'file',
      size: 123,
      permsText: '-rw-r--r--',
      ownerText: 'root/root',
      mtimeText: '2026-01-02 03:04',
    },
    {
      layer: zstLayer,
      path: 'app/demo/conf/app.yaml',
      kind: 'file',
      size: 11,
      permsText: '-rw-r--r--',
      ownerText: 'root/root',
      mtimeText: '2026-01-02 03:04',
    },
  ],
};

assert.equal(replacementName(String.raw`C:\libs\libdemo.so`), 'libdemo.so');
assert.equal(REMOTE_PACKAGE_PATCH_DEFAULT_SSH_PORT, 23333);
assert.equal(REMOTE_PACKAGE_PATCH_DEFAULT_PASSWORD, 'admin_123');
assert.deepEqual(
  updateRemotePackagePatchHostHistory(['10.0.0.2', ' 10.0.0.1 ', '10.0.0.2'], '10.0.0.1'),
  ['10.0.0.1', '10.0.0.2'],
);
assert.deepEqual(updateRemotePackagePatchHostHistory('invalid', 'server.local'), ['server.local']);
assert.equal(resolveRemotePackagePatchSshPort(undefined), 23333);
assert.equal(resolveRemotePackagePatchSshPort(0), 23333);
assert.equal(resolveRemotePackagePatchSshPort('abc'), 23333);
assert.equal(resolveRemotePackagePatchSshPort(2222), 2222);
assert.equal(shouldAttemptRemotePackagePatchAutoEnable('TCP connect failed: connection refused'), true);
assert.equal(shouldAttemptRemotePackagePatchAutoEnable('SSH handshake failed: banner timeout'), true);
assert.equal(shouldAttemptRemotePackagePatchAutoEnable('SSH password authentication failed: denied'), false);
assert.equal(defaultPatchedPath('/tmp/VMS.tar.gz'), '/tmp/VMS.patched.tar.gz');
assert.equal(defaultPatchedPath('/tmp/VMS.bin'), '/tmp/VMS.bin.patched.tar.gz');

assert.deepEqual(
  buildRemotePackagePatchEnableSshRequest({
    host: '192.168.1.15',
    port: 23333,
    username: 'root',
    auth: { kind: 'password', password: 'secret' },
  }),
  {
    targets: [{ ip: '192.168.1.15' }],
    applianceVersion: 'componentized',
    whitelistScope: 'allTcp',
    sshUsername: 'root',
    sshPassword: 'secret',
    addWhitelistRule: false,
  },
);
assert.equal(
  buildRemotePackagePatchEnableSshRequest({
    host: 'example.local',
    port: 23333,
    username: 'root',
    auth: { kind: 'password', password: 'secret' },
  }),
  null,
);

const candidates = targetCandidates(inventory, 'libdemo.so');
assert.equal(candidates.length, 1);
assert.equal(candidates[0].path, './app/demo/bin/libdemo.so');

assert.equal(layerKey(null), 'auto');
assert.equal(layerKey({ kind: 'middle' }), 'middle');
assert.equal(layerKey(zstLayer), 'zst:pkg/app/demo.tar.zst');

assert.equal(
  composeInternalTargetPath('app/demo/bin/', 'libdemo.so'),
  'app/demo/bin/libdemo.so',
);
assert.equal(validateInternalTargetPath('app/demo/bin/libdemo.so'), null);
assert.equal(validateInternalTargetPath(''), 'required');
assert.equal(validateInternalTargetPath('/app/libdemo.so'), 'absolute');
assert.equal(validateInternalTargetPath('app/demo/'), 'trailingSlash');
assert.equal(validateInternalTargetPath('../libdemo.so'), 'parentSegment');

const newFileZstStages = visibleStages({ overwrite: false, layer: zstLayer });
assert.ok(newFileZstStages.includes('finalize'));
assert.ok(!newFileZstStages.includes('backup_overwrite'));
assert.ok(newFileZstStages.includes('extract_inner'));

const overwriteMiddleStages = visibleStages({ overwrite: true, layer: { kind: 'middle' } });
assert.ok(overwriteMiddleStages.includes('backup_overwrite'));
assert.ok(!overwriteMiddleStages.includes('finalize'));
assert.ok(!overwriteMiddleStages.includes('extract_inner'));
assert.ok(!overwriteMiddleStages.includes('repack_inner'));

const autoStages = visibleStages({ overwrite: false, layer: null });
assert.ok(autoStages.includes('extract_inner'));

assert.equal(formatBytes(1536), '1.5 KB');

console.log('remotePackagePatch tests PASSED');
