# Screen Share Backend Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add explicit screen-capture backend modes so users can choose automatic fallback, WGC-first compatibility, or DXGI-only performance mode.

**Architecture:** Extend the Tauri `ScreenShareConfig` contract with a serialized backend-mode enum, then route capture-source creation through mode-aware backend selection and retry windows. Surface the same mode in the Vue screen-share page with saved settings and explanatory copy so users understand the trade-off before starting sharing.

**Tech Stack:** Rust/Tauri, Vue 3, TypeScript, vue-i18n, node:test, cargo test

---

### Task 1: Lock Behavior with Tests

**Files:**
- Modify: `src-tauri/src/screenshare.rs`
- Modify: `src/pages/ScreenSharePage.test.mjs`

- [ ] Add Rust unit tests that describe the backend-mode labels and automatic retry-window budget.
- [ ] Add page-source tests that require the three translated mode labels plus descriptive helper text to appear in `ScreenSharePage.vue`.
- [ ] Run the targeted tests first and confirm they fail because the mode feature does not exist yet.

### Task 2: Implement Mode-Aware Capture Selection

**Files:**
- Modify: `src-tauri/src/screenshare.rs`

- [ ] Add a serializable backend-mode enum to `ScreenShareConfig` with a default automatic mode.
- [ ] Split DXGI retry delays into mode-aware policies so automatic mode uses a short DXGI probe window before WGC fallback while DXGI-only retains the full retry window.
- [ ] Update startup and recreation capture creation paths to honor the selected mode and keep backend-selection diagnostics precise.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml screenshare` or the closest available targeted command and confirm the new tests pass.

### Task 3: Expose the Mode in the Vue UI

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/pages/ScreenSharePage.vue`
- Modify: `src/locales/messages.ts`

- [ ] Extend the frontend `ScreenShareConfig` type plus saved settings with the backend mode field.
- [ ] Add the three-option selector to the performance section with the exact labels approved by the user.
- [ ] Add concise helper copy describing what each mode does, keeping “自动（推荐）” clearly recommended.
- [ ] Run the page-source test file and any relevant typecheck command for the touched files.

### Task 4: Capture the Cross-Layer Contract

**Files:**
- Modify: `.trellis/spec/backend/screen-share.md`

- [ ] Update the backend spec so it documents the new `capture_backend_mode` contract, automatic-mode retry budget, and explicit DXGI-only/WGC-only behavior.
- [ ] Re-run the targeted verification commands and record any remaining risks if full app-level manual testing is still pending.
