# UI workspace consistency polish - technical design

## 1. Architecture and boundaries

This is a presentation-only change. The existing route tree, component ownership, stores, Tauri calls, and persistence contracts remain authoritative. The task changes layout classes and limited template grouping in three existing surfaces:

1. `src/components/Sidebar.vue`
2. `src/components/sync/SyncConfigurationEditor.vue`
3. `src/pages/RemotePackagePatchPage.vue`

No shared layout component will be introduced. Only two tool pages need the full-width correction, and a wrapper abstraction would add indirection without eliminating meaningful behavior. Consistency will come from the same gutter, panel, spacing, and responsive-grid conventions, guarded by source-level layout tests.

## 2. Sidebar design

The sidebar shell changes from `w-64` to `w-72 xl:w-80`, yielding 288 px at standard desktop widths and 320 px at wide widths. It remains a fixed flex child and keeps its current scroll region, header, section containers, footer, active indicator, and shadow.

Each `router-link` receives `:title="item.label"`. The visible label keeps `truncate`, because runtime dots and the active rail must not be pushed out when a translation exceeds even the wider width. The existing text remains the accessible name; the title is a pointer-hover fallback rather than the sole label.

## 3. Sync-console workspace design

`SyncConfigurationEditor.vue` keeps the outer scroll container. Its inner root drops `max-w-4xl mx-auto` and joins the existing semantic workspace marker:

```text
sync-console-workspace min-h-full w-full p-6 space-y-6 pb-24
```

The tasks page remains a full-width list. The strategy page keeps Local Storage full width, because filesystem paths benefit from horizontal room. Scan timing controls use a responsive internal grid at wide breakpoints, while file-extension and filename-filter cards retain their existing two-card grid. Delivery keeps each top-level card full width, with existing server metrics and command/script lists using available horizontal space.

Modal maximum widths are intentionally preserved. Modal reading width and page workspace width solve different problems.

## 4. Remote Package Replacement design

### 4.1 Workspace shell

The page root becomes a full-width workspace with `px-6 py-6`, matching the sync console. The header keeps the title and note but uses the same slate hierarchy, border rhythm, and spacing baseline as other tools.

### 4.2 Step grid

At wide widths, steps 1 and 2 remain a `400px + flexible` row:

```text
Connection                         Remote package browser
Target selection (spans both columns)
Execution and logs (spans both columns)
```

Steps 3 and 4 receive `xl:col-span-2`. Step 3 adds a responsive internal split: replacement/upload and scan actions on the left; discovered candidates, target-path controls, overwrite confirmation, and output details on the right. At narrower widths the split stacks in the original step order.

### 4.3 Panel anatomy

All four steps use the same panel anatomy as sync configuration cards:

- `rounded-xl border border-slate-200 bg-white shadow-sm overflow-hidden`
- a slate-tinted header row with icon/step badge and status/action
- a padded content body
- Lucide icons, existing semantic colors, and visible focus rings

The existing `rpp-input`, primary, secondary, and segment helpers remain, but their radius/focus/shadow values are aligned to the shared card language. Business-state classes and validation messages are untouched.

## 5. Appliance Access whitelist-source adjustment

`EnableApplianceSshPage.vue` already uses the desired default:

```ts
const whitelistSourceMode = ref<'local' | 'all'>('all');
```

The implementation only moves the existing Allow all radio label ahead of Local IP (auto-detect). The state type, default ref, hint visibility, and request mapping remain unchanged:

```text
all   -> whitelistCidr = 0.0.0.0/0
local -> whitelistCidr = undefined -> backend auto-detects the route source IP
```

A data-driven option abstraction is intentionally not introduced for two static choices. Direct template ordering is the smallest mechanism and preserves the existing bindings verbatim.

## 6. Data flow and behavior preservation

No event handler, computed property, store call, Tauri invocation, or translation key changes. Template nodes may be regrouped for layout, but controls keep their current bindings and handlers:

```text
sidebar route label -> existing i18n label -> visible text + title fallback
sync form control -> configStore.config -> existing saveSync path
remote package control -> existing refs/computed values -> existing Tauri commands
```

Layout changes must not alter DOM conditions that gate scan, overwrite confirmation, execution progress, or results.

## 7. Testing strategy

Follow the repository's existing source-level layout-test convention:

- Add a Sidebar layout test asserting responsive width classes, retained truncation, and the translated title binding.
- Extend `SyncConsoleLayout.test.mjs` to load `SyncConfigurationEditor.vue` and reject `max-w-4xl`/`mx-auto` while requiring the shared workspace marker.
- Add a Remote Package Replacement layout test rejecting `max-w-7xl`/page-level centering and requiring full-span classes for steps 3 and 4.
- Add an Appliance Access source-level test asserting that `whitelistSourceMode` initializes to `'all'`, Allow all appears before Local IP in the template, and the existing CIDR mapping remains intact.

Run tests red before implementation, then green after the minimal layout changes. Finish with Vue type checking, ESLint, production build, and visual checks at 1024/1440/1920 px.

## 8. Compatibility, rollback, and risk

- No configuration or data migration is required.
- Rollback is limited to the three UI files and their layout tests.
- The highest-risk area is moving conditional target-selection markup in `RemotePackagePatchPage.vue`; tests and manual state checks must confirm that all existing `v-if` branches and bindings remain intact.
- The sidebar width reduces main-content space by 32-64 px. Responsive grids must therefore be tested at 1024 px to ensure they stack before becoming cramped.
- The Appliance Access change has no backend risk because its default and request mapping already match the requested behavior; the only production change is DOM order.

