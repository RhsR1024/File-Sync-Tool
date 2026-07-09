# Remote Package Patch Backend Contracts

## Commands

- `remote_package_test_connection(config)` validates SSH credentials and returns the remote `uname -sr` output.
- `remote_package_list_dir(config, path)` lists one remote directory through SFTP. Entries are sorted with directories first.
- `remote_package_pick_local_file(kind)` opens a native local file picker for replacement files or private keys through the Tauri main thread.
- `remote_package_scan_package(config, packagePath)` runs a remote scan script and returns package inventory for middle tar and nested tar.zst layers.
- `remote_package_start_patch(request)` uploads one replacement file, uploads a generated patch script, executes it remotely, and returns output path, backup path, replacement md5, workdir, and updated manifest paths.

## Safety Rules

- Default output mode never overwrites the source package.
- Overwrite mode backs up the source package first and replaces it only after final package verification succeeds.
- Scan and patch workdirs are created beside the selected remote package and are emitted in events/logs.
- Credentials are session-only and must not be written to config or logs.
- md5 updates are exact-path only; same-name files are not batch-updated.
- Scan and patch share one `PATCH_BUSY` guard. Connection test and directory listing do not use that guard.
- Remote archive mutation assumes Linux has `bash`, `tar`, `gzip`, `zstd`, `md5sum`, `df`, `awk`, `cp`, `mv`, and `du`.
