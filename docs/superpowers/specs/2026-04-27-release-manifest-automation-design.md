# Release Manifest Automation Design

- **Date**: 2026-04-27
- **Status**: Approved
- **Owner**: codex-agent
- **Scope**: Automate `scripts/release-server/manifest.json` updates after `pnpm tauri:build:versioned-exe`.

---

## Goal

Make `pnpm tauri:build:versioned-exe` do three things in one flow:

1. Build the Tauri release executable.
2. Rename the executable to the versioned `file-sync-tool-<version>-<timestamp>.exe` format.
3. Create or incrementally update `scripts/release-server/manifest.json`.

After the build, the developer should only need to:

1. Fill in `changelog`.
2. Copy the new `.exe` and `manifest.json` to the Linux release directory.

---

## Manifest Contract

The generated `manifest.json` must stay compatible with the existing updater parser in `src-tauri/src/updater/manifest.rs`.

Each entry must contain:

- `version`
- `url`
- `sha256`
- `released_at`
- `changelog`

Top-level fields:

- `latest`
- `versions`

Rules:

- `latest` must point at the newest entry's `version`.
- `versions` must be ordered newest first.
- `url` should contain only the built file name, not an absolute path.
- `released_at` uses `YYYY-MM-DD`.
- `changelog` defaults to `[]`.

---

## Incremental Update Rules

This automation uses **version-based deduplication**.

- If `manifest.json` does not exist, create it with the current version as the first entry.
- If the current `version` does not exist, insert a new entry at the front.
- If the current `version` already exists, update only:
  - `url`
  - `sha256`
  - `released_at`
- If the current `version` already exists, preserve its existing `changelog`.
- Keep all other historical entries intact.

This means repeated builds of the same semantic version replace that version's artifact metadata instead of appending duplicate history rows.

---

## Script Structure

Use a dedicated script:

- `scripts/release-server/update-manifest.mjs`

Keep the existing rename step, but extract shared build-path helpers so both scripts follow the same release-directory and naming rules.

Recommended split:

- `scripts/versioned-exe-utils.mjs`
  - read Tauri/Cargo metadata
  - resolve release directory candidates
  - build the versioned filename pattern
- `scripts/rename-tauri-exe.mjs`
  - rename the freshly built executable
- `scripts/release-server/update-manifest.mjs`
  - find the latest versioned executable
  - compute SHA-256
  - update `manifest.json`

---

## Failure Behavior

- If no versioned executable is found after the rename step, fail the command.
- If `manifest.json` exists but is malformed, fail the command rather than silently overwriting release history.
- If `manifest.json` is missing, create a new one.
- If hashing fails, fail the command.

---

## Non-Goals

- Uploading files to Linux.
- Auto-generating changelog text.
- Keeping multiple history rows for the same semantic version.
- Updating any server-side file outside `scripts/release-server/manifest.json`.
