# Remote Package Patch Design

## Recommended Approach

Use the app as a Windows-side control panel and run package mutation on the target Linux server through SSH/SFTP. The target environment is assumed to have `zstd`, so the MVP can use remote system commands instead of bundling a Linux helper binary.

This avoids transferring the full installation package across the network. The only network upload is the selected replacement file plus a small generated remote shell script.

## UI Shape

The tool page should be a workbench, not a landing page.

1. Connection panel
   - Server IP/host, port, username.
   - Authentication mode: password or private key.
   - Test/connect action.
   - Option to load from existing saved deploy servers where possible.

2. Remote package browser
   - XFTP-like file list after a successful connection.
   - Breadcrumb/current path bar with refresh and parent directory controls.
   - Directory rows navigable by double click, Enter, and explicit open button.
   - File rows selectable; `*.tar.gz` rows are emphasized as valid package candidates.

3. Replacement setup
   - Local file picker for the replacement library/config.
   - Internal target selector with three levels:
     1. Auto-scan matching file-name candidates inside the package and let the user choose one.
     2. If no candidate is suitable, open a package-internal directory browser so the user can choose the full internal directory; combine it with the local replacement file name and allow a final filename edit.
     3. If directory browsing is insufficient, allow direct full internal target path input.
   - Output naming: default `original-name.patched.tar.gz`.
   - Optional overwrite mode: disabled by default; when enabled, show a required confirmation summary and backup path.
   - Safety summary before execution.

4. Execution panel
   - Stage checklist and streaming logs.
   - Remote temp directory and output package path visible after start.
   - Final result with copyable output path.

## Backend Boundaries

Add a dedicated Rust module, tentatively `remote_package_patch`, instead of growing `main.rs`.

Tauri commands:

- `remote_package_connect_test(request)`: validates SSH credentials.
- `remote_package_list_dir(request)`: returns remote directory entries through SFTP.
- `remote_package_pick_local_file(...)` or a general local file picker command if needed.
- `remote_package_start_patch(request)`: uploads replacement file, writes remote script, executes remote patch, streams logs/events.

Core structs:

- `RemoteSshConfig`: host, port, username, auth mode, password/key.
- `RemoteDirEntry`: name, path, kind, size, modified time, permissions.
- `PackageInternalEntry`: internal path, kind, size, source archive layer, optional modified time.
- `PackageTargetCandidate`: internal file path, matched file name, source archive layer, md5 manifest path if known.
- `PackagePatchRequest`: connection, package path, replacement local path, internal target path or candidate selection, output policy.
- `PackagePatchEvent`: stage, level, message, optional progress.

## Remote Execution Strategy

The Rust backend should create a unique remote temp directory beside the selected package, for example `<package-dir>/.file-sync-tool-patch-<timestamp>/`. This keeps the heavy rewrite work on the same likely-large filesystem as the source package. Custom temp roots can be added later as an advanced option if real usage needs it. The backend uploads:

- replacement file
- generated patch script

The script performs the archive rewrite on the server. It must avoid extracting the full package into a large directory. It can use streaming and temporary archive files for each affected layer. Because tar entries include size/checksum and compressed streams cannot be edited in place, the script writes a new output package.

Safety defaults:

- Never overwrite the source package by default.
- If overwrite mode is enabled, move/copy the original package to a timestamped backup path before replacing it.
- Require explicit UI confirmation for overwrite mode, including source package path, generated output path, and backup path.
- Emit the remote temp directory path in logs so failed cleanup can be handled manually.
- Use `set -euo pipefail`.
- Quote paths safely.
- Verify expected output exists and is non-empty.
- Verify replacement file MD5 appears in the final package md5 list for the selected internal target path.
- Do not update same-name files or unrelated md5 rows that the user did not select.
- When a rewritten lower-level `md5` file is itself listed by a parent `md5`, update only that parent row.
- Leave source package untouched on failure.

## Key Trade-Offs

- Remote shell commands are faster to ship and fit the known Linux environment, but require careful quoting and tests around generated scripts.
- A future bundled Linux helper would be more portable and easier to unit-test deeply, but is not required because the server always has `zstd`.
- A full XFTP clone is unnecessary; MVP should provide enough remote browsing to select package paths reliably.

## Current Recommendation

MVP should support remote directory browsing, password/private-key auth, local replacement file upload, three-level package-internal target selection, remote command execution, safe new-package output, optional backup-then-overwrite mode, and md5 update/verification. The overwrite option stays disabled by default until the feature proves stable in regular use.
