# Release Manifest Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `pnpm tauri:build:versioned-exe` automatically create or incrementally update `scripts/release-server/manifest.json` using version-based deduplication.

**Architecture:** Extract shared Tauri release-path helpers, keep the rename step focused on renaming, and add a dedicated manifest updater that hashes the latest versioned executable and merges it into the existing release manifest while preserving historical changelog content.

**Tech Stack:** Node.js ESM scripts, `node:test`, `node:crypto`, `node:fs/promises`, `pnpm`, Tauri build metadata from `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`.

---

### Task 1: Write failing manifest automation tests

**Files:**
- Create: `scripts/release-server/update-manifest.test.mjs`

- [ ] **Step 1: Add a fixture that simulates a built versioned exe**

Create a temporary project with:

- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`
- `src-tauri/target/release/file-sync-tool-1.0.7-202604271200.exe`

- [ ] **Step 2: Add a failing test for first-run manifest creation**

Test expectations:

- running the new script exits `0`
- `scripts/release-server/manifest.json` is created
- `latest === "1.0.7"`
- `versions.length === 1`
- `versions[0].url` equals the built exe filename
- `versions[0].sha256` is populated
- `versions[0].released_at` is today
- `versions[0].changelog` is `[]`

- [ ] **Step 3: Add a failing test for version-based incremental update**

Seed an existing `manifest.json` with:

- one older `1.0.6` entry
- one current `1.0.7` entry with a non-empty `changelog`

Test expectations after running the script:

- `latest === "1.0.7"`
- `versions.length` stays unchanged
- the `1.0.7` entry moves or stays at the front
- its `url`, `sha256`, and `released_at` are refreshed
- its existing `changelog` is preserved
- the older `1.0.6` entry remains intact

- [ ] **Step 4: Add a failing test for malformed manifest protection**

Seed invalid JSON and assert the script exits non-zero instead of overwriting history.

- [ ] **Step 5: Run the tests and confirm they fail**

Run:

```bash
node --test scripts/release-server/update-manifest.test.mjs
```

Expected:

- failures because the new updater script does not exist yet

---

### Task 2: Implement the shared helpers and manifest updater

**Files:**
- Create: `scripts/versioned-exe-utils.mjs`
- Modify: `scripts/rename-tauri-exe.mjs`
- Create: `scripts/release-server/update-manifest.mjs`

- [ ] **Step 1: Extract shared versioned-exe helpers**

Move shared logic out of `rename-tauri-exe.mjs` so both scripts can reuse:

- Tauri product/version loading
- Cargo binary-name loading
- release directory resolution
- versioned exe filename matching

- [ ] **Step 2: Implement manifest merge behavior**

In `update-manifest.mjs`, implement:

- locate the newest versioned exe for the current version
- hash it with SHA-256
- read existing `manifest.json` if present
- create/update the current version entry
- preserve `changelog` for an existing same-version entry
- sort newest-first with the current version forced to the front
- write pretty JSON back to `scripts/release-server/manifest.json`

- [ ] **Step 3: Keep the CLI strict**

The command should fail when:

- the exe cannot be found
- the existing manifest is malformed
- the file hash cannot be computed

- [ ] **Step 4: Re-run the tests**

Run:

```bash
node --test scripts/release-server/update-manifest.test.mjs scripts/rename-tauri-exe.test.mjs
```

Expected:

- all tests pass

---

### Task 3: Wire the build command and update docs

**Files:**
- Modify: `package.json`
- Modify: `scripts/release-server/README.md`
- Modify: `scripts/release-server/UPDATE_DEPLOYMENT_GUIDE.md`

- [ ] **Step 1: Append the manifest updater to the custom build command**

Update `tauri:build:versioned-exe` to run:

```json
"pnpm tauri build && node scripts/rename-tauri-exe.mjs && node scripts/release-server/update-manifest.mjs"
```

- [ ] **Step 2: Update the release-server README**

Describe the new flow:

- build command now also updates `manifest.json`
- developer only fills `changelog`
- copy the exe plus `manifest.json` to Linux

- [ ] **Step 3: Update the deployment guide**

Clarify:

- `pnpm tauri:build:versioned-exe` is a project custom command
- `manifest.json` now auto-creates/auto-updates locally
- same-version rebuilds are incremental updates, not extra history rows

- [ ] **Step 4: Run targeted verification**

Run:

```bash
node --test scripts/release-server/update-manifest.test.mjs scripts/rename-tauri-exe.test.mjs
pnpm check
```

Expected:

- script tests pass
- frontend type-check still passes

- [ ] **Step 5: Optional build smoke test**

Run:

```bash
cmd /c pnpm tauri:build:versioned-exe
```

Expected:

- a versioned `.exe` is produced
- `scripts/release-server/manifest.json` is created or updated

If this is too expensive for the current turn, report that clearly instead of claiming it ran.
