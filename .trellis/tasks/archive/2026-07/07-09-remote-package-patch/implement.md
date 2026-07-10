# Remote Package Patch Implementation Plan

The detailed implementation plan is maintained at:

- `docs/superpowers/plans/2026-07-09-remote-package-patch-implementation.md`

Use that plan as the task-by-task execution source. Trellis inline mode skips jsonl curation; load backend/frontend specs through `trellis-before-dev` before editing.

## Validation Commands

- `cargo test --manifest-path src-tauri/Cargo.toml -p app remote_package_patch`
- `node --test src/lib/remotePackagePatch.test.mjs`
- `node src/lib/sidebarNavigation.test.mjs`
- `pnpm check`

## Current Gate

Implementation is in progress. Core backend commands, remote scripts, frontend API contracts, workbench UI, navigation entry, fixture generator, and backend spec should all pass the validation commands above before completion.
