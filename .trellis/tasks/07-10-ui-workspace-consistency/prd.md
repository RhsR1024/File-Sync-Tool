# UI workspace consistency polish

## Goal

Make the desktop workspace feel intentional and consistent in Chinese and English by widening the primary sidebar, giving truncated navigation labels a complete hover fallback, and applying the approved full-width workspace direction to the sync-console configuration tabs and Remote Package Replacement tool.

## Background

- The user selected layout direction **B: full-width workspace** after reviewing a visual comparison.
- `src/components/Sidebar.vue:85` fixes the sidebar at `w-64` (256 px). Navigation labels use `truncate` at `src/components/Sidebar.vue:136`, but the link does not expose the complete translated label as a hover title.
- `src/pages/sync/SyncConsolePage.vue:19` and `src/pages/sync/SyncOverviewPage.vue:268` already mark the console workspace with `sync-console-workspace` and render edge-to-edge within consistent 24 px gutters.
- `src/components/sync/SyncConfigurationEditor.vue:737` independently applies `max-w-4xl mx-auto`, causing the tasks, strategy, and delivery tabs to become a narrow centered column while the overview remains full width.
- `src/pages/RemotePackagePatchPage.vue:458` applies `max-w-7xl mx-auto` and uses tighter `rounded-md`/`rounded-lg` panels than the other tool surfaces. Its desktop grid at line 471 places steps 1/3 in a 400 px column and steps 2/4 in the flexible column, making the target-selection stage cramped and the execution stage visually detached.
- `src/pages/EnableApplianceSshPage.vue:59` already initializes `whitelistSourceMode` to `'all'`, so Allow all is already the runtime default. The template currently renders Local IP first at lines 774-783 and Allow all second at lines 784-793.
- The completed `07-10-sync-console-refactor` task established the sync-console routes, shared configuration store, and domain-specific persistence contracts. Those behaviors are stable and are not part of this task.
- UI/UX Pro Max v2.5.0 recommends a dense enterprise dashboard with a minimal grid, consistent surfaces, responsive gutters, complete fallbacks for truncated content, visible focus states, and 150-300 ms state transitions.

## Requirements

### R1. Sidebar width and complete label access

- Use a responsive desktop width: 288 px at standard desktop widths and 320 px on wide screens.
- Preserve ellipsis as a defensive fallback when the window is narrow or translated copy grows.
- Every sidebar navigation link must expose its complete translated label on hover through a native title and remain correctly named to assistive technology.
- Existing active-state, runtime indicator, keyboard focus, scrolling, and version-chip behavior must remain unchanged.

### R2. Sync-console full-width configuration workspace

- The overview, tasks, strategy, and delivery routes must share the same full-width workspace and horizontal gutter baseline.
- Remove the configuration editor's `max-w-4xl`/`mx-auto` cap.
- Keep cards full width while using responsive internal grids for related controls so long inputs do not become visually unbounded on wide screens.
- Keep the fixed Save Changes action, validation messages, modals, task/server/command-group management, and persistence behavior unchanged.
- Preserve single-column stacking at narrower desktop widths and avoid horizontal page scrolling.

### R3. Remote Package Replacement visual alignment

- Remove the page-level centered maximum-width cap and use the same 24 px workspace gutters as the sync console.
- Align page header, panel radius, border, background, shadow, icon treatment, spacing rhythm, focus treatment, and button hierarchy with the existing tool language.
- Keep connection and remote package browsing side by side on wide screens.
- Make target selection and execution span the full desktop workspace. Target selection must use a responsive internal split so replacement/scan controls and target details both have usable width.
- Preserve the existing four-step order, connection/scan/patch behavior, validation, confirmation, progress, result, and log data flow.

### R4. Scope and compatibility

- Keep all existing route names, translation keys, Tauri commands, stores, configuration contracts, and backend behavior unchanged.
- Reuse Lucide icons and existing Tailwind design language; do not introduce a new UI library or remote web fonts.
- Respect `prefers-reduced-motion` patterns already used by the application.

### R5. Appliance Access whitelist-source order and default

- In Appliance Access Control, render **Allow all** before **Local IP (auto-detect)** under Whitelist source in both languages.
- Keep **Allow all** as the initial/default selection.
- Preserve the existing request contract: Allow all continues to submit `0.0.0.0/0`, while Local IP continues to omit the CIDR and use automatic source detection.
- Add a regression test that locks both option order and the default source mode.

## Acceptance Criteria

- [ ] At 1440 px and wider, the sidebar uses 320 px; at standard desktop widths it uses 288 px.
- [ ] In English, long navigation items such as Remote Package Replacement are either visible in full or show the full label on hover, with no overlap with runtime/active indicators.
- [ ] The sync-console tasks, strategy, and delivery pages no longer contain the `max-w-4xl mx-auto` root constraint and align with the overview/header workspace gutters.
- [ ] Strategy and delivery controls reflow into responsive groups on wide screens and stack without horizontal scrolling on narrower windows.
- [ ] Remote Package Replacement no longer uses a page-level `max-w-7xl mx-auto` container.
- [ ] Remote Package Replacement steps 3 and 4 span the wide-screen grid and use the same panel hierarchy as sync-console configuration cards.
- [ ] Appliance Access Control displays Allow all before Local IP (auto-detect), and a newly opened page selects Allow all by default.
- [ ] Switching to Local IP still uses automatic source detection; Allow all still maps to `0.0.0.0/0`.
- [ ] No route, persistence, configuration, scan, deployment, or package-patch behavior changes.
- [ ] Targeted layout regression tests pass, followed by `pnpm check`, `pnpm lint`, and the production frontend build.
- [ ] The finished UI is visually checked at 1024 px, 1440 px, and 1920 px widths, including keyboard focus and long English labels.

## Out of Scope

- Changes to sync scheduling, configuration persistence, deployment behavior, package replacement logic, or Rust code.
- Redesigning unrelated parts of Appliance Access Control, other tool pages, Settings, runtime logs, or History.
- Adding sidebar collapse/mobile navigation, dark mode, a new typography family, or a new component library.
- Replacing existing native `title` behavior with a custom tooltip framework.
- Updating the repository-vendored UI/UX Pro Max skill copy; the Codex-global v2.5.0 installation is handled separately from product UI code.

## Constraints

- Preserve the user's existing `src-tauri/Cargo.toml` modification and untracked `.codegraph/` directory.
- Trellis 0.6.6 template updates are part of the user's requested tool update and must remain separate from product UI edits during review and commit staging.
- Planning and implementation documentation is written in English per `.trellis/spec/frontend/index.md`; user-facing responses remain Chinese.
