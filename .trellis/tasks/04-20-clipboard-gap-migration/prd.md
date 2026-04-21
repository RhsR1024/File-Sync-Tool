# Clipboard Gap Migration

## Goal
Bring the clipboard manager in `File-Sync-Tool` up to and beyond the `2026-04-19-clipboard-gap-migration-spec.md` target by completing milestones `M6` to `M9`, using `ElegantClipboard-main/` as the reference implementation for any behavior the spec leaves implicit.

## Approved Inputs
- Spec baseline: `docs/superpowers/specs/2026-04-19-clipboard-gap-migration-spec.md`
- Prior design context: `docs/superpowers/specs/2026-04-19-clipboard-manager-design.md`
- Prior plan context: `docs/superpowers/plans/2026-04-19-clipboard-manager.md`
- Reference source: `ElegantClipboard-main/`

## Constraints
- Work in the existing worktree branch: `feature/clipboard-manager`
- Keep `ElegantClipboard-main/` uncommitted and ignored
- Do not implement items explicitly marked as out of scope in the spec
- When the spec is ambiguous, match `ElegantClipboard` behavior first
- Final `M9` delivery must not be worse than `ElegantClipboard` in completeness or performance

## Current Baseline
- Rust backend currently supports text/html/image/file at a basic level, but `watcher.rs`, `db.rs`, `paste.rs`, `commands.rs`, `source.rs`, and `admin.rs` still reflect an early implementation
- Vue frontend currently has a single-store + single-list layout, simple settings panel, and no context menu / groups / pinned split / preview windows
- `.trellis/spec/frontend/*` exists but is mostly placeholder text
- `.trellis/spec/backend/*` does not exist in this worktree, so backend conventions must be inferred from current repository patterns
- Baseline verification is partially blocked by environment issues:
  - `cmd /c pnpm check` fails because `node_modules` is not installed in the worktree
  - `cargo test` requires registry access and currently fails under sandboxed networking

## Acceptance Criteria
- [ ] `M6` delivers html/files/rtf/source-icon support, smarter search coverage, dedup strategies, and DB read/write separation
- [ ] `M7` delivers context menus, file actions, merge paste, range selection, and quick-paste shortcuts
- [ ] `M8` delivers display personalization, highlight/search UX, preview-window rewrite, and panel/window persistence
- [ ] `M9` delivers tray/system integration, non-activating panel display, scheduled-elevation flow, import/export, groups, pinned/favorite split, cleanup tooling, and app filters
- [ ] All new backend changes are covered by targeted Rust tests before implementation claims
- [ ] Frontend type contracts remain synchronized with backend models/commands
- [ ] Final verification includes targeted tests, type-check/build checks, and manual milestone regression lists from the spec

## Implementation Strategy
1. Establish the new data contracts and persistence foundation first
2. Build outward from backend capability to frontend integration
3. Keep milestone boundaries explicit so subagents can execute one bounded task at a time
4. Run spec-first reviews after each subagent task, then code-quality review
