# UI/UX Polish Master Plan

- **Date**: 2026-04-26
- **Owner**: codex-agent (driven by Claude main)
- **Scope**: A non-functional UI/UX refinement pass across the entire app, executed module by module.
- **Hard constraint**: No functional change. Every change must be reversible without altering routing, state shape, command APIs, event names, or business behavior. If a change might break a feature, it goes to a future plan.

---

## 1. Working Principles

1. **Don't refactor architecture** — `SettingsPage.vue` (1946 lines) and `CodeStatisticsPage.vue` (1887 lines) are *not* getting split into sub-routes here. Polish them in place.
2. **Don't replace working components** — extend `Empty.vue`, don't replace it. Add a shared `Toast.vue` that pages can opt into; don't yank the existing `showToast` / `showStatus` / `showStatusMsg` helpers until pages migrate voluntarily.
3. **Tokens before pixels** — define a small set of design tokens (radius scale, shadow scale, spacing rhythm) and apply them consistently. Don't hand-tune random values.
4. **Accessibility is mandatory, not optional** — every interactive element needs keyboard reachability, focus rings, and screen-reader-friendly labels. Color is never the sole signal.
5. **Each module is a PR-sized chunk** — typical module is 2-6 files, 1-3 hours of work, mergeable independently. No module depends on another except M01 (foundations) which other modules consume.
6. **Each module ships verification** — type-check + node tests + manual screenshot before merge.

---

## 2. Existing visual vocabulary (audit summary)

Already in use across the codebase — keep these, formalize as tokens:

| Token | Existing values | Action |
|---|---|---|
| Border radius | `rounded-lg`, `rounded-xl`, `rounded-2xl`, `rounded-[20px]`, `rounded-[24px]`, `rounded-[28px]` | Standardize: card=`rounded-2xl`, button=`rounded-xl`, chip=`rounded-full`, hero=`rounded-[24px]` |
| Shadow | `shadow-sm`, `shadow-lg`, `shadow-2xl`, `shadow-[0_18px_60px_rgba(15,23,42,0.08)]` | Three tiers: `shadow-sm` (resting), `shadow-[0_14px_40px_rgba(15,23,42,0.06)]` (cards), `shadow-[0_18px_60px_rgba(15,23,42,0.08)]` (heroes) |
| Surfaces | `bg-white`, `bg-white/85`, `bg-white/90`, `bg-slate-50`, `bg-[#0b1220]` (sidebar), `bg-[#0f172a]` (terminal) | Light surfaces use opacity 85-95% over the gradient backdrop; dark surfaces stay opaque |
| Accent gradients | 8+ tool-specific pairs (amber→orange, sky→blue, emerald→teal, etc.) | **Keep per-tool gradients** — they're a deliberate identity device. Just verify each pair has 4.5:1 contrast for icon glyph |
| Neutrals | Slate 50→950 | Body text `slate-700`, headings `slate-950`, muted `slate-500`, borders `slate-200` |
| Icons | `lucide-vue-next` exclusively, mostly h-4/h-5 | Lock icon size scale: 14px (inline), 16px (button), 20px (header), 24px (feature) |

**Anti-patterns to remove on contact:**
- ASCII `-` used as em-dash (replace with `—`)
- `'...'` instead of `'…'` for ellipsis in i18n strings
- Empty cells rendered as blank space instead of muted dash
- Hardcoded zh strings (e.g. SettingsPage built-in command groups)
- Buttons without focus rings
- Icon-only buttons without `aria-label` / `title`
- Modal/dialog without `role="dialog"` + `aria-modal="true"` + ESC dismissal

---

## 3. Module list

Modules are tagged by phase. **Phase 1 must complete before pages start migrating to shared primitives** (Toast, LoadingSkeleton, formalized Empty). All other phases parallelize.

### Phase 1 — Foundations (sequential; do M01 first)

| ID | Title | Files (rough) | Why first |
|---|---|---|---|
| M01 | Design tokens & shared primitives | new: `src/lib/uiTokens.ts`, `src/components/Toast.vue`, `src/components/LoadingSkeleton.vue`; extend `Empty.vue` | Other modules depend on these |
| M02 | Sidebar & app shell a11y | `App.vue`, `Sidebar.vue` | Frame visible on every page; high blast radius if done last |

### Phase 2 — Standard pages (parallelizable after M01)

| ID | Title | Files |
|---|---|---|
| M03 | Console & History polish | `MainConsole.vue`, `HistoryPage.vue` |
| M04 | Tasks & Manual copy polish | `TaskStatusPage.vue`, `ManualCopyPage.vue`, `ManualCopyModal.vue`, `TaskGroupsTable.vue`, `TaskGroupDetailPanel.vue` |
| M05 | Tools hub & Error code lookup polish | `ToolsHubPage.vue`, `ErrorCodeLookupPage.vue` |
| M06 | About & Update flow polish | `AboutPage.vue`, `UpdateDialog.vue`, `UpdateRedDot.vue` |

