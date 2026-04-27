# Error Code Git2 Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the error-code sync transport with an embedded Git implementation that works on Windows machines without Git installed.

**Architecture:** Keep the existing parser, cache, and query pipeline intact. Only swap the transport layer from "download zip archive" to "clone temp worktree with git2 and collect CSV files".

**Tech Stack:** Rust, git2/libgit2, existing error-code backend modules, Tauri logging.

---

## File Structure

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/error_code/gitlab.rs`
- Modify: `src-tauri/src/error_code/sync.rs`
- Test: `src-tauri/src/error_code/gitlab.rs`
- Test: `src-tauri/src/error_code/sync.rs`

## Tasks

### Task 1: Add embedded Git dependency

- [ ] Add `git2` with vendored features to `src-tauri/Cargo.toml`.
- [ ] Run `cargo check` to update `Cargo.lock`.

### Task 2: Write failing tests first

- [ ] Add a test proving local Git sync can collect only CSV files from a temporary repository.
- [ ] Add a test proving branch fallback from `main` to `master`.
- [ ] Run the targeted test command and confirm the new tests fail before implementation.

### Task 3: Implement Git transport

- [ ] Replace archive URL builders with repository URL + branch candidate builders.
- [ ] Add git2 credential callbacks for username/password auth.
- [ ] Clone into a temp directory with depth `1` and collect CSV files from the worktree.
- [ ] Remove the old web-login fallback code.

### Task 4: Reconnect sync pipeline

- [ ] Update `sync.rs` to consume collected CSV files directly instead of unzipping archive bytes.
- [ ] Keep cache/meta/store update behavior unchanged.

### Task 5: Verify

- [ ] Run targeted error-code tests.
- [ ] Run `cargo test error_code -- --nocapture`.
- [ ] Run `cargo check`.
