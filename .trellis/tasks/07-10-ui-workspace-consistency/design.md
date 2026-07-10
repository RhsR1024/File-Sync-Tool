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