### Phase 3 — Heavy pages (parallelizable after M01)

| ID | Title | Files |
|---|---|---|
| M07 | Settings page in-place polish (no refactor) | `SettingsPage.vue` |
| M08 | Disk cache cleanup polish | `DiskCacheCleanupPage.vue` |
| M09 | Code statistics polish | `CodeStatisticsPage.vue`, `CodeStatisticsScopeTreeNode.vue` |

### Phase 4 — Specialty pages (parallelizable after M01)

| ID | Title | Files |
|---|---|---|
| M10 | Network tools polish | `NetworkToolsPage.vue` + 5 tabs |
| M11 | Frameworks polish | `FrameworkPasswordPage.vue`, `EnableApplianceSshPage.vue` |
| M12 | LAN sharing polish | `FileSharePage.vue`, `ScreenSharePage.vue` |

### Phase 5 — Clipboard subsystem (parallelizable after M01)

| ID | Title | Files |
|---|---|---|
| M13 | Clipboard panel polish | `ClipboardPanelPage.vue`, `ClipboardList.vue`, `ClipboardToolbar.vue`, `ClipboardSearchBox.vue`, hover/menu/highlight components |
| M14 | Clipboard settings tabs polish | `GeneralTab.vue`, `DataTab.vue`, `DisplayTab.vue`, `AppFilterTab.vue`, `PreviewTab.vue`, `ShortcutsTab.vue` |

---

## 4. Common deliverables per module

Each module spec (`Mxx-*.md`) MUST contain:

1. **Goal** — one paragraph stating what the module is and isn't.
2. **Files** — exact paths.
3. **Issues** — numbered list of concrete observations with file:line citations.
4. **Recommended fixes** — paired 1:1 with issues; describe the change, not the code.
5. **Out of scope** — what NOT to touch.
6. **Verification** — type-check / tests / manual checklist.

After a subagent completes a module, the human verifies by:
- `pnpm check` clean
- `node --test` clean for any test files in the module
- Visual smoke: launch `pnpm dev`, exercise the affected page, confirm no functional regression
- If the module touches Tauri: `cargo test ... -p app <module>` clean

---

## 5. Cross-cutting acceptance criteria (every module)

These apply to every module unless the module spec explicitly carves an exception:

- All interactive elements have visible focus rings (`focus-visible:ring-2 focus-visible:ring-offset-2`).
- No icon-only button without `aria-label` (and `title` for tooltip parity).
- All text 14px+ for body content; meta text 12px is OK in chips/captions only.
- Color contrast ≥4.5:1 for body text, ≥3:1 for large/heading text and icons.
- Loading states for any async operation > 300ms.
- Empty states use the shared `Empty.vue` with consistent props.
- Modals/dialogs have `role="dialog"`, `aria-modal="true"`, ESC dismissal, focus trap.
- All zh and en i18n keys exist in pairs (no missing translations).
- Em-dash `—` replaces ASCII `-` for "no value" placeholders.
- Ellipsis `…` replaces ASCII `...` in i18n strings.
- `prefers-reduced-motion` respected for any new animation.

---

## 6. Execution policy

- The main agent (Claude) writes per-module specs from this overview.
- Each module is dispatched to a `general-purpose` subagent with the spec attached. Subagent applies changes, runs verification, reports back.
- The human reviews each module's diff before commit. **Subagents do NOT commit.**
- If a subagent finds issues outside its module that block the work, it reports back without modifying out-of-scope files.

---

## 7. Status tracker

| Module | Spec written | Subagent dispatched | Verified | Committed |
|---|---|---|---|---|
| M01 | ✅ | ✅ | ✅ pnpm check + 6/6 + 12/12 | ☐ |
| M02 | ✅ | ✅ | ✅ pnpm check + 18/18 | ☐ |
| M03 | ✅ | ✅ | ✅ pnpm check + 18/18 | ☐ |
| M04 | ✅ | ✅ | ✅ pnpm check + 18/18 (agent crashed during report; work landed) | ☐ |
| M05 | ✅ | ✅ | ✅ pnpm check + 18/18 | ☐ |
| M06 | ✅ | ✅ | ✅ pnpm check + 18/18 | ☐ |
| M07 | ✅ | ✅ | ✅ pnpm check + 18/18 | ☐ |
| M08 | ✅ | ✅ | ✅ pnpm check + 19/19 + cargo 16/16 | ☐ |
| M09 | ✅ | ✅ | ✅ pnpm check + 18/18 | ☐ |
| M10 | ✅ | ☐ | ☐ | ☐ |
| M11 | ✅ | ☐ | ☐ | ☐ |
| M12 | ✅ | ☐ | ☐ | ☐ |
| M13 | ✅ | ☐ | ☐ | ☐ |
| M14 | ✅ | ☐ | ☐ | ☐ |
