# Clipboard Manager Redesign Execution

## Goal
Implement the approved 2026-04-23 clipboard redesign on `main`:
- unify the Alt+C quick panel frame border
- add self-copy bypass behavior plus source tagging
- expose the new setting in the UI
- turn `/tools/clipboard` into a settings-only page

## Requirements
- Keep Alt+C as the only clipboard CRUD, search, and batch-operation surface.
- Add `reinsert_on_self_copy` to clipboard settings with a default of `false`.
- Add `from_self` to clipboard items and carry it through Rust, SQLite, Tauri serialization, TypeScript types, and list rendering.
- Detect clipboard writes triggered by this app and either skip capture or reinsert as `from_self` based on settings.
- Preserve clipboard import/export fidelity for the new `from_self` field.
- Preserve existing Alt+C capabilities, including paste variants, batch tools, pinned items, and context menu actions.
- Move `/tools/clipboard` to a simplified settings-only layout and remove manager-page-only dead code.

## Acceptance Criteria
- [ ] Alt+C panel renders a consistent four-sided border with a single inner divider below the header.
- [ ] Default self-copy behavior does not reorder clipboard history when copying from Alt+C.
- [ ] Enabled self-copy reinsertion marks the resulting history entry as originating from "This tool".
- [ ] External app captures still show the real source app metadata.
- [ ] `/tools/clipboard` shows only the descriptive header and `ClipboardSettingsPanel`.
- [ ] Clipboard-related Rust tests, `pnpm check`, the source-badge node test, ESLint, and the desktop build pass.

## Technical Notes
- This is a cross-layer change across Vue, TypeScript contracts, Tauri commands, Rust models, watcher/paste behavior, SQLite migration, and import/export mapping.
- Follow plan order: Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6.
- Use TDD for each behavior change by adding failing tests first before implementation.
- Do not touch unrelated untracked generated files already present in the repo root.