## 9. Decision record

- Chosen direction: full-width workspace (user-selected option B).
- Sidebar width: responsive 288/320 px rather than a single fixed width.
- Hover fallback: native title, not a new tooltip dependency.
- Reuse strategy: shared conventions and tests, not a new wrapper component.
- Remote step layout: preserve workflow order, widen complex steps instead of inventing new content to fill space.
- Appliance whitelist source: reorder the existing radio labels directly and keep the existing `'all'` initialization and CIDR mapping.

## 10. 2026-07-11 sync-console handoff amendment

This amendment supersedes Sections 3 and 7 only. The sidebar, Remote Package Replacement, and Appliance Access decisions above remain completed historical context.

### 10.1 Source of truth and scope

The visual source of truth is `C:\Users\Z4973\Downloads\同步控制台功能重设计\同步控制台落地稿.dc.html`. Its only import, `support.js`, is the generated Design Component runtime and contributes no product components. The production implementation recreates the prototype's information hierarchy and visual language in Vue/Tailwind while preserving every existing store, Tauri command, persisted configuration field, modal workflow, and task action.

The prototype's 232 px sample sidebar is not part of this change. The previously approved responsive 288/320 px application sidebar remains authoritative.

### 10.2 Navigation and route compatibility

The sync console uses three visible tabs:

1. Overview at `/sync`
2. Tasks and Strategy at `/sync/tasks`
3. Delivery Configuration at `/sync/delivery`

The former `/sync/strategy` URL remains as a named compatibility redirect to `/sync/tasks`. No stored link is allowed to land on a blank or removed page. `SyncTasksPage` owns the combined configuration surface and renders `SyncConfigurationEditor` with a grouped `tasks-strategy` section.

### 10.3 Shared console header

The shell header follows the handoff layout: title and description on the left; scheduler status, next-scan value, and start/stop action on the right; three compact tabs below. Scheduler start/stop moves out of the overview action row so it remains available on every tab. The header reads `appStore.isRunning` and `appStore.nextRunTime` directly and calls the existing `startScheduler()` / `stopScheduler()` functions.

The next-scan indicator does not invent a percentage because the runtime exposes a formatted next-run value, not a stable interval-progress contract. Current transfer speed remains an overview metric and reads the real `appStore.progress.speed`; the empty value is an em dash.

### 10.4 Overview layout and behavior

The overview uses a dense four-cell summary strip for scheduler state, next scan, current transfer speed, and task-record count. The task-record panel retains the existing `TaskGroupsTable` and `TaskGroupDetailPanel`, so pause, resume, cancel, retry run, retry deployment, clear, path inspection, live progress, elapsed time, and detail loading remain intact.

The overview action row contains Scan now, Manual copy, and Clear finished. Manual copy continues to use the production modal, target-existence preview, overwrite/skip decision, queue acknowledgement, and keyboard focus restoration. The prototype's simplified manual-copy form is not copied.

### 10.5 Combined Tasks and Strategy layout

The page uses one full-width vertical workflow at every viewport width. Scan Tasks comes first, followed by the continuous Scan Strategy panel (Local Storage, Scan Timing, and Stability), then Time Ranges and File Filters. Tasks and strategy must not be placed in parallel desktop columns: their content heights vary independently, and an empty or short task list otherwise creates a large unusable blank region beside the longer strategy form. The surface has no horizontal page scrolling.

The compact task rows keep enable/disable, rule summary, remote path, deployment summary, edit, and delete. The existing task modal keeps task-local path override, rule configuration, per-server command-group ordering, local-script binding, and local/remote/parallel post-copy order. The combined page uses the existing cross-route `configStore` object and one shared save action; it does not create a second configuration copy.

The strategy panel must not reuse viewport-based nested `xl`/`2xl` grids from the former page. Local Storage and Scan Timing form one visually continuous full-width Scan Strategy panel. Scan Timing is a vertical stack; only the two stability fields use an intrinsic `auto-fit/minmax(220px, 1fr)` grid, so they wrap from the panel's real width rather than the window width. File-extension and filename-keyword filters remain available below the primary sections and span the full combined workspace.

### 10.6 Delivery layout

Delivery uses the handoff's two-column rhythm while retaining production depth. The left stack owns Remote Deployment and Manual Deployment. The right stack owns Command Groups and Local Post-Copy Scripts. Server detail management, enablement, SSH timeout, individual/batch connection tests, manual server bindings, command ordering, progress, local-script failure policy, and all existing edit dialogs remain available.

### 10.7 Accessibility and responsive rules

- Use semantic buttons and navigation; do not copy the prototype's clickable `div` elements.
- Preserve visible focus rings, translated accessible names, 44 px primary touch targets, and `motion-reduce` behavior.
- Use Lucide icons only; prototype emoji/glyph icons are replaced with the existing icon set.
- Keep slate/blue/emerald/amber/red tokens from the prototype and application; do not use the unrelated gold palette suggested by the generic dashboard search.
- Verify the layout at 1024, 1440, and 1920 px. Tables may scroll inside their panel, but the page must not scroll horizontally.

### 10.8 Documentation handoff

Codex reads repository `AGENTS.md` instructions. `CLAUDE.md` is not an automatic substitute for Codex. The implementation keeps the Trellis-managed block in `AGENTS.md`, then synchronizes the useful project guidance from `CLAUDE.md` and corrects stale sync-console paths instead of deleting Trellis instructions or copying obsolete architecture verbatim.
