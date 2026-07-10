# Remote Package Patch Backend Contracts

## Commands

- `remote_package_test_connection(config)` validates TCP reachability, SSH handshake, and authentication only. It must not execute a remote command, because hardened appliances can allow SSH login while rejecting exec channels with `Remote command execution is not allowed`.
- `remote_package_list_dir(config, path)` lists one remote directory through SFTP. Entries are sorted with directories first.
- `remote_package_pick_local_file(kind)` opens a native local file picker for replacement files or private keys through the Tauri main thread.
- `remote_package_scan_package(config, packagePath)` runs a remote scan script and returns package inventory for middle tar and nested tar.zst layers.
- `remote_package_start_patch(request)` uploads one replacement file, uploads a generated patch script, executes it remotely, and returns output path, backup path, replacement md5, workdir, and updated manifest paths.

## Safety Rules

- Default output mode never overwrites the source package.
- Overwrite mode backs up the source package first and replaces it only after final package verification succeeds.
- Scan and patch workdirs are created beside the selected remote package and are emitted in events/logs.
- Credentials are session-only and must not be written to config or logs.
- The remote package page defaults SSH port input to `23333`. If a connection probe fails at TCP connect or SSH handshake for an IPv4 host, the frontend may call the existing `enable_appliance_ssh` access-control command with a single direct target, then retry the SSH probe. Authentication failures must not auto-enable SSH. If the access-control result reports a port, retry with that port.
- md5 updates are exact-path only; same-name files are not batch-updated.
- Scan and patch share one `PATCH_BUSY` guard. Connection test and directory listing do not use that guard.
- Remote archive mutation assumes Linux has `bash`, `tar`, `gzip`, `zstd`, `md5sum`, `df`, `awk`, `cp`, `mv`, and `du`.

## Scenario: SSH Probe, Default Port, And Access-Control Retry

### 1. Scope / Trigger

- Trigger: changes to the remote package page connection panel, `remote_package_test_connection`, default SSH port selection, or the automatic retry through appliance access control.

### 2. Signatures

```typescript
// src/lib/remotePackagePatch.ts
export const REMOTE_PACKAGE_PATCH_DEFAULT_SSH_PORT = 23333;
export function resolveRemotePackagePatchSshPort(value: unknown): number;
export function shouldAttemptRemotePackagePatchAutoEnable(error: string): boolean;
export function buildRemotePackagePatchEnableSshRequest(config: RemoteSshConfig): EnableApplianceSshRequest | null;
```

```rust
// src-tauri/src/remote_package_patch/mod.rs
#[tauri::command]
pub async fn remote_package_test_connection(config: RemoteSshConfig) -> Result<String, String>;
```

### 3. Contracts

- New remote package connection forms default to SSH port `23333`.
- `remote_package_test_connection` validates config, TCP reachability, SSH handshake, and authentication only. It must not run `uname`, `true`, SFTP, or any other remote command.
- The success message is informational; the frontend treats any `Ok(_)` as connected.
- Automatic SSH enable is frontend-orchestrated by calling the existing `enableApplianceSsh()` wrapper with one direct IPv4 target and `addWhitelistRule: false`.
- Automatic enable is attempted only for transport-shaped failures: `TCP connect failed` or `SSH handshake failed`.
- Authentication failures, validation failures, hostnames, and non-IPv4 hosts must not call access control.
- If access control returns a non-zero SSH `port`, the page updates the port and retries the SSH probe once.

### 4. Validation & Error Matrix

| Condition | Behavior |
|---|---|
| Port is blank, zero, non-numeric, or outside `1..65535` | Frontend resolves it to `23333` before sending `RemoteSshConfig` |
| SSH login succeeds but remote exec is forbidden | Connection test succeeds; scan/patch may still fail later if remote scripts cannot execute |
| First probe fails with TCP connect or SSH handshake error and host is IPv4 | Call access control, then retry once |
| First probe fails with SSH authentication error | Do not call access control; show the original error |
| Access control succeeds and returns `port` | Retry with that returned port |
| Access control fails or returns no successful result | Show the original SSH probe error and log the access-control failure |

### 5. Good/Base/Bad Cases

- Good: `192.168.1.15:23333` refuses TCP, access control enables SSH and reports port `23333`, retry succeeds.
- Good: appliance allows SSH login but rejects exec with `Remote command execution is not allowed`; test connection still succeeds because it only authenticates.
- Base: saved deploy server has an explicit port; the page uses that port instead of the default.
- Bad: wrong SSH password triggers access control and changes appliance state even though the service is already reachable.
- Bad: test connection runs `uname -sr`; hardened appliances report failure even though SSH login works.

### 6. Tests Required

- Rust unit: `run_connection_test` succeeds when the injected connect/auth function succeeds and does not require command output.
- Rust unit: connect errors are surfaced unchanged.
- Node unit: default port resolves to `23333`.
- Node unit: access-control request builder returns a single direct target for IPv4 and `null` for hostnames.
- Node unit: auto-enable trigger returns true for TCP/handshake errors and false for authentication errors.
- Node unit: `RemotePackagePatchPage` is present in the app keep-alive include list.
- Type/build: `pnpm check` after changing the page or helper contracts.

### 7. Wrong vs Correct

#### Wrong

```rust
let session = ssh::connect(&config)?;
let output = ssh::exec_capture(&session, "uname -sr")?;
Ok(output.trim().to_string())
```

This treats restricted remote command execution as a failed SSH connection.

#### Correct

```rust
run_connection_test(config, |config| ssh::connect(config).map(|_| ()))
```

The test command proves login/authentication only; script execution is checked by scan/patch.

#### Wrong

```typescript
if (firstError) await enableApplianceSsh(request);
```

This calls access control for wrong-password failures.

#### Correct

```typescript
if (shouldAttemptRemotePackagePatchAutoEnable(firstError)) {
  await enableApplianceSsh(request);
}
```

Only transport-shaped failures trigger appliance state changes.
